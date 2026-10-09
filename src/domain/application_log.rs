use super::{ErrorSignature, Evidence, HttpStatus, LogLevel};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct ApplicationLog {
    pub observed_at: DateTime<Utc>,
    pub level: LogLevel,
    pub status: Option<HttpStatus>,
    pub signature: ErrorSignature,
    pub evidence: Evidence,
}
