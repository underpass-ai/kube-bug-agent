mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{FaultyRepository, observation};
use http_body_util::BodyExt;
use kube_bug_agent::{
    adapters::{
        config::AccessToken,
        http::{CollectorServer, CollectorState},
        sqlite::SqliteIncidentRepository,
    },
    application::IncidentQuery,
    domain::*,
    ports::IncidentRepository,
};
use std::{collections::HashMap, sync::Arc};
use tower::ServiceExt;

fn request(path: &str, token: Option<&str>, payload: Option<String>) -> Request<Body> {
    let mut builder = Request::builder().uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if payload.is_some() {
        builder = builder
            .method("POST")
            .header("content-type", "application/json");
    }
    builder
        .body(payload.map(Body::from).unwrap_or_else(Body::empty))
        .unwrap()
}

#[tokio::test]
async fn collector_authenticates_persists_and_rejects_conflicts_and_bad_input() {
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    let router = CollectorServer::router(CollectorState {
        repository: repository.clone(),
        token: Some(AccessToken::new("fixture-token".into()).unwrap()),
        topology: None,
    });
    assert_eq!(
        router
            .clone()
            .oneshot(request("/healthz", None, None))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request("/readyz", None, None))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request("/v1/incidents", None, None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request("/v1/incidents", Some("wrong"), None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let first = observation("event-1");
    let payload = serde_json::to_string(&first).unwrap();
    let response = router
        .clone()
        .oneshot(request(
            "/v1/observations",
            Some("fixture-token"),
            Some(payload.clone()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let receipt: IngestReceipt =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(!receipt.duplicate);
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/v1/observations",
                Some("fixture-token"),
                Some(payload)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let mut conflict = first;
    conflict.severity = Severity::Critical;
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/v1/observations",
                Some("fixture-token"),
                Some(serde_json::to_string(&conflict).unwrap())
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/v1/incidents?namespace=demo&deployment=orders&limit=1",
                Some("fixture-token"),
                None
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/v1/incidents?limit=0",
                Some("fixture-token"),
                None
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/v1/incidents/bad/occurrences",
                Some("fixture-token"),
                None
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                &format!("/v1/incidents/{}/occurrences", receipt.incident_id),
                Some("fixture-token"),
                None
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let invalid = router
        .clone()
        .oneshot(request(
            "/v1/observations",
            Some("fixture-token"),
            Some("{}".into()),
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let large = router
        .oneshot(request(
            "/v1/observations",
            Some("fixture-token"),
            Some("x".repeat(65537)),
        ))
        .await
        .unwrap();
    assert_eq!(large.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        repository.list(IncidentQuery::default()).await.unwrap()[0]
            .occurrences()
            .value(),
        1
    );
}

#[tokio::test]
async fn topology_enriches_identity_and_unknown_pods_retry() {
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    let topology = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
    let router = CollectorServer::router(CollectorState {
        repository: repository.clone(),
        token: None,
        topology: Some(topology.clone()),
    });
    let payload = serde_json::to_string(&observation("event-1")).unwrap();
    assert_eq!(
        router
            .clone()
            .oneshot(request("/v1/observations", None, Some(payload.clone())))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let mut workload = common::workload();
    workload.revision = Revision::new("authoritative").unwrap();
    topology
        .write()
        .await
        .insert(workload.pod_uid.clone(), workload);
    assert_eq!(
        router
            .oneshot(request("/v1/observations", None, Some(payload)))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        repository.list(IncidentQuery::default()).await.unwrap()[0]
            .observation()
            .workload
            .revision
            .as_str(),
        "authoritative"
    );
}

#[tokio::test]
async fn unavailable_repository_returns_safe_errors() {
    let router = CollectorServer::router(CollectorState {
        repository: Arc::new(FaultyRepository),
        token: None,
        topology: None,
    });
    for path in [
        "/v1/incidents",
        &format!("/v1/incidents/{}/occurrences", "0".repeat(64)),
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(request(path, None, None))
                .await
                .unwrap()
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "/v1/observations",
                None,
                Some(serde_json::to_string(&observation("event-1")).unwrap())
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        router
            .oneshot(request("/readyz", None, None))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let state = CollectorState {
        repository: Arc::new(FaultyRepository),
        token: None,
        topology: None,
    };
    assert!(
        CollectorServer::serve("0.0.0.0:0".parse().unwrap(), state)
            .await
            .is_err()
    );
}
