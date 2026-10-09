use super::{
    BackendName, BackpressurePolicy, ConcurrencyLimit, ControlDecision, DecisionReason,
    PressureWindow, TrafficSnapshot,
};
use crate::domain::{DomainError, EventId};
use chrono::{DateTime, Utc};

pub struct BackpressureController {
    backend: BackendName,
    policy: BackpressurePolicy,
    previous: Option<TrafficSnapshot>,
    healthy_streak: u32,
    last_change: Option<DateTime<Utc>>,
}

impl BackpressureController {
    pub fn new(backend: BackendName, policy: BackpressurePolicy) -> Result<Self, DomainError> {
        policy.validate()?;
        Ok(Self {
            backend,
            policy,
            previous: None,
            healthy_streak: 0,
            last_change: None,
        })
    }
    pub fn backend(&self) -> &BackendName {
        &self.backend
    }
    pub fn observe(
        &mut self,
        snapshot: TrafficSnapshot,
        current: ConcurrencyLimit,
    ) -> Result<ControlDecision, DomainError> {
        if !self.policy.permits(current) {
            return Err(DomainError::InvalidValue("current limit outside policy"));
        }
        let window = self
            .previous
            .as_ref()
            .map(|previous| PressureWindow::between(previous, &snapshot))
            .transpose()?;
        let mut reason = if window.is_none() {
            DecisionReason::Baseline
        } else {
            DecisionReason::CounterReset
        };
        let window = window.flatten();
        let mut after = current;
        if let Some(window) = &window {
            if window.completed.value() < self.policy.minimum_samples.value() {
                self.healthy_streak = 0;
                reason = DecisionReason::InsufficientTraffic;
            } else {
                let overloaded = window.error_ratio.value() >= self.policy.error_budget.value()
                    || window.p95.is_some_and(|latency| {
                        latency.value() >= self.policy.latency_budget.value()
                    });
                let healthy = window.failed.value() == 0
                    && window.p95.is_some_and(|latency| {
                        latency.value() < self.policy.latency_budget.value() * 0.75
                    });
                self.healthy_streak = if healthy {
                    self.healthy_streak.saturating_add(1)
                } else {
                    0
                };
                let cooling = self.last_change.is_some_and(|last| {
                    (snapshot.observed_at - last).to_std().unwrap_or_default()
                        < self.policy.cooldown
                });
                if cooling {
                    reason = DecisionReason::Cooldown;
                } else if overloaded {
                    after = ConcurrencyLimit::new(
                        (current.value() / 2).max(self.policy.minimum.value()),
                    )?;
                    reason = if after == current {
                        DecisionReason::AtMinimum
                    } else {
                        DecisionReason::Reduce
                    };
                } else if self.healthy_streak >= self.policy.healthy_windows.value() {
                    after = ConcurrencyLimit::new(
                        (current.value() + 1).min(self.policy.maximum.value()),
                    )?;
                    reason = if after == current {
                        DecisionReason::AtMaximum
                    } else {
                        DecisionReason::Recover
                    };
                } else {
                    reason = DecisionReason::Hold;
                }
            }
        } else {
            self.healthy_streak = 0;
        }
        let decision = ControlDecision {
            id: EventId::new(uuid::Uuid::new_v4().to_string())?,
            backend: self.backend.clone(),
            observed_at: snapshot.observed_at,
            before: current,
            after,
            reason,
            window,
        };
        self.previous = Some(snapshot);
        Ok(decision)
    }
    pub fn confirm(&mut self, decision: &ControlDecision) {
        if decision.changes_limit() {
            self.last_change = Some(decision.observed_at);
            self.healthy_streak = 0;
        }
    }
}
