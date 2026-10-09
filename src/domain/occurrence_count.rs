use super::DomainError;
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct OccurrenceCount(u64);
impl OccurrenceCount {
    pub fn one() -> Self {
        Self(1)
    }
    pub fn value(self) -> u64 {
        self.0
    }
    pub fn increment(&mut self) -> Result<(), DomainError> {
        self.0 = self
            .0
            .checked_add(1)
            .ok_or(DomainError::InvalidValue("OccurrenceCount"))?;
        Ok(())
    }
}
impl TryFrom<u64> for OccurrenceCount {
    type Error = DomainError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value == 0 {
            return Err(DomainError::InvalidValue("OccurrenceCount"));
        }
        Ok(Self(value))
    }
}
impl From<OccurrenceCount> for u64 {
    fn from(value: OccurrenceCount) -> Self {
        value.0
    }
}
