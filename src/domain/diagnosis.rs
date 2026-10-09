use super::{
    Confidence, DiagnosisSummary, DomainError, EventId, EvidenceReferences, RecommendedChecks,
    SuspectedCause,
};
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnosis {
    pub summary: DiagnosisSummary,
    pub suspected_cause: SuspectedCause,
    pub confidence: Confidence,
    pub recommended_checks: RecommendedChecks,
    pub evidence_refs: EvidenceReferences,
}
impl Diagnosis {
    pub fn validate_for(&self, event: &EventId) -> Result<(), DomainError> {
        if self
            .evidence_refs
            .items()
            .iter()
            .any(|reference| reference != event)
        {
            return Err(DomainError::UnknownEvidence);
        }
        Ok(())
    }
}
