use crate::domain::DomainError;
#[derive(Clone, Copy)]
pub struct QueryLimit(u32);
impl QueryLimit {
    pub fn new(value: u32) -> Result<Self, DomainError> {
        if !(1..=500).contains(&value) {
            return Err(DomainError::InvalidValue("QueryLimit"));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u32 {
        self.0
    }
}
impl Default for QueryLimit {
    fn default() -> Self {
        Self(100)
    }
}
