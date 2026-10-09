use crate::domain::{
    Confidence, Diagnosis, DiagnosisSummary, DomainError, EventId, EvidenceReferences,
    InvestigationStep, RecommendedChecks, SuspectedCause,
};
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosisDto {
    summary: String,
    suspected_cause: String,
    confidence: Confidence,
    recommended_checks: Vec<String>,
    evidence_refs: Vec<String>,
}
impl TryFrom<DiagnosisDto> for Diagnosis {
    type Error = DomainError;
    fn try_from(value: DiagnosisDto) -> Result<Self, Self::Error> {
        Ok(Self {
            summary: DiagnosisSummary::new(value.summary)?,
            suspected_cause: SuspectedCause::new(value.suspected_cause)?,
            confidence: value.confidence,
            recommended_checks: RecommendedChecks::new(
                value
                    .recommended_checks
                    .into_iter()
                    .map(InvestigationStep::new)
                    .collect::<Result<_, _>>()?,
            )?,
            evidence_refs: EvidenceReferences::new(
                value
                    .evidence_refs
                    .into_iter()
                    .map(EventId::new)
                    .collect::<Result<_, _>>()?,
            )?,
        })
    }
}
impl From<&Diagnosis> for DiagnosisDto {
    fn from(value: &Diagnosis) -> Self {
        Self {
            summary: value.summary.as_str().into(),
            suspected_cause: value.suspected_cause.as_str().into(),
            confidence: value.confidence.clone(),
            recommended_checks: value
                .recommended_checks
                .items()
                .iter()
                .map(|step| step.as_str().into())
                .collect(),
            evidence_refs: value
                .evidence_refs
                .items()
                .iter()
                .map(|reference| reference.as_str().into())
                .collect(),
        }
    }
}
