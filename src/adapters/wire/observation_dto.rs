use super::WorkloadDto;
use crate::domain::{
    DetectorKind, DomainError, ErrorSignature, EventId, Evidence, Observation, Severity,
};
use chrono::{DateTime, Utc};
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationDto {
    event_id: String,
    observed_at: DateTime<Utc>,
    workload: WorkloadDto,
    detector: DetectorKind,
    severity: Severity,
    signature: String,
    evidence: serde_json::Value,
}
impl TryFrom<ObservationDto> for Observation {
    type Error = DomainError;
    fn try_from(value: ObservationDto) -> Result<Self, Self::Error> {
        Ok(Self {
            event_id: EventId::new(value.event_id)?,
            observed_at: value.observed_at,
            workload: value.workload.try_into()?,
            detector: value.detector,
            severity: value.severity,
            signature: ErrorSignature::new(value.signature)?,
            evidence: Evidence::new(value.evidence)?,
        })
    }
}
impl From<&Observation> for ObservationDto {
    fn from(value: &Observation) -> Self {
        Self {
            event_id: value.event_id.as_str().into(),
            observed_at: value.observed_at,
            workload: (&value.workload).into(),
            detector: value.detector.clone(),
            severity: value.severity.clone(),
            signature: value.signature.as_str().into(),
            evidence: value.evidence.value().clone(),
        }
    }
}
