mod common;
use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use common::{TestServer, observation};
use kube_bug_agent::{
    adapters::{
        config::{AccessToken, ApiEndpoint, ModelId},
        openai::OpenAiDiagnosisProvider,
    },
    ports::DiagnosisProvider,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn connector_discovers_once_sends_redacted_evidence_and_validates_output() {
    let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let target = captured.clone();
    let discovery = calls.clone();
    let server=TestServer::new(Router::new().route("/v1/models",get(move || { let calls=discovery.clone(); async move { calls.fetch_add(1,Ordering::SeqCst); Json(json!({"data":[{"id":"fixture-model"}]})) } }))
        .route("/v1/chat/completions",post(move |headers:HeaderMap,Json(body):Json<Value>| { let captured=target.clone(); async move {
            assert_eq!(headers["authorization"],"Bearer fixture-token");
            let observation:Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            let id=observation["event_id"].as_str().unwrap();
            let diagnosis=common::diagnosis(&kube_bug_agent::domain::EventId::new(id).unwrap());
            captured.lock().unwrap().push(body);
            Json(json!({"choices":[{"finish_reason":"stop","message":{"content":serde_json::to_string(&diagnosis).unwrap()}}]}))
        } }))).await;
    let provider = OpenAiDiagnosisProvider::new(
        ApiEndpoint::new(&format!("{}/v1", server.url())).unwrap(),
        None,
        Some(AccessToken::new("fixture-token".into()).unwrap()),
        true,
    )
    .unwrap();
    provider.analyze(&observation("event-1")).await.unwrap();
    provider.analyze(&observation("event-2")).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.model().await.unwrap().as_str(), "fixture-model");
    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert!(!captured[0].to_string().contains("fixture-secret"));
    assert_eq!(
        captured[0]["chat_template_kwargs"]["enable_thinking"],
        false
    );
    assert_eq!(captured[0]["response_format"]["type"], "json_object");
}

#[tokio::test]
async fn connector_rejects_refusals_truncation_bad_json_and_unknown_references() {
    let valid = common::diagnosis(&kube_bug_agent::domain::EventId::new("event-1").unwrap());
    let unknown = common::diagnosis(&kube_bug_agent::domain::EventId::new("invented").unwrap());
    let cases = vec![
        json!({"choices":[]}),
        json!({"choices":[{"finish_reason":"length","message":{"content":"{}"}}]}),
        json!({"choices":[{"finish_reason":"stop","message":{"content":null}}]}),
        json!({"choices":[{"finish_reason":"stop","message":{"content":"not JSON"}}]}),
        json!({"choices":[{"finish_reason":"stop","message":{"content":"{}"}}]}),
        json!({"choices":[{"finish_reason":"stop","message":{"content":serde_json::to_string(&unknown).unwrap()}}]}),
    ];
    for response in cases {
        let server = TestServer::new(Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let response = response.clone();
                async move { Json(response) }
            }),
        ))
        .await;
        let provider = OpenAiDiagnosisProvider::new(
            ApiEndpoint::new(&format!("{}/v1", server.url())).unwrap(),
            Some(ModelId::new("fixture-model").unwrap()),
            None,
            false,
        )
        .unwrap();
        assert!(provider.analyze(&observation("event-1")).await.is_err());
    }
    let body = json!({"choices":[{"finish_reason":"stop","message":{"content":serde_json::to_string(&valid).unwrap()}}]});
    let server = TestServer::new(Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let body = body.clone();
            async move { Json(body) }
        }),
    ))
    .await;
    let provider = OpenAiDiagnosisProvider::new(
        ApiEndpoint::new(&format!("{}/v1", server.url())).unwrap(),
        Some(ModelId::new("fixture-model").unwrap()),
        None,
        false,
    )
    .unwrap();
    assert!(provider.analyze(&observation("event-1")).await.is_ok());
}

#[tokio::test]
async fn provider_errors_never_expose_response_bodies() {
    let server = TestServer::new(
        Router::new()
            .route("/v1/models", get(|| async { Json(json!({"data":[]})) }))
            .route(
                "/v1/chat/completions",
                post(|| async {
                    (StatusCode::SERVICE_UNAVAILABLE, "provider-secret").into_response()
                }),
            ),
    )
    .await;
    let endpoint = ApiEndpoint::new(&format!("{}/v1", server.url())).unwrap();
    let discovery = OpenAiDiagnosisProvider::new(endpoint.clone(), None, None, false).unwrap();
    assert!(discovery.model().await.is_err());
    let provider = OpenAiDiagnosisProvider::new(
        endpoint,
        Some(ModelId::new("fixture-model").unwrap()),
        None,
        false,
    )
    .unwrap();
    let error = provider.analyze(&observation("event-1")).await.unwrap_err();
    assert!(!error.to_string().contains("provider-secret"));
    for content in ["not JSON".to_owned(), "x".repeat(262145)] {
        let server = TestServer::new(Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let content = content.clone();
                async move { content }
            }),
        ))
        .await;
        let provider = OpenAiDiagnosisProvider::new(
            ApiEndpoint::new(&format!("{}/v1", server.url())).unwrap(),
            Some(ModelId::new("fixture-model").unwrap()),
            None,
            false,
        )
        .unwrap();
        assert!(provider.analyze(&observation("event-1")).await.is_err());
    }
}

#[test]
fn endpoints_and_tokens_are_validated() {
    for value in [
        "file:///tmp/model",
        "http://user:secret@localhost/v1",
        "http://localhost/v1?token=secret",
        "http://localhost/v1#fragment",
    ] {
        assert!(ApiEndpoint::new(value).is_err());
    }
    assert!(
        ApiEndpoint::new("http://127.0.0.1:8080/v1")
            .unwrap()
            .is_loopback()
    );
    assert!(
        !ApiEndpoint::new("https://example.com/v1")
            .unwrap()
            .is_loopback()
    );
    assert!(AccessToken::new(" ".into()).is_err());
    assert!(ModelId::new("").is_err());
}
