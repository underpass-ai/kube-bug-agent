use super::super::domain::{ControlDecision, ExecutionStatus, RecordedDecision};
use crate::{application::QueryLimit, domain::EventId};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait DecisionRepository: Send + Sync {
    async fn prepare(&self, decision: &ControlDecision) -> Result<()>;
    async fn finish(&self, id: &EventId, status: ExecutionStatus) -> Result<()>;
    async fn recent(&self, limit: QueryLimit) -> Result<Vec<RecordedDecision>>;
}
