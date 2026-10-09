use crate::domain::DomainError;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Latency(f64);

impl Latency {
    pub fn milliseconds(value: f64) -> Result<Self, DomainError> {
        if !value.is_finite() || !(0.0..=3_600_000.0).contains(&value) {
            return Err(DomainError::InvalidValue("Latency"));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> f64 {
        self.0
    }
}
impl TryFrom<f64> for Latency {
    type Error = DomainError;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::milliseconds(value)
    }
}
impl From<Latency> for f64 {
    fn from(value: Latency) -> Self {
        value.0
    }
}
