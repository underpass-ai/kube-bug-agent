mod common;
use axum::{
    Router,
    http::StatusCode,
    routing::{get, post},
};
use common::{TestServer, TestWorkspace, observation};
use kube_bug_agent::{
    adapters::{
        config::{ApiEndpoint, LocalHealthEndpoint},
        http::{CollectorServer, CollectorState, HttpObservationSink},
        sidecar::{FileLogReader, SidecarConfig, SidecarRuntime, SpoolCapacity, SqliteOutbox},
        sqlite::SqliteIncidentRepository,
    },
    application::{FlushOutbox, IncidentQuery},
    domain::DetectorKind,
    ports::{IncidentRepository, ObservationOutbox},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn config(workspace: &TestWorkspace, capacity: u32) -> SidecarConfig {
    SidecarConfig {
        log: workspace.path("app.log"),
        spool: workspace.path("spool/outbox.db"),
        workload: common::workload(),
        health: None,
        capacity: SpoolCapacity::new(capacity).unwrap(),
    }
}

#[tokio::test]
async fn sidecar_delivers_real_http_events_and_keeps_cursor_across_restarts() {
    let workspace = TestWorkspace::new();
    std::fs::write(
        workspace.path("app.log"),
        include_str!("fixtures/application.jsonl"),
    )
    .unwrap();
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    let server = TestServer::new(CollectorServer::router(CollectorState {
        repository: repository.clone(),
        token: None,
        topology: None,
    }))
    .await;
    let sink =
        Arc::new(HttpObservationSink::new(ApiEndpoint::new(server.url()).unwrap(), None).unwrap());
    let runtime = SidecarRuntime::new(config(&workspace, 100), sink.clone()).unwrap();
    runtime.run_once().await.unwrap();
    drop(runtime);
    let runtime = SidecarRuntime::new(config(&workspace, 100), sink).unwrap();
    runtime.run_once().await.unwrap();
    let incidents = repository.list(IncidentQuery::default()).await.unwrap();
    assert_eq!(incidents.len(), 1);
    assert_eq!(incidents[0].occurrences().value(), 2);
    assert!(
        !serde_json::to_string(&incidents)
            .unwrap()
            .contains("fixture-secret")
    );
}

#[tokio::test]
async fn outbox_backpressure_partial_lines_and_rotation_preserve_identity() {
    let workspace = TestWorkspace::new();
    let cfg = config(&workspace, 1);
    let outbox = SqliteOutbox::open(&cfg.spool, cfg.capacity).unwrap();
    assert_eq!(FileLogReader::read(&cfg, &outbox, false).await.unwrap(), 0);
    assert!(FileLogReader::read(&cfg, &outbox, true).await.is_err());
    let line = r#"{"level":"error","message":"timeout","time":"2026-10-01T00:00:00Z"}"#;
    std::fs::write(&cfg.log, line).unwrap();
    assert_eq!(FileLogReader::read(&cfg, &outbox, true).await.unwrap(), 0);
    std::fs::write(&cfg.log, format!("{line}\n{line}\n")).unwrap();
    assert_eq!(FileLogReader::read(&cfg, &outbox, true).await.unwrap(), 1);
    let first = outbox.next().await.unwrap().unwrap();
    assert!(!outbox.enqueue(observation("extra")).unwrap());
    outbox.acknowledge(first.event_id.clone()).await.unwrap();
    assert_eq!(FileLogReader::read(&cfg, &outbox, true).await.unwrap(), 1);
    let second = outbox.next().await.unwrap().unwrap();
    assert_ne!(first.event_id, second.event_id);
    outbox.acknowledge(second.event_id).await.unwrap();
    std::fs::write(&cfg.log, format!("{line}\n")).unwrap();
    FileLogReader::read(&cfg, &outbox, true).await.unwrap();
    let truncated = outbox.next().await.unwrap().unwrap();
    assert_ne!(first.event_id, truncated.event_id);
    outbox.acknowledge(truncated.event_id).await.unwrap();
    std::fs::rename(&cfg.log, workspace.path("old.log")).unwrap();
    std::fs::write(&cfg.log, format!("{line}\n")).unwrap();
    FileLogReader::read(&cfg, &outbox, true).await.unwrap();
    let rotated = outbox.next().await.unwrap().unwrap();
    assert_ne!(first.event_id, rotated.event_id);
    outbox.acknowledge(rotated.event_id).await.unwrap();
    std::fs::write(&cfg.log, format!("{}\n{line}\n", "X".repeat(70000))).unwrap();
    // Truncation to zero is observable before rewriting an oversized line.
    let fresh = config(&workspace, 1);
    let fresh_outbox = SqliteOutbox::open(&workspace.path("fresh.db"), fresh.capacity).unwrap();
    FileLogReader::read(&fresh, &fresh_outbox, true)
        .await
        .unwrap();
    assert!(fresh_outbox.next().await.unwrap().is_some());
    assert!(SpoolCapacity::new(0).is_err());
    assert!(SpoolCapacity::new(10001).is_err());
}

#[tokio::test]
async fn failed_delivery_keeps_durable_outbox_until_acknowledgement() {
    let workspace = TestWorkspace::new();
    std::fs::write(
        workspace.path("app.log"),
        "{\"level\":\"error\",\"message\":\"timeout\"}\n",
    )
    .unwrap();
    let failing = TestServer::new(Router::new().route(
        "/v1/observations",
        post(|| async { StatusCode::SERVICE_UNAVAILABLE }),
    ))
    .await;
    let sink =
        Arc::new(HttpObservationSink::new(ApiEndpoint::new(failing.url()).unwrap(), None).unwrap());
    let runtime = SidecarRuntime::new(config(&workspace, 10), sink).unwrap();
    assert!(runtime.run_once().await.is_err());
    drop(runtime);
    let outbox = Arc::new(
        SqliteOutbox::open(
            &workspace.path("spool/outbox.db"),
            SpoolCapacity::new(10).unwrap(),
        )
        .unwrap(),
    );
    assert!(outbox.next().await.unwrap().is_some());
    let invalid = TestServer::new(Router::new().route(
        "/v1/observations",
        post(|| async { "not an acknowledgement" }),
    ))
    .await;
    let sink =
        Arc::new(HttpObservationSink::new(ApiEndpoint::new(invalid.url()).unwrap(), None).unwrap());
    assert!(
        FlushOutbox::new(outbox.clone(), sink)
            .execute()
            .await
            .is_err()
    );
    assert!(outbox.next().await.unwrap().is_some());
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    let server = TestServer::new(CollectorServer::router(CollectorState {
        repository: repository.clone(),
        token: None,
        topology: None,
    }))
    .await;
    let sink =
        Arc::new(HttpObservationSink::new(ApiEndpoint::new(server.url()).unwrap(), None).unwrap());
    let flush = FlushOutbox::new(outbox.clone(), sink);
    assert_eq!(flush.execute().await.unwrap(), 1);
    assert_eq!(flush.execute().await.unwrap(), 0);
    assert!(outbox.next().await.unwrap().is_none());
    assert_eq!(
        repository
            .list(IncidentQuery::default())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn probes_report_once_per_failure_episode_and_recover() {
    let workspace = TestWorkspace::new();
    std::fs::write(workspace.path("app.log"), "").unwrap();
    let healthy = Arc::new(AtomicBool::new(false));
    let target = healthy.clone();
    let health = TestServer::new(Router::new().route(
        "/health",
        get(move || {
            let target = target.clone();
            async move {
                if target.load(Ordering::SeqCst) {
                    StatusCode::OK
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            }
        }),
    ))
    .await;
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    let server = TestServer::new(CollectorServer::router(CollectorState {
        repository: repository.clone(),
        token: None,
        topology: None,
    }))
    .await;
    let sink =
        Arc::new(HttpObservationSink::new(ApiEndpoint::new(server.url()).unwrap(), None).unwrap());
    let mut cfg = config(&workspace, 10);
    cfg.health = Some(LocalHealthEndpoint::new(&format!("{}/health", health.url())).unwrap());
    let mut runtime = SidecarRuntime::new(cfg, sink).unwrap();
    runtime.tick(false).await.unwrap();
    for _ in 0..4 {
        runtime.tick(true).await.unwrap();
    }
    let items = repository.list(IncidentQuery::default()).await.unwrap();
    assert_eq!(
        items[0].observation().detector,
        DetectorKind::LocalProbeFailed
    );
    assert_eq!(items[0].occurrences().value(), 1);
    healthy.store(true, Ordering::SeqCst);
    runtime.tick(true).await.unwrap();
    healthy.store(false, Ordering::SeqCst);
    for _ in 0..3 {
        runtime.tick(true).await.unwrap();
    }
    assert_eq!(
        repository.list(IncidentQuery::default()).await.unwrap()[0]
            .occurrences()
            .value(),
        2
    );
    assert!(LocalHealthEndpoint::new("https://example.com/health").is_err());
    assert!(LocalHealthEndpoint::new("http://user:secret@localhost/health").is_err());
}
