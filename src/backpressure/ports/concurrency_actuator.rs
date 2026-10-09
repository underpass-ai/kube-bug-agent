use super::super::domain::{BackendName, ConcurrencyLimit};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait ConcurrencyActuator: Send + Sync {
    async fn current(&self, backend: &BackendName) -> Result<ConcurrencyLimit>;
    async fn apply(
        &self,
        backend: &BackendName,
        expected: ConcurrencyLimit,
        desired: ConcurrencyLimit,
    ) -> Result<()>;
}
