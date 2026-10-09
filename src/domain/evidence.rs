use super::DomainError;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static CREDENTIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(authorization\s*[:=]\s*(?:bearer\s+|basic\s+)?|(?:api[_-]?key|access[_-]?token|token|password|secret)\s*[:=]\s*["']?)[^\s,"';}]+"#).unwrap()
});
static API_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bsk-[A-Za-z0-9_-]{8,}\b").unwrap());

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(try_from = "Value", into = "Value")]
pub struct Evidence(Value);

impl Evidence {
    pub fn new(value: Value) -> Result<Self, DomainError> {
        if !value.is_object() {
            return Err(DomainError::InvalidValue("Evidence"));
        }
        let value = Self::redact_value(&value);
        if serde_json::to_vec(&value)
            .map_err(|_| DomainError::InvalidValue("Evidence"))?
            .len()
            > 32_768
        {
            return Err(DomainError::InvalidValue("Evidence"));
        }
        Ok(Self(value))
    }
    pub fn value(&self) -> &Value {
        &self.0
    }
    pub fn redact_text(input: &str) -> String {
        let value = CREDENTIAL.replace_all(input, "${1}[REDACTED]");
        API_KEY.replace_all(&value, "[REDACTED]").into_owned()
    }
    pub fn redact_value(input: &Value) -> Value {
        match input {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| {
                        let key_normalized = key.to_ascii_lowercase().replace(['_', '-'], "");
                        let sensitive = [
                            "authorization",
                            "apikey",
                            "token",
                            "password",
                            "secret",
                            "cookie",
                        ]
                        .iter()
                        .any(|term| key_normalized.contains(term));
                        (
                            key.clone(),
                            if sensitive {
                                Value::String("[REDACTED]".into())
                            } else {
                                Self::redact_value(value)
                            },
                        )
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(Self::redact_value).collect()),
            Value::String(s) => Value::String(Self::redact_text(s)),
            other => other.clone(),
        }
    }
}
impl TryFrom<Value> for Evidence {
    type Error = DomainError;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<Evidence> for Value {
    fn from(value: Evidence) -> Self {
        value.0
    }
}
