use super::{DomainError, EventId};
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(try_from = "Vec<EventId>", into = "Vec<EventId>")]
pub struct EvidenceReferences(Vec<EventId>);
impl EvidenceReferences {
    pub fn new(values: Vec<EventId>) -> Result<Self, DomainError> {
        if values.is_empty() || values.len() > 10 {
            return Err(DomainError::InvalidValue("EvidenceReferences"));
        }
        Ok(Self(values))
    }
    pub fn items(&self) -> &[EventId] {
        &self.0
    }
}
impl TryFrom<Vec<EventId>> for EvidenceReferences {
    type Error = DomainError;
    fn try_from(values: Vec<EventId>) -> Result<Self, Self::Error> {
        Self::new(values)
    }
}
impl From<EvidenceReferences> for Vec<EventId> {
    fn from(value: EvidenceReferences) -> Self {
        value.0
    }
}
