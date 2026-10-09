use crate::domain::{IngestReceipt, Observation};
use anyhow::Result;
use async_trait::async_trait;
#[async_trait]
pub trait ObservationSink: Send + Sync {
    async fn deliver(&self, observation: &Observation) -> Result<IngestReceipt>;
}
