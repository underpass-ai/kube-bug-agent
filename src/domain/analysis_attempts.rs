use super::DomainError;
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct AnalysisAttempts(u8);
impl AnalysisAttempts {
    pub fn value(self) -> u8 {
        self.0
    }
    pub fn increment(self) -> Result<Self, DomainError> {
        Self::try_from(self.0.saturating_add(1))
    }
}
impl TryFrom<u8> for AnalysisAttempts {
    type Error = DomainError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value > 3 {
            return Err(DomainError::InvalidValue("AnalysisAttempts"));
        }
        Ok(Self(value))
    }
}
impl From<AnalysisAttempts> for u8 {
    fn from(value: AnalysisAttempts) -> Self {
        value.0
    }
}
