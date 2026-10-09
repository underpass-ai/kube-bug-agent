mod common;
use chrono::{Duration, Utc};
use common::{FixedProvider, RejectingProvider, TestWorkspace, diagnosis, observation};
use kube_bug_agent::{
    adapters::sqlite::SqliteIncidentRepository,
    application::{AnalyzeIncident, IncidentQuery, IngestIncident, QueryLimit},
    domain::*,
    ports::IncidentRepository,
};
use std::sync::Arc;

#[tokio::test]
async fn transactions_deduplicate_group_and_preserve_occurrences() {
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    let ingest = IngestIncident::new(repository.clone());
    let first = observation("event-1");
    let receipt = ingest.execute(first.clone()).await.unwrap();
    assert!(!receipt.duplicate);
    assert!(ingest.execute(first.clone()).await.unwrap().duplicate);
    let mut conflict = first.clone();
    conflict.severity = Severity::Critical;
    assert!(matches!(
        ingest
            .execute(conflict)
            .await
            .unwrap_err()
            .downcast_ref::<DomainError>(),
        Some(DomainError::EventConflict)
    ));
    let mut second = observation("event-2");
    second.observed_at += Duration::seconds(5);
    assert_eq!(
        ingest.execute(second).await.unwrap().incident_id,
        receipt.incident_id
    );
    let mut different = observation("event-3");
    different.workload.revision = Revision::new("3").unwrap();
    ingest.execute(different).await.unwrap();
    let items = repository.list(IncidentQuery::default()).await.unwrap();
    assert_eq!(items.len(), 2);
    let grouped = items
        .iter()
        .find(|item| item.id() == &receipt.incident_id)
        .unwrap();
    assert_eq!(grouped.occurrences().value(), 2);
    assert_eq!(
        repository
            .occurrences(receipt.incident_id)
            .await
            .unwrap()
            .len(),
        2
    );
    let filtered = IncidentQuery {
        namespace: Some(Namespace::new("other").unwrap()),
        ..Default::default()
    };
    assert!(repository.list(filtered).await.unwrap().is_empty());
    let filtered = IncidentQuery {
        namespace: Some(Namespace::new("demo").unwrap()),
        deployment: Some(DeploymentName::new("orders").unwrap()),
        limit: QueryLimit::new(1).unwrap(),
    };
    assert_eq!(repository.list(filtered).await.unwrap().len(), 1);
    repository.health().await.unwrap();
}

#[tokio::test]
async fn sqlite_survives_restarts_and_recovers_interrupted_analysis() {
    let workspace = TestWorkspace::new();
    let path = workspace.path("database/incidents.db");
    let repository = SqliteIncidentRepository::open(&path).unwrap();
    repository.record(observation("event-1")).await.unwrap();
    let incident = repository
        .claim_pending(Utc::now() + Duration::seconds(1))
        .await
        .unwrap()
        .unwrap();
    assert!(
        repository
            .claim_pending(Utc::now())
            .await
            .unwrap()
            .is_none()
    );
    drop(repository);
    let repository = SqliteIncidentRepository::open(&path).unwrap();
    let recovered = repository
        .claim_pending(Utc::now() + Duration::seconds(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.id(), incident.id());
    assert_eq!(recovered.analysis().attempts().value(), 2);
    repository
        .finish_analysis(
            recovered.id().clone(),
            Ok(diagnosis(&recovered.observation().event_id)),
            Utc::now(),
        )
        .await
        .unwrap();
    drop(repository);
    let repository = SqliteIncidentRepository::open(&path).unwrap();
    assert!(matches!(
        repository.list(IncidentQuery::default()).await.unwrap()[0].analysis(),
        AnalysisState::Complete { .. }
    ));
    assert!(
        repository
            .finish_analysis(
                IncidentId::new("0".repeat(64)).unwrap(),
                Err(AnalysisFailure::new("missing").unwrap()),
                Utc::now()
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn analysis_use_case_persists_success_and_provider_failure() {
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    repository.record(observation("event-1")).await.unwrap();
    let analyze = AnalyzeIncident::new(repository.clone(), Arc::new(FixedProvider));
    assert!(analyze.execute_next().await.unwrap());
    assert!(!analyze.execute_next().await.unwrap());
    assert!(matches!(
        repository.list(IncidentQuery::default()).await.unwrap()[0].analysis(),
        AnalysisState::Complete { .. }
    ));
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    repository.record(observation("event-1")).await.unwrap();
    let analyze = AnalyzeIncident::new(repository.clone(), Arc::new(RejectingProvider));
    assert!(analyze.execute_next().await.unwrap());
    let items = repository.list(IncidentQuery::default()).await.unwrap();
    assert!(matches!(
        items[0].analysis(),
        AnalysisState::Pending {
            last_error: Some(_),
            ..
        }
    ));
    assert!(!analyze.execute_next().await.unwrap());
    for attempt in 1..=2 {
        let now = Utc::now() + Duration::seconds(100 * attempt);
        let pending = repository.claim_pending(now).await.unwrap().unwrap();
        repository
            .finish_analysis(
                pending.id().clone(),
                Err(AnalysisFailure::new("still unavailable").unwrap()),
                now,
            )
            .await
            .unwrap();
    }
    assert!(matches!(
        repository.list(IncidentQuery::default()).await.unwrap()[0].analysis(),
        AnalysisState::Failed { .. }
    ));
}
