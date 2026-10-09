mod common;
use common::{TestServer, TestWorkspace};
use kube_bug_agent::{
    adapters::{
        config::ApiEndpoint,
        http::{CollectorServer, CollectorState, HttpObservationSink},
        openai::OpenAiDiagnosisProvider,
        sidecar::{SidecarConfig, SidecarRuntime, SpoolCapacity},
        sqlite::SqliteIncidentRepository,
        wire::IncidentDto,
    },
    application::{AnalyzeIncident, IncidentQuery},
    domain::AnalysisState,
    ports::IncidentRepository,
};
use std::{path::PathBuf, sync::Arc, time::Instant};

#[tokio::test]
#[ignore = "requires the installed local model; run make smoke-local"]
async fn installed_model_diagnoses_a_real_sidecar_observation() {
    let endpoint =
        std::env::var("LOCAL_LLM_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080/v1".into());
    let endpoint_value = ApiEndpoint::new(&endpoint).unwrap();
    assert!(
        endpoint_value.is_loopback(),
        "smoke test must use a local model"
    );
    let workspace = TestWorkspace::new();
    let repository =
        Arc::new(SqliteIncidentRepository::open(&workspace.path("incidents.db")).unwrap());
    let server = TestServer::new(CollectorServer::router(CollectorState {
        repository: repository.clone(),
        token: None,
        topology: None,
    }))
    .await;
    let log = workspace.path("app.log");
    std::fs::write(&log, include_str!("fixtures/application.jsonl")).unwrap();
    let sink =
        Arc::new(HttpObservationSink::new(ApiEndpoint::new(server.url()).unwrap(), None).unwrap());
    let cfg = SidecarConfig {
        log,
        spool: workspace.path("spool.db"),
        workload: common::workload(),
        health: None,
        capacity: SpoolCapacity::new(100).unwrap(),
    };
    SidecarRuntime::new(cfg, sink)
        .unwrap()
        .run_once()
        .await
        .unwrap();
    let provider =
        Arc::new(OpenAiDiagnosisProvider::new(endpoint_value, None, None, true).unwrap());
    let model = provider.model().await.unwrap().as_str().to_owned();
    let start = Instant::now();
    assert!(
        AnalyzeIncident::new(repository.clone(), provider)
            .execute_next()
            .await
            .unwrap()
    );
    let incidents = repository.list(IncidentQuery::default()).await.unwrap();
    assert_eq!(incidents.len(), 1);
    assert_eq!(incidents[0].occurrences().value(), 2);
    assert!(
        matches!(incidents[0].analysis(), AnalysisState::Complete { .. }),
        "local analysis failed: {:?}",
        incidents[0].analysis()
    );
    let artifact = serde_json::json!({"passed":true,"model":model,"endpoint":endpoint,"elapsed_ms":start.elapsed().as_millis(),"incidents":incidents.iter().map(IncidentDto::from).collect::<Vec<_>>()});
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/local-smoke.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&artifact).unwrap()).unwrap();
    println!("Local model smoke passed; evidence: {}", path.display());
}
