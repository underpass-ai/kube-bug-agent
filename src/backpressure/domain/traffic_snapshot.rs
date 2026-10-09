use super::{Latency, RequestCount};
use chrono::{DateTime, Utc};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct TrafficSnapshot {
    pub observed_at: DateTime<Utc>,
    pub uptime: Duration,
    pub completed: RequestCount,
    pub failed: RequestCount,
    pub timed_out: RequestCount,
    pub rejected: RequestCount,
    pub active: RequestCount,
    pub pending: RequestCount,
    pub p95: Option<Latency>,
}
