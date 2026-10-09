use super::{Latency, Ratio, RequestCount, TrafficSnapshot};
use crate::domain::DomainError;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PressureWindow {
    pub completed: RequestCount,
    pub failed: RequestCount,
    pub rejected: RequestCount,
    pub active: RequestCount,
    pub pending: RequestCount,
    pub p95: Option<Latency>,
    pub error_ratio: Ratio,
}

impl PressureWindow {
    pub fn between(
        previous: &TrafficSnapshot,
        current: &TrafficSnapshot,
    ) -> Result<Option<Self>, DomainError> {
        if current.observed_at <= previous.observed_at {
            return Err(DomainError::InvalidValue("sample time must advance"));
        }
        if current.uptime < previous.uptime {
            return Ok(None);
        }
        let delta = |a: RequestCount, b: RequestCount| a.value().checked_sub(b.value());
        let Some(completed) = delta(current.completed, previous.completed) else {
            return Ok(None);
        };
        let Some(failed) = delta(current.failed, previous.failed) else {
            return Ok(None);
        };
        let Some(timeouts) = delta(current.timed_out, previous.timed_out) else {
            return Ok(None);
        };
        let Some(rejected) = delta(current.rejected, previous.rejected) else {
            return Ok(None);
        };
        // Envoy local overload responses are not backend failures.
        let completed = completed.saturating_sub(rejected);
        let failed = failed.saturating_sub(rejected).max(timeouts).min(completed);
        Ok(Some(Self {
            completed: RequestCount::new(completed),
            failed: RequestCount::new(failed),
            rejected: RequestCount::new(rejected),
            active: current.active,
            pending: current.pending,
            p95: current.p95,
            error_ratio: Ratio::new(if completed == 0 {
                0.0
            } else {
                failed as f64 / completed as f64
            })?,
        }))
    }
}
