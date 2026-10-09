use super::SourceSnapshot;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait ObservationSource: Send + Sync {
    async fn collect(&self) -> Result<SourceSnapshot>;
}
