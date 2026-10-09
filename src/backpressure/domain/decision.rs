use super::{BackendName, ConcurrencyLimit, DecisionReason, PressureWindow};
use crate::domain::EventId;
use chrono::{DateTime, Utc};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ControlDecision {
    pub id: EventId,
    pub backend: BackendName,
    pub observed_at: DateTime<Utc>,
    pub before: ConcurrencyLimit,
    pub after: ConcurrencyLimit,
    pub reason: DecisionReason,
    pub window: Option<PressureWindow>,
}

impl ControlDecision {
    pub fn changes_limit(&self) -> bool {
        self.before != self.after
    }
}
