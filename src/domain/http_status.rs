use super::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpStatus(u16);
impl HttpStatus {
    pub fn new(value: u16) -> Result<Self, DomainError> {
        if !(100..600).contains(&value) {
            return Err(DomainError::InvalidValue("HttpStatus"));
        }
        Ok(Self(value))
    }

    pub fn is_server_error(self) -> bool {
        self.0 >= 500
    }
}
