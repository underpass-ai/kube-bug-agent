use super::{ControlDecision, ExecutionStatus};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecordedDecision {
    pub decision: ControlDecision,
    pub status: ExecutionStatus,
}
