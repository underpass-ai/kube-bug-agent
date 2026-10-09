mod common;
use axum::{Json, Router, extract::Request, routing::get};
use kube_bug_agent::{
    adapters::{
        kubernetes::{FixtureSource, KubernetesSnapshot, KubernetesSource},
        sqlite::SqliteIncidentRepository,
    },
    application::{CollectIncidents, IncidentQuery},
    domain::{DetectorKind, Namespace},
    ports::{IncidentRepository, ObservationSource},
};
use serde_json::json;
use std::sync::Arc;

#[test]
fn snapshot_correlates_uids_and_detects_infrastructure_failures() {
    let snapshot: KubernetesSnapshot = serde_json::from_value(common::fixture()).unwrap();
    let now = "2026-10-09T00:00:00Z".parse().unwrap();
    let observations = snapshot.detect(now, 60);
    for kind in [
        DetectorKind::ContainerOom,
        DetectorKind::CrashLoop,
        DetectorKind::ImagePullFailed,
        DetectorKind::PodUnschedulable,
        DetectorKind::PodNotReady,
        DetectorKind::RolloutStalled,
        DetectorKind::KubernetesWarning,
    ] {
        assert!(
            observations
                .iter()
                .any(|observation| observation.detector == kind),
            "missing {kind:?}"
        );
    }
    assert_eq!(snapshot.topology().len(), 3);
    assert_eq!(
        snapshot
            .workload(&snapshot.pods[0])
            .unwrap()
            .revision
            .as_str(),
        "2"
    );
    assert_eq!(
        observations,
        snapshot.detect(now + chrono::Duration::seconds(30), 60)
    );
    let mut missing: KubernetesSnapshot = serde_json::from_value(common::fixture()).unwrap();
    missing.replica_sets.clear();
    assert!(missing.workload(&missing.pods[0]).is_none());
    let mut fallback = common::fixture();
    fallback["replica_sets"][0]["metadata"]["annotations"] = json!({});
    let fallback: KubernetesSnapshot = serde_json::from_value(fallback).unwrap();
    assert_eq!(
        fallback
            .workload(&fallback.pods[0])
            .unwrap()
            .revision
            .as_str(),
        "replicaset-1"
    );
}

#[test]
fn detector_ignores_stale_paused_deleted_and_healthy_resources() {
    let now = "2026-10-09T00:00:00Z".parse().unwrap();
    for modification in 0..6 {
        let mut fixture = common::fixture();
        fixture["pods"] = json!([]);
        fixture["events"] = json!([]);
        match modification {
            0 => fixture["deployments"][0]["spec"]["paused"] = json!(true),
            1 => fixture["deployments"][0]["spec"]["replicas"] = json!(0),
            2 => fixture["deployments"][0]["status"]["observedGeneration"] = json!(1),
            3 => fixture["deployments"][0]["status"]["conditions"][0]["status"] = json!("True"),
            4 => {
                fixture["deployments"][0]["metadata"]["deletionTimestamp"] =
                    json!("2026-10-08T00:00:00Z")
            }
            _ => fixture["deployments"][0]["status"] = json!({}),
        }
        let snapshot: KubernetesSnapshot = serde_json::from_value(fixture).unwrap();
        assert!(snapshot.detect(now, 60).is_empty());
    }
    let mut fixture = common::fixture();
    fixture["deployments"][0]["status"] = json!({});
    fixture["events"] = json!([]);
    for pod in fixture["pods"].as_array_mut().unwrap() {
        pod["metadata"]["deletionTimestamp"] = json!("2026-10-08T00:00:00Z");
    }
    let snapshot: KubernetesSnapshot = serde_json::from_value(fixture).unwrap();
    assert!(snapshot.detect(now, 60).is_empty());
}

#[test]
fn warning_events_correlate_pods_deployments_and_unknown_owners() {
    let now = "2026-10-09T00:00:00Z".parse().unwrap();
    for (kind, uid, expected) in [
        ("Pod", "pod-1", true),
        ("Deployment", "deployment-1", true),
        ("ReplicaSet", "replicaset-1", true),
        ("Node", "unknown", false),
        ("Pod", "missing", false),
    ] {
        let mut fixture = common::fixture();
        fixture["events"][0]["involvedObject"]["kind"] = json!(kind);
        fixture["events"][0]["involvedObject"]["uid"] = json!(uid);
        fixture["events"][0]["lastTimestamp"] = json!(null);
        fixture["events"][0]["eventTime"] = json!("2026-10-01T00:05:00.000001Z");
        let snapshot: KubernetesSnapshot = serde_json::from_value(fixture).unwrap();
        assert_eq!(
            snapshot
                .detect(now, 60)
                .iter()
                .any(|observation| observation.detector == DetectorKind::KubernetesWarning),
            expected
        );
    }
}

#[tokio::test]
async fn kubernetes_api_adapter_uses_scoped_lists_and_fixture_source_persists() {
    let fixture = common::fixture();
    let source = fixture.clone();
    let server=common::TestServer::new(Router::new().fallback(get(move |request:Request| { let fixture=source.clone(); async move {
        let path=request.uri().path(); let (kind,items)=if path.ends_with("/pods") { ("PodList",fixture["pods"].clone()) }
            else if path.ends_with("/replicasets") { ("ReplicaSetList",fixture["replica_sets"].clone()) }
            else if path.ends_with("/deployments") { ("DeploymentList",fixture["deployments"].clone()) }
            else { ("EventList",fixture["events"].clone()) };
        assert!(path.contains("/namespaces/demo/"));
        Json(json!({"kind":kind,"apiVersion":if kind=="DeploymentList"||kind=="ReplicaSetList" {"apps/v1"} else {"v1"},"metadata":{"resourceVersion":"1"},"items":items}))
    } }))).await;
    let client = kube::Client::try_from(kube::Config::new(server.url().parse().unwrap())).unwrap();
    let source = KubernetesSource::new(
        client,
        Namespace::new("demo").unwrap(),
        "bug-agent.io/enabled=true".into(),
    );
    let snapshot = source.collect().await.unwrap();
    assert_eq!(snapshot.workloads.len(), 3);
    assert_eq!(snapshot.observations.len(), 7);
    let workspace = common::TestWorkspace::new();
    let path = workspace.path("snapshot.json");
    std::fs::write(&path, fixture.to_string()).unwrap();
    let repository = Arc::new(SqliteIncidentRepository::in_memory().unwrap());
    let source = Arc::new(FixtureSource::new(path));
    let collect = CollectIncidents::new(repository.clone(), source);
    collect.execute().await.unwrap();
    collect.execute().await.unwrap();
    let incidents = repository.list(IncidentQuery::default()).await.unwrap();
    assert_eq!(incidents.len(), 7);
    assert!(
        incidents
            .iter()
            .all(|incident| incident.occurrences().value() == 1)
    );
}
