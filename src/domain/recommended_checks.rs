use super::{DomainError, InvestigationStep};
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(try_from = "Vec<InvestigationStep>", into = "Vec<InvestigationStep>")]
pub struct RecommendedChecks(Vec<InvestigationStep>);
impl RecommendedChecks {
    pub fn new(values: Vec<InvestigationStep>) -> Result<Self, DomainError> {
        if values.is_empty() || values.len() > 10 {
            return Err(DomainError::InvalidValue("RecommendedChecks"));
        }
        Ok(Self(values))
    }
    pub fn items(&self) -> &[InvestigationStep] {
        &self.0
    }
}
impl TryFrom<Vec<InvestigationStep>> for RecommendedChecks {
    type Error = DomainError;
    fn try_from(values: Vec<InvestigationStep>) -> Result<Self, Self::Error> {
        Self::new(values)
    }
}
impl From<RecommendedChecks> for Vec<InvestigationStep> {
    fn from(value: RecommendedChecks) -> Self {
        value.0
    }
}
