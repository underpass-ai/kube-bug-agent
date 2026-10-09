#![allow(dead_code, unused_imports)]
mod test_workspace;
pub use test_workspace::TestWorkspace;
mod test_server;
pub use test_server::TestServer;
mod rejecting_provider;
pub use rejecting_provider::RejectingProvider;
mod fixed_provider;
pub use fixed_provider::FixedProvider;
mod faulty_repository;
pub use faulty_repository::FaultyRepository;

use kube_bug_agent::domain::*;
use serde_json::json;

pub fn workload() -> Workload {
    Workload {
        namespace: Namespace::new("demo").unwrap(),
        deployment: DeploymentName::new("orders").unwrap(),
        deployment_uid: Some(DeploymentUid::new("deployment-1").unwrap()),
        revision: Revision::new("2").unwrap(),
        pod: PodName::new("orders-one").unwrap(),
        pod_uid: PodUid::new("pod-1").unwrap(),
        container: ContainerName::new("app").unwrap(),
    }
}
pub fn observation(id: &str) -> Observation {
    Observation {
        event_id: EventId::new(id).unwrap(),
        observed_at: "2026-10-01T00:05:00Z".parse().unwrap(),
        workload: workload(),
        detector: DetectorKind::ApplicationError,
        severity: Severity::Error,
        signature: ErrorSignature::new("connection timeout for request 123").unwrap(),
        evidence: Evidence::new(
            json!({"message":"connection timeout","password":"fixture-secret"}),
        )
        .unwrap(),
    }
}
pub fn diagnosis(id: &EventId) -> Diagnosis {
    Diagnosis {
        summary: DiagnosisSummary::new("Connection timeout observed").unwrap(),
        suspected_cause: SuspectedCause::new("Upstream service may be unavailable").unwrap(),
        confidence: Confidence::Low,
        recommended_checks: RecommendedChecks::new(vec![
            InvestigationStep::new("Check upstream availability").unwrap(),
        ])
        .unwrap(),
        evidence_refs: EvidenceReferences::new(vec![id.clone()]).unwrap(),
    }
}
pub fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../fixtures/deployment-failures.json")).unwrap()
}
