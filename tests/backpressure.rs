mod common;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
};
use clap::Parser;
use common::{TestServer, TestWorkspace};
use kube_bug_agent::{
    application::QueryLimit,
    backpressure::{
        adapters::{EnvoyAdmin, EnvoyAdminEndpoint, EnvoyStatsDto, SqliteDecisionRepository},
        application::ControlBackpressure,
        cli::BackpressureCli,
        domain::*,
        ports::{ConcurrencyActuator, DecisionRepository, PressureSource},
    },
    domain::EventId,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

const KEY: &str = "circuit_breakers.orders.default.max_requests";
fn limit(value: u32) -> ConcurrencyLimit {
    ConcurrencyLimit::new(value).unwrap()
}
fn controller() -> BackpressureController {
    BackpressureController::new(
        BackendName::new("orders").unwrap(),
        BackpressurePolicy {
            minimum: limit(1),
            maximum: limit(8),
            latency_budget: Latency::milliseconds(200.0).unwrap(),
            error_budget: Ratio::new(0.1).unwrap(),
            minimum_samples: RequestCount::new(10),
            healthy_windows: WindowCount::new(2).unwrap(),
            cooldown: Duration::from_millis(100),
        },
    )
    .unwrap()
}
fn stats(completed: u64, failed: u64, p95: Option<f64>) -> Value {
    json!({"stats":[
        {"name":"server.uptime","value":100},
        {"name":"cluster.orders.upstream_rq_active","value":1},
        {"name":"cluster.orders.upstream_rq_pending_active","value":0},
        {"name":"cluster.orders.upstream_rq_completed","value":completed},
        {"name":"cluster.orders.upstream_rq_5xx","value":failed},
        {"histograms":{"supported_quantiles":[50,95],"computed_quantiles":[
            {"name":"cluster.orders.upstream_rq_time","values":[{"interval":10},{"interval":p95}]}
        ]}}
    ]})
}
#[derive(Clone)]
struct FakeEnvoy {
    stats: Arc<Mutex<Value>>,
    limit: Arc<Mutex<u32>>,
    ignore_writes: bool,
    fail_writes: bool,
}
impl FakeEnvoy {
    fn new() -> Self {
        Self {
            stats: Arc::new(Mutex::new(stats(0, 0, None))),
            limit: Arc::new(Mutex::new(8)),
            ignore_writes: false,
            fail_writes: false,
        }
    }
    async fn server(&self) -> TestServer {
        TestServer::new(Router::new()
            .route("/stats", get(|State(fake):State<FakeEnvoy>, Query(query):Query<HashMap<String,String>>| async move {
                assert_eq!(query.get("format").unwrap(), "json");
                assert!(query.get("filter").unwrap().contains("orders"));
                Json(fake.stats.lock().unwrap().clone())
            }))
            .route("/runtime", get(|State(fake):State<FakeEnvoy>| async move {
                Json(json!({"entries":{KEY:{"final_value":fake.limit.lock().unwrap().to_string()}}}))
            }))
            .route("/runtime_modify", post(|State(fake):State<FakeEnvoy>, Query(query):Query<HashMap<String,String>>| async move {
                assert_eq!(query.len(), 1);
                if fake.fail_writes { return StatusCode::SERVICE_UNAVAILABLE; }
                if !fake.ignore_writes { *fake.limit.lock().unwrap()=query[KEY].parse().unwrap(); }
                StatusCode::OK
            }))
            .with_state(self.clone())).await
    }
}

#[tokio::test]
async fn controller_applies_envoy_limit_and_persists_audit_with_real_http_and_sqlite() {
    let workspace = TestWorkspace::new();
    let path = workspace.path("audit.db");
    let fake = FakeEnvoy::new();
    let server = fake.server().await;
    let envoy = Arc::new(EnvoyAdmin::new(EnvoyAdminEndpoint::new(server.url()).unwrap()).unwrap());
    let repository = Arc::new(SqliteDecisionRepository::open(&path).unwrap());
    let mut control = ControlBackpressure::new(
        controller(),
        envoy.clone(),
        envoy,
        repository.clone(),
        false,
    );
    assert_eq!(
        control.tick().await.unwrap().status,
        ExecutionStatus::Observed
    );
    *fake.stats.lock().unwrap() = stats(20, 5, Some(50.0));
    let reduced = control.tick().await.unwrap();
    assert_eq!(reduced.status, ExecutionStatus::Applied);
    assert_eq!(*fake.limit.lock().unwrap(), 4);
    tokio::time::sleep(Duration::from_millis(110)).await;
    *fake.stats.lock().unwrap() = stats(40, 5, Some(50.0));
    assert_eq!(
        control.tick().await.unwrap().decision.reason,
        DecisionReason::Hold
    );
    *fake.stats.lock().unwrap() = stats(60, 5, Some(50.0));
    assert_eq!(
        control.tick().await.unwrap().decision.reason,
        DecisionReason::Recover
    );
    assert_eq!(*fake.limit.lock().unwrap(), 5);
    let reopened = SqliteDecisionRepository::open(&path).unwrap();
    let history = reopened.recent(QueryLimit::new(20).unwrap()).await.unwrap();
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].status, ExecutionStatus::Applied);
    repository.prepare(&reduced.decision).await.unwrap();
    let mut conflicting = reduced.decision.clone();
    conflicting.after = limit(3);
    assert!(repository.prepare(&conflicting).await.is_err());
    assert!(
        repository
            .finish(&reduced.decision.id, ExecutionStatus::Observed)
            .await
            .is_err()
    );
    repository
        .finish(&reduced.decision.id, ExecutionStatus::Applied)
        .await
        .unwrap();
    assert!(
        repository
            .finish(&reduced.decision.id, ExecutionStatus::Pending)
            .await
            .is_err()
    );
    assert!(
        repository
            .finish(&EventId::new("missing").unwrap(), ExecutionStatus::Failed)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn dry_run_proposes_without_mutation_and_failed_actuation_is_audited() {
    for (dry_run, ignore_writes, fail_writes) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let workspace = TestWorkspace::new();
        let mut fake = FakeEnvoy::new();
        fake.ignore_writes = ignore_writes;
        fake.fail_writes = fail_writes;
        let server = fake.server().await;
        let envoy =
            Arc::new(EnvoyAdmin::new(EnvoyAdminEndpoint::new(server.url()).unwrap()).unwrap());
        let repository =
            Arc::new(SqliteDecisionRepository::open(&workspace.path("audit.db")).unwrap());
        let mut control = ControlBackpressure::new(
            controller(),
            envoy.clone(),
            envoy,
            repository.clone(),
            dry_run,
        );
        control.tick().await.unwrap();
        *fake.stats.lock().unwrap() = stats(20, 5, Some(50.0));
        let result = control.tick().await;
        assert_eq!(result.is_ok(), dry_run);
        assert_eq!(*fake.limit.lock().unwrap(), 8);
        let latest = repository
            .recent(QueryLimit::new(1).unwrap())
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(
            latest.status,
            if dry_run {
                ExecutionStatus::DryRun
            } else {
                ExecutionStatus::Failed
            }
        );
    }
}

#[tokio::test]
async fn endpoint_parser_and_adapter_reject_unsafe_or_invalid_admin_data() {
    for url in [
        "http://example.com:9901",
        "http://localhost:9901",
        "https://127.0.0.1:9901",
        "http://u:p@127.0.0.1:9901",
        "http://127.0.0.1:9901/a",
        "http://127.0.0.1:9901?x=y",
        "http://127.0.0.1:9901#x",
        "not a url",
    ] {
        assert!(EnvoyAdminEndpoint::new(url).is_err(), "{url}");
    }
    assert!(EnvoyAdminEndpoint::new("http://[::1]:9901").is_ok());
    let fake = FakeEnvoy::new();
    let server = fake.server().await;
    let envoy = EnvoyAdmin::new(EnvoyAdminEndpoint::new(server.url()).unwrap()).unwrap();
    let backend = BackendName::new("orders").unwrap();
    assert!(envoy.apply(&backend, limit(2), limit(1)).await.is_err());
    assert_eq!(*fake.limit.lock().unwrap(), 8);
    assert!(
        envoy
            .current(&BackendName::new("missing").unwrap())
            .await
            .is_err()
    );
    assert!(
        envoy
            .sample(&BackendName::new("missing").unwrap())
            .await
            .is_err()
    );
    *fake.stats.lock().unwrap() = json!({"stats":[{"name":"server.uptime","value":"invalid"}]});
    assert!(envoy.sample(&backend).await.is_err());
    let duplicate =
        json!({"stats":[{"name":"server.uptime","value":1},{"name":"server.uptime","value":2}]});
    assert!(
        serde_json::from_value::<EnvoyStatsDto>(duplicate)
            .unwrap()
            .into_snapshot(&backend)
            .is_err()
    );
    let absent_uptime = json!({"stats":[]});
    assert!(
        serde_json::from_value::<EnvoyStatsDto>(absent_uptime)
            .unwrap()
            .into_snapshot(&backend)
            .is_err()
    );
    let bad =
        TestServer::new(Router::new().fallback(|| async { StatusCode::SERVICE_UNAVAILABLE })).await;
    let envoy = EnvoyAdmin::new(EnvoyAdminEndpoint::new(bad.url()).unwrap()).unwrap();
    assert!(envoy.current(&backend).await.is_err());
    let large = TestServer::new(Router::new().fallback(|| async { "X".repeat(1_048_577) })).await;
    let envoy = EnvoyAdmin::new(EnvoyAdminEndpoint::new(large.url()).unwrap()).unwrap();
    assert!(envoy.current(&backend).await.is_err());
}

#[tokio::test]
async fn cli_validates_configuration_and_history_and_once_do_not_mutate() {
    let workspace = TestWorkspace::new();
    let path = workspace.path("cli.db");
    let fake = FakeEnvoy::new();
    let server = fake.server().await;
    let cli = BackpressureCli::try_parse_from([
        "agent",
        "run",
        "--database",
        path.to_str().unwrap(),
        "--envoy-admin",
        server.url(),
        "--once",
        "--dry-run",
    ])
    .unwrap();
    assert_eq!(cli.execute().await.unwrap()["status"], "dry_run");
    let history = BackpressureCli::try_parse_from([
        "agent",
        "history",
        "--database",
        path.to_str().unwrap(),
        "--limit",
        "1",
    ])
    .unwrap();
    assert_eq!(
        history.execute().await.unwrap().as_array().unwrap().len(),
        1
    );
    for args in [
        ["agent", "run", "--min-concurrency", "0"],
        ["agent", "run", "--error-ratio", "2"],
        ["agent", "run", "--healthy-windows", "0"],
    ] {
        let cli = BackpressureCli::try_parse_from(args).unwrap();
        assert!(cli.execute().await.is_err());
    }
    assert!(BackpressureCli::try_parse_from(["agent", "run", "--poll-seconds", "0"]).is_err());
    assert!(BackpressureCli::try_parse_from(["agent", "history", "--limit", "0"]).is_err());
}

#[tokio::test]
async fn audit_failure_prevents_actuation_and_missing_telemetry_is_not_a_healthy_window() {
    let workspace = TestWorkspace::new();
    let path = workspace.path("audit.db");
    let fake = FakeEnvoy::new();
    let server = fake.server().await;
    let envoy = Arc::new(EnvoyAdmin::new(EnvoyAdminEndpoint::new(server.url()).unwrap()).unwrap());
    let repository = Arc::new(SqliteDecisionRepository::open(&path).unwrap());
    let mut control =
        ControlBackpressure::new(controller(), envoy.clone(), envoy, repository, false);
    control.tick().await.unwrap();
    *fake.stats.lock().unwrap() = json!({"stats": []});
    assert!(control.tick().await.is_err());
    assert_eq!(*fake.limit.lock().unwrap(), 8);
    rusqlite::Connection::open(&path).unwrap().execute_batch(
        "CREATE TRIGGER deny_audit BEFORE INSERT ON control_decisions BEGIN SELECT RAISE(ABORT,'audit unavailable'); END;",
    ).unwrap();
    *fake.stats.lock().unwrap() = stats(20, 5, Some(50.0));
    assert!(control.tick().await.is_err());
    assert_eq!(*fake.limit.lock().unwrap(), 8);
}

#[tokio::test]
async fn real_mitigator_binary_applies_limit_handles_sigterm_and_reports_invalid_inputs() {
    let workspace = TestWorkspace::new();
    let path = workspace.path("process.db");
    let fake = FakeEnvoy::new();
    let server = fake.server().await;
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_backpressure-agent"));
    command
        .args([
            "run",
            "--envoy-admin",
            server.url(),
            "--database",
            path.to_str().unwrap(),
            "--poll-seconds",
            "1",
            "--cooldown-seconds",
            "1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if path.exists()
                && let Ok(repository) = SqliteDecisionRepository::open(&path)
                && let Ok(items) = repository.recent(QueryLimit::new(1).unwrap()).await
                && !items.is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    *fake.stats.lock().unwrap() = stats(20, 5, Some(50.0));
    tokio::time::timeout(Duration::from_secs(5), async {
        while *fake.limit.lock().unwrap() != 4 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    *fake.stats.lock().unwrap() = json!({"stats": []});
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(
        tokio::process::Command::new("kill")
            .args(["-TERM", &child.id().unwrap().to_string()])
            .status()
            .await
            .unwrap()
            .success()
    );
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["status"],
        "stopped"
    );
    assert_eq!(*fake.limit.lock().unwrap(), 4);
    let history = tokio::process::Command::new(env!("CARGO_BIN_EXE_backpressure-agent"))
        .args(["history", "--database", path.to_str().unwrap()])
        .output()
        .await
        .unwrap();
    assert!(history.status.success());
    assert!(
        serde_json::from_slice::<Vec<Value>>(&history.stdout)
            .unwrap()
            .iter()
            .any(|item| item["status"] == "applied")
    );
    let invalid = tokio::process::Command::new(env!("CARGO_BIN_EXE_backpressure-agent"))
        .args(["run", "--min-concurrency", "0", "--once"])
        .output()
        .await
        .unwrap();
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("ConcurrencyLimit"));
}
