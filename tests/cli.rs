mod common;
use axum::{Json, Router, routing::post};
use clap::Parser;
use common::{TestServer, TestWorkspace};
use kube_bug_agent::{
    adapters::{
        http::{CollectorServer, CollectorState},
        sqlite::SqliteIncidentRepository,
    },
    application::IncidentQuery,
    cli::{Cli, Command, LlmOptions},
    ports::IncidentRepository,
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::{process::Stdio, time::Duration};
use tokio::process::{Child, Command as ProcessCommand};

#[tokio::test]
async fn cli_scans_filters_and_analyzes_a_persisted_database() {
    let workspace = TestWorkspace::new();
    let database = workspace.path("incidents.db");
    let database = database.to_str().unwrap();
    let fixture = format!(
        "{}/tests/fixtures/deployment-failures.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let scan = Cli::try_parse_from([
        "agent",
        "scan",
        "--database",
        database,
        "--fixture",
        &fixture,
    ])
    .unwrap();
    assert_eq!(scan.execute().await.unwrap()["observations"], 7);
    let list = Cli::try_parse_from([
        "agent",
        "incidents",
        "--database",
        database,
        "--namespace",
        "demo",
        "--deployment",
        "orders",
    ])
    .unwrap();
    assert_eq!(list.execute().await.unwrap().as_array().unwrap().len(), 7);
    let invalid = Cli::try_parse_from([
        "agent",
        "incidents",
        "--database",
        database,
        "--namespace",
        "bad namespace",
    ])
    .unwrap();
    assert!(invalid.execute().await.is_err());
    let provider=TestServer::new(Router::new().route("/v1/chat/completions",post(|Json(body):Json<Value>| async move {
        let observation:Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        let id=kube_bug_agent::domain::EventId::new(observation["event_id"].as_str().unwrap()).unwrap();
        Json(json!({"choices":[{"finish_reason":"stop","message":{"content":serde_json::to_string(&common::diagnosis(&id)).unwrap()}}]}))
    }))).await;
    let endpoint = format!("{}/v1", provider.url());
    let analyze = Cli::try_parse_from([
        "agent",
        "analyze",
        "--database",
        database,
        "--limit",
        "10",
        "--llm-base-url",
        &endpoint,
        "--llm-model",
        "fixture-model",
        "--disable-thinking",
    ])
    .unwrap();
    let result = analyze.execute().await.unwrap();
    assert!(
        result
            .as_array()
            .unwrap()
            .iter()
            .all(|incident| incident["analysis"]["status"] == "complete")
    );
    let options = LlmOptions {
        llm_base_url: None,
        llm_model: None,
        disable_thinking: false,
    };
    assert!(options.provider(false).unwrap().is_none());
    assert!(options.provider(true).unwrap().is_some());
    assert!(Cli::try_parse_from(["agent", "analyze", "--limit", "0"]).is_err());
    assert!(Cli::try_parse_from(["agent", "collector", "--poll-seconds", "0"]).is_err());
}

#[tokio::test]
async fn cli_sidecar_delivers_and_validates_domain_inputs() {
    let workspace = TestWorkspace::new();
    let log = workspace.path("app.log");
    std::fs::write(&log, include_str!("fixtures/application.jsonl")).unwrap();
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    let server = TestServer::new(CollectorServer::router(CollectorState {
        repository: repository.clone(),
        token: None,
        topology: None,
    }))
    .await;
    let spool = workspace.path("spool.db");
    let args = [
        "agent",
        "sidecar",
        "--logs",
        log.to_str().unwrap(),
        "--spool",
        spool.to_str().unwrap(),
        "--collector-url",
        server.url(),
        "--deployment",
        "orders",
        "--deployment-uid",
        "deployment-1",
        "--revision",
        "2",
        "--pod",
        "orders-one",
        "--pod-uid",
        "pod-1",
        "--namespace",
        "demo",
        "--once",
    ];
    let cli = Cli::try_parse_from(args).unwrap();
    assert_eq!(cli.execute().await.unwrap()["status"], "delivered");
    assert_eq!(
        repository
            .list(IncidentQuery::default())
            .await
            .unwrap()
            .len(),
        1
    );
    let mut bad = Cli::try_parse_from(args).unwrap();
    if let Command::Sidecar(options) = &mut bad.command {
        options.namespace = "bad namespace".into();
    }
    assert!(bad.execute().await.is_err());
    let mut health = Cli::try_parse_from(args).unwrap();
    if let Command::Sidecar(options) = &mut health.command {
        options.health_url = Some("http://127.0.0.1:12345/health".into());
        assert!(options.config().unwrap().health.is_some());
    }
}

#[tokio::test]
async fn collector_cli_fails_closed_without_a_network_token() {
    let workspace = TestWorkspace::new();
    let path = workspace.path("incidents.db");
    let cli = Cli::try_parse_from([
        "agent",
        "collector",
        "--database",
        path.to_str().unwrap(),
        "--listen",
        "0.0.0.0:0",
    ])
    .unwrap();
    assert!(cli.execute().await.is_err());
}

fn spawn_binary(args: &[&str]) -> Child {
    ProcessCommand::new(env!("CARGO_BIN_EXE_kube-bug-agent"))
        .args(args)
        .env_remove("AGENT_TOKEN")
        .env_remove("LLM_BASE_URL")
        .env_remove("LLM_MODEL")
        .env_remove("LLM_API_KEY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

async fn terminate_binary(child: Child) -> Value {
    assert!(
        ProcessCommand::new("kill")
            .args(["-TERM", &child.id().unwrap().to_string()])
            .status()
            .await
            .unwrap()
            .success()
    );
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .expect("binary must shut down within the Kubernetes termination grace period")
        .unwrap();
    assert!(
        output.status.success(),
        "binary failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn real_binary_handles_sigterm_for_collector_and_sidecar() {
    let workspace = TestWorkspace::new();
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let database = workspace.path("collector.db");
    let collector = spawn_binary(&[
        "collector",
        "--database",
        database.to_str().unwrap(),
        "--listen",
        &address.to_string(),
    ]);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(250))
        .build()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if client
                .get(format!("http://{address}/readyz"))
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("collector must become ready");
    assert_eq!(terminate_binary(collector).await["status"], "stopped");

    let log = workspace.path("app.log");
    std::fs::write(&log, include_str!("fixtures/application.jsonl")).unwrap();
    let spool = workspace.path("sidecar.db");
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    let server = TestServer::new(CollectorServer::router(CollectorState {
        repository: repository.clone(),
        token: None,
        topology: None,
    }))
    .await;
    let sidecar = spawn_binary(&[
        "sidecar",
        "--logs",
        log.to_str().unwrap(),
        "--spool",
        spool.to_str().unwrap(),
        "--collector-url",
        server.url(),
        "--namespace",
        "demo",
        "--deployment",
        "orders",
        "--pod",
        "orders-one",
        "--pod-uid",
        "pod-1",
    ]);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let incidents = repository.list(IncidentQuery::default()).await.unwrap();
            if incidents
                .first()
                .is_some_and(|incident| incident.occurrences().value() == 2)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("sidecar must deliver its observations");
    assert_eq!(terminate_binary(sidecar).await["status"], "delivered");
}
