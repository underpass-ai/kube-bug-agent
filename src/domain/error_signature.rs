use super::{DomainError, Evidence};
use regex::Regex;
use std::sync::LazyLock;

static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f-]{27,}\b|\b0x[0-9a-f]+\b|\b\d+\b").unwrap()
});

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ErrorSignature(String);

impl ErrorSignature {
    pub fn new(input: impl Into<String>) -> Result<Self, DomainError> {
        let input = Evidence::redact_text(&input.into());
        let value = IDENTIFIER
            .replace_all(&input, "#")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if value.is_empty() || value.len() > 1024 {
            return Err(DomainError::InvalidValue("ErrorSignature"));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for ErrorSignature {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<ErrorSignature> for String {
    fn from(value: ErrorSignature) -> Self {
        value.0
    }
}
