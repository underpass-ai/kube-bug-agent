use crate::domain::{EventId, Observation};
use anyhow::Result;
use async_trait::async_trait;
#[async_trait]
pub trait ObservationOutbox: Send + Sync {
    async fn next(&self) -> Result<Option<Observation>>;
    async fn acknowledge(&self, id: EventId) -> Result<()>;
}
