use super::{ConcurrencyLimit, Latency, Ratio, RequestCount, WindowCount};
use crate::domain::DomainError;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct BackpressurePolicy {
    pub minimum: ConcurrencyLimit,
    pub maximum: ConcurrencyLimit,
    pub latency_budget: Latency,
    pub error_budget: Ratio,
    pub minimum_samples: RequestCount,
    pub healthy_windows: WindowCount,
    pub cooldown: Duration,
}

impl BackpressurePolicy {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.minimum.value() > self.maximum.value()
            || self.latency_budget.value() <= 0.0
            || self.error_budget.value() <= 0.0
            || !(1..=1_000_000).contains(&self.minimum_samples.value())
            || !(Duration::from_millis(100)..=Duration::from_secs(300)).contains(&self.cooldown)
        {
            return Err(DomainError::InvalidValue("BackpressurePolicy"));
        }
        Ok(())
    }
    pub fn permits(&self, limit: ConcurrencyLimit) -> bool {
        (self.minimum.value()..=self.maximum.value()).contains(&limit.value())
    }
}
