use super::{DetectorKind, ErrorSignature, EventId, Evidence, IncidentId, Severity, Workload};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub event_id: EventId,
    pub observed_at: DateTime<Utc>,
    pub workload: Workload,
    pub detector: DetectorKind,
    pub severity: Severity,
    pub signature: ErrorSignature,
    pub evidence: Evidence,
}
impl Observation {
    pub fn fingerprint(&self) -> IncidentId {
        let owner = self
            .workload
            .deployment_uid
            .as_ref()
            .map(|uid| uid.as_str())
            .unwrap_or(self.workload.pod_uid.as_str());
        let data = serde_json::to_vec(&(
            &self.workload.namespace,
            owner,
            &self.workload.revision,
            &self.workload.container,
            &self.detector,
            &self.signature,
        ))
        .expect("serializable identity");
        IncidentId::new(format!("{:x}", Sha256::digest(data))).expect("valid digest")
    }
    pub fn source_event_id(data: &[u8]) -> EventId {
        EventId::new(format!("{:x}", Sha256::digest(data))).expect("valid digest")
    }
}
