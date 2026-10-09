use crate::domain::{ApplicationLog, ErrorSignature, Evidence, HttpStatus, LogLevel};
use chrono::Utc;
use serde_json::Value;

pub struct JsonLogDecoder;
impl JsonLogDecoder {
    pub fn decode(line: &str) -> Option<ApplicationLog> {
        let log: Value = serde_json::from_str(line).ok()?;
        let level = match log
            .get("level")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "trace" => LogLevel::Trace,
            "debug" => LogLevel::Debug,
            "info" => LogLevel::Info,
            "warn" | "warning" => LogLevel::Warning,
            "error" => LogLevel::Error,
            "fatal" => LogLevel::Fatal,
            "panic" => LogLevel::Panic,
            "critical" => LogLevel::Critical,
            _ => LogLevel::Unknown,
        };
        let status = log
            .get("status")
            .or_else(|| log.get("status_code"))
            .and_then(Value::as_u64)
            .and_then(|status| u16::try_from(status).ok())
            .and_then(|status| HttpStatus::new(status).ok());
        let message = log
            .get("message")
            .or_else(|| log.get("msg"))
            .and_then(Value::as_str)
            .unwrap_or("HTTP server error");
        let observed_at = log
            .get("timestamp")
            .or_else(|| log.get("time"))
            .and_then(Value::as_str)
            .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        Some(ApplicationLog {
            observed_at,
            level,
            status,
            signature: ErrorSignature::new(message).ok()?,
            evidence: Evidence::new(log).ok()?,
        })
    }
}
