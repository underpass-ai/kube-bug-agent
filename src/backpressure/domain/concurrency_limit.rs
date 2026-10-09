use crate::domain::DomainError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct ConcurrencyLimit(u32);

impl ConcurrencyLimit {
    pub fn new(value: u32) -> Result<Self, DomainError> {
        if !(1..=10_000).contains(&value) {
            return Err(DomainError::InvalidValue("ConcurrencyLimit"));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u32 {
        self.0
    }
}
impl TryFrom<u32> for ConcurrencyLimit {
    type Error = DomainError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<ConcurrencyLimit> for u32 {
    fn from(value: ConcurrencyLimit) -> Self {
        value.0
    }
}
