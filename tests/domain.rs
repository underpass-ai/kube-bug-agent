mod common;
use chrono::{Duration, Utc};
use common::{diagnosis, observation};
use kube_bug_agent::{
    adapters::sidecar::JsonLogDecoder,
    application::{LogDetector, QueryLimit},
    domain::*,
};
use serde_json::json;

#[test]
fn identifiers_and_payloads_cannot_bypass_invariants() {
    assert!(Namespace::new("").is_err());
    assert!(Namespace::new("UPPER").is_err());
    assert!(Namespace::new("a".repeat(64)).is_err());
    assert!(DeploymentName::new("a..b").is_err());
    assert!(PodUid::new("bad uid").is_err());
    assert!(IncidentId::new("bad").is_err());
    assert!(DiagnosisSummary::new(" ").is_err());
    assert!(InvestigationStep::new("a".repeat(2049)).is_err());
    assert!(OccurrenceCount::try_from(0).is_err());
    assert!(AnalysisAttempts::try_from(4).is_err());
    assert!(QueryLimit::new(0).is_err());
    assert!(QueryLimit::new(501).is_err());
    assert!(HttpStatus::new(99).is_err());
    assert!(HttpStatus::new(600).is_err());
    assert_eq!(QueryLimit::default().value(), 100);
    assert!(serde_json::from_value::<Namespace>(json!("wrong namespace")).is_err());
    assert!(Evidence::new(json!([])).is_err());
    assert!(Evidence::new(json!({"payload":"x".repeat(32769)})).is_err());
    assert!(ErrorSignature::new("").is_err());
    assert!(ErrorSignature::new("x".repeat(1025)).is_err());
    assert!(RecommendedChecks::new(vec![]).is_err());
    assert!(EvidenceReferences::new(vec![]).is_err());
    assert_eq!(Namespace::new("demo").unwrap().to_string(), "demo");
    let plain: String = Namespace::new("demo").unwrap().into();
    assert_eq!(plain, "demo");
    assert!(DeploymentName::new("one.two").is_ok());
}

#[test]
fn evidence_is_redacted_and_signatures_group_dynamic_values() {
    let evidence=Evidence::new(json!({"nested":{"api_key":"secret","text":"password=hunter2 Authorization: Bearer secret-token"},"items":["sk-proj-1234567890",42]})).unwrap();
    let text = serde_json::to_string(&evidence).unwrap();
    assert!(!text.contains("hunter2"));
    assert!(!text.contains("secret-token"));
    assert!(!text.contains("sk-proj-"));
    let value: serde_json::Value = evidence.clone().into();
    assert_eq!(value["nested"]["api_key"], "[REDACTED]");
    assert_eq!(
        ErrorSignature::new("timeout request 123").unwrap(),
        ErrorSignature::new("timeout request 456").unwrap()
    );
    assert_eq!(
        ErrorSignature::new("addr 0x123abc").unwrap().as_str(),
        "addr #"
    );
    assert!(serde_json::from_value::<Observation>(json!({"event_id":""})).is_err());
}

#[test]
fn incident_fingerprints_track_release_and_workload_identity() {
    let first = observation("event-1");
    let mut second = observation("event-2");
    second.workload.pod_uid = PodUid::new("another-pod").unwrap();
    assert_eq!(first.fingerprint(), second.fingerprint());
    second.workload.revision = Revision::new("3").unwrap();
    assert_ne!(first.fingerprint(), second.fingerprint());
    second = first.clone();
    second.workload.deployment_uid = None;
    let mut third = second.clone();
    third.workload.pod_uid = PodUid::new("other").unwrap();
    assert_ne!(second.fingerprint(), third.fingerprint());
    assert_eq!(
        Observation::source_event_id(b"stable"),
        Observation::source_event_id(b"stable")
    );
}

#[test]
fn incident_aggregate_controls_counts_time_and_analysis_transitions() {
    let now = Utc::now();
    let first = observation("event-1");
    let mut incident = Incident::new(first.clone(), now);
    let mut second = observation("event-2");
    second.observed_at += Duration::seconds(5);
    incident.register(&second).unwrap();
    assert_eq!(incident.occurrences().value(), 2);
    assert_eq!(incident.first_seen(), first.observed_at);
    assert_eq!(incident.last_seen(), second.observed_at);
    second.workload.revision = Revision::new("3").unwrap();
    assert!(incident.register(&second).is_err());
    assert!(
        incident
            .complete_analysis(diagnosis(&first.event_id))
            .is_err()
    );
    assert!(
        incident
            .fail_analysis(AnalysisFailure::new("failed").unwrap(), now)
            .is_err()
    );
    incident.claim_analysis(now).unwrap();
    assert!(
        incident
            .complete_analysis(diagnosis(&EventId::new("other").unwrap()))
            .is_err()
    );
    incident
        .fail_analysis(AnalysisFailure::new("failed").unwrap(), now)
        .unwrap();
    assert!(!incident.analysis().pending_at(now));
    assert!(incident.claim_analysis(now).is_err());
    let later = now + Duration::seconds(100);
    incident.claim_analysis(later).unwrap();
    incident.recover_analysis(later);
    assert!(incident.analysis().pending_at(later));
    incident.claim_analysis(later).unwrap();
    incident
        .fail_analysis(AnalysisFailure::new("failed").unwrap(), later)
        .unwrap();
    assert!(matches!(incident.analysis(), AnalysisState::Failed { .. }));
    assert_eq!(incident.analysis().attempts().value(), 3);
    let mut completed = Incident::new(first.clone(), now);
    completed.claim_analysis(now).unwrap();
    completed
        .complete_analysis(diagnosis(&first.event_id))
        .unwrap();
    completed.recover_analysis(now);
    assert!(matches!(
        completed.analysis(),
        AnalysisState::Complete { .. }
    ));
    let mut interrupted = Incident::new(first, now);
    for attempt in 0..3 {
        let time = now + Duration::seconds(100 * attempt);
        interrupted.claim_analysis(time).unwrap();
        if attempt < 2 {
            interrupted
                .fail_analysis(AnalysisFailure::new("failed").unwrap(), time)
                .unwrap();
        }
    }
    interrupted.recover_analysis(now);
    assert!(matches!(
        interrupted.analysis(),
        AnalysisState::Failed { .. }
    ));
}

#[test]
fn log_detection_is_specific_and_rejects_invalid_evidence() {
    let workload = common::workload();
    let id = EventId::new("event-1").unwrap();
    for line in [
        "not JSON",
        r#"{"level":"info","status":200}"#,
        r#"{"level":"error","message":""}"#,
    ] {
        assert!(
            JsonLogDecoder::decode(line)
                .and_then(|log| LogDetector::detect(log, id.clone(), &workload))
                .is_none()
        );
    }
    let panic = LogDetector::detect(
        JsonLogDecoder::decode(
            r#"{"level":"panic","message":"unexpected state","time":"2026-10-01T00:00:00Z"}"#,
        )
        .unwrap(),
        id.clone(),
        &workload,
    )
    .unwrap();
    assert_eq!(panic.severity, Severity::Critical);
    let http = LogDetector::detect(
        JsonLogDecoder::decode(r#"{"status_code":503}"#).unwrap(),
        id.clone(),
        &workload,
    )
    .unwrap();
    assert_eq!(http.detector, DetectorKind::Http5xx);
    let error = LogDetector::detect(
        JsonLogDecoder::decode(r#"{"level":"ERROR","msg":"timeout","timestamp":"bad"}"#).unwrap(),
        id,
        &workload,
    )
    .unwrap();
    assert_eq!(error.detector, DetectorKind::ApplicationError);
    assert_eq!(
        diagnosis(&error.event_id).recommended_checks.items().len(),
        1
    );
    for reason in [
        "CrashLoopBackOff",
        "ImagePullBackOff",
        "ErrImagePull",
        "CreateContainerError",
        "CreateContainerConfigError",
    ] {
        assert!(DetectorKind::waiting(reason).is_some());
    }
    assert!(DetectorKind::waiting("ContainerCreating").is_none());
    for (level, expected) in [
        ("trace", LogLevel::Trace),
        ("debug", LogLevel::Debug),
        ("info", LogLevel::Info),
        ("warn", LogLevel::Warning),
        ("warning", LogLevel::Warning),
        ("error", LogLevel::Error),
        ("fatal", LogLevel::Fatal),
        ("critical", LogLevel::Critical),
        ("other", LogLevel::Unknown),
    ] {
        let log = JsonLogDecoder::decode(&json!({"level":level,"status":200}).to_string()).unwrap();
        assert_eq!(log.level, expected);
        let event = LogDetector::detect(log, EventId::new("level-check").unwrap(), &workload);
        assert_eq!(event.is_some(), expected.is_error());
    }
    for invalid_status in [0, 99, 600, 65536, u64::MAX] {
        let log = JsonLogDecoder::decode(&json!({"status":invalid_status}).to_string()).unwrap();
        assert!(log.status.is_none());
    }
}
