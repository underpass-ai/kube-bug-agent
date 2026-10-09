use crate::domain::DomainError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowCount(u32);

impl WindowCount {
    pub fn new(value: u32) -> Result<Self, DomainError> {
        if !(1..=100).contains(&value) {
            return Err(DomainError::InvalidValue("WindowCount"));
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u32 {
        self.0
    }
}
