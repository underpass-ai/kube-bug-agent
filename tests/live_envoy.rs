mod common;
use axum::{Router, extract::State, http::StatusCode, routing::get};
use common::{TestServer, TestWorkspace};
use kube_bug_agent::{
    application::QueryLimit,
    backpressure::{
        adapters::{EnvoyAdmin, EnvoyAdminEndpoint, SqliteDecisionRepository},
        application::ControlBackpressure,
        domain::*,
        ports::{ConcurrencyActuator, DecisionRepository},
    },
};
use serde_json::{Value, json};
use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Clone)]
struct SaturatedBackend {
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    failures: Arc<AtomicUsize>,
}
struct EnvoyContainer(String);
impl Drop for EnvoyContainer {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "--force", &self.0])
            .output();
    }
}
fn unused_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
async fn wave(client: &reqwest::Client, url: &str) -> (usize, usize) {
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..32 {
        let client = client.clone();
        let url = url.to_owned();
        tasks.spawn(async move {
            let response = client.get(url).send().await.unwrap();
            (
                response.status().is_success(),
                response.headers().contains_key("x-envoy-overloaded"),
            )
        });
    }
    let (mut succeeded, mut rejected) = (0, 0);
    while let Some(result) = tasks.join_next().await {
        let (success, overloaded) = result.unwrap();
        succeeded += usize::from(success);
        rejected += usize::from(overloaded);
    }
    (succeeded, rejected)
}

#[tokio::test]
#[ignore = "requires Docker and envoyproxy/envoy:v1.39.3; invokes a real Envoy"]
async fn real_envoy_reduces_backend_overload_and_recovers_gradually() {
    let workspace = TestWorkspace::new();
    let state = SaturatedBackend {
        active: Arc::new(AtomicUsize::new(0)),
        peak: Arc::new(AtomicUsize::new(0)),
        failures: Arc::new(AtomicUsize::new(0)),
    };
    let backend = TestServer::new(
        Router::new()
            .route(
                "/",
                get(|State(state): State<SaturatedBackend>| async move {
                    let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
                    state.peak.fetch_max(active, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    state.active.fetch_sub(1, Ordering::SeqCst);
                    if active > 2 {
                        state.failures.fetch_add(1, Ordering::SeqCst);
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        StatusCode::OK
                    }
                }),
            )
            .with_state(state.clone()),
    )
    .await;
    let mut bootstrap: Value =
        serde_json::from_str(include_str!("../deploy/backpressure/envoy.json")).unwrap();
    let admin_port = unused_port();
    let proxy_port = unused_port();
    bootstrap["admin"]["address"]["socket_address"]["port_value"] = json!(admin_port);
    bootstrap["static_resources"]["listeners"][0]["address"]["socket_address"]["port_value"] =
        json!(proxy_port);
    bootstrap["static_resources"]["clusters"][0]["load_assignment"]["endpoints"][0]["lb_endpoints"]
        [0]["endpoint"]["address"]["socket_address"]["port_value"] =
        json!(reqwest::Url::parse(backend.url()).unwrap().port().unwrap());
    let config = workspace.path("envoy.json");
    std::fs::write(&config, serde_json::to_vec_pretty(&bootstrap).unwrap()).unwrap();
    let name = format!("backpressure-test-{}", uuid::Uuid::new_v4());
    let _container = EnvoyContainer(name.clone());
    let mount = format!("{}:/etc/envoy/envoy.json:ro", config.display());
    let result = Command::new("docker")
        .args([
            "run",
            "--detach",
            "--name",
            &name,
            "--network",
            "host",
            "--read-only",
            "--user",
            "10001:10001",
            "--entrypoint",
            "envoy",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
            "--tmpfs",
            "/tmp",
            "--mount",
            &format!(
                "type=bind,source={},target=/etc/envoy/envoy.json,readonly",
                config.display()
            ),
            "envoyproxy/envoy:v1.39.3",
            "-c",
            "/etc/envoy/envoy.json",
            "--concurrency",
            "1",
            "--disable-hot-restart",
            "--log-level",
            "error",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}; {mount}",
        String::from_utf8_lossy(&result.stderr)
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .no_proxy()
        .build()
        .unwrap();
    let admin = format!("http://127.0.0.1:{admin_port}");
    let url = format!("http://127.0.0.1:{proxy_port}/");
    let mut ready = false;
    for _ in 0..100 {
        if client
            .get(format!("{admin}/ready"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !ready {
        let logs = Command::new("docker")
            .args(["logs", &name])
            .output()
            .unwrap();
        panic!("Envoy not ready: {}", String::from_utf8_lossy(&logs.stderr));
    }
    let envoy = Arc::new(EnvoyAdmin::new(EnvoyAdminEndpoint::new(&admin).unwrap()).unwrap());
    let repository = Arc::new(SqliteDecisionRepository::open(&workspace.path("audit.db")).unwrap());
    let controller = BackpressureController::new(
        BackendName::new("orders").unwrap(),
        BackpressurePolicy {
            minimum: ConcurrencyLimit::new(1).unwrap(),
            maximum: ConcurrencyLimit::new(8).unwrap(),
            latency_budget: Latency::milliseconds(200.0).unwrap(),
            error_budget: Ratio::new(0.1).unwrap(),
            minimum_samples: RequestCount::new(2),
            healthy_windows: WindowCount::new(3).unwrap(),
            cooldown: Duration::from_millis(100),
        },
    )
    .unwrap();
    let mut control = ControlBackpressure::new(
        controller,
        envoy.clone(),
        envoy.clone(),
        repository.clone(),
        false,
    );
    control.tick().await.unwrap();
    assert!(client.get(&url).send().await.unwrap().status().is_success());
    let mut baseline = (0, 0);
    for _ in 0..3 {
        let result = wave(&client, &url).await;
        baseline.0 += result.0;
        baseline.1 += result.1;
    }
    let baseline_failures = state.failures.load(Ordering::SeqCst);
    let baseline_peak = state.peak.swap(0, Ordering::SeqCst);
    assert!(baseline_failures > 0);
    let first = control.tick().await.unwrap();
    assert_eq!(first.decision.reason, DecisionReason::Reduce, "{first:?}");
    assert_eq!(first.decision.after.value(), 4);
    tokio::time::sleep(Duration::from_millis(110)).await;
    wave(&client, &url).await;
    assert_eq!(control.tick().await.unwrap().decision.after.value(), 2);
    tokio::time::sleep(Duration::from_millis(110)).await;
    state.peak.store(0, Ordering::SeqCst);
    let failures_before = state.failures.load(Ordering::SeqCst);
    let protected = wave(&client, &url).await;
    let protected_peak = state.peak.load(Ordering::SeqCst);
    let protected_failures = state.failures.load(Ordering::SeqCst) - failures_before;
    assert_eq!(protected_failures, 0);
    assert!(protected_peak <= 2, "peak {protected_peak}");
    assert!(protected.1 > 0);
    assert!(protected.0 > 0);
    assert_eq!(
        control.tick().await.unwrap().decision.reason,
        DecisionReason::Hold
    );
    for _ in 0..2 {
        // Low-rate healthy traffic proves recovery without another overload burst.
        for _ in 0..3 {
            assert!(client.get(&url).send().await.unwrap().status().is_success());
        }
        control.tick().await.unwrap();
    }
    assert_eq!(
        envoy
            .current(&BackendName::new("orders").unwrap())
            .await
            .unwrap()
            .value(),
        3
    );
    let history = repository
        .recent(QueryLimit::new(20).unwrap())
        .await
        .unwrap();
    std::fs::create_dir_all("artifacts").unwrap();
    std::fs::write("artifacts/backpressure-smoke.json",serde_json::to_vec_pretty(&json!({
        "envoy":"v1.39.3","baseline":{"successes":baseline.0,"backend_failures":baseline_failures,"peak":baseline_peak},
        "protected":{"successes":protected.0,"rejected":protected.1,"backend_failures":protected_failures,"peak":protected_peak},
        "recovered_limit":3,"history":history,
    })).unwrap()).unwrap();
    println!(
        "baseline peak={baseline_peak} failures={baseline_failures}; protected peak={protected_peak} failures={protected_failures} rejected={}; recovered=3",
        protected.1
    );
}
