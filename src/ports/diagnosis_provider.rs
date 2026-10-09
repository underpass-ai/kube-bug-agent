use crate::domain::{Diagnosis, Observation};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait DiagnosisProvider: Send + Sync {
    async fn analyze(&self, observation: &Observation) -> Result<Diagnosis>;
}
