use super::super::domain::{BackendName, TrafficSnapshot};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait PressureSource: Send + Sync {
    async fn sample(&self, backend: &BackendName) -> Result<TrafficSnapshot>;
}
