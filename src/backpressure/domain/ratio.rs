use crate::domain::DomainError;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Ratio(f64);

impl Ratio {
    pub fn new(value: f64) -> Result<Self, DomainError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(DomainError::InvalidValue("Ratio"));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> f64 {
        self.0
    }
}
impl TryFrom<f64> for Ratio {
    type Error = DomainError;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<Ratio> for f64 {
    fn from(value: Ratio) -> Self {
        value.0
    }
}
