use super::KubernetesSnapshot;
use crate::ports::{ObservationSource, SourceSnapshot};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use std::path::PathBuf;
pub struct FixtureSource {
    path: PathBuf,
}
impl FixtureSource {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}
#[async_trait]
impl ObservationSource for FixtureSource {
    async fn collect(&self) -> Result<SourceSnapshot> {
        let snapshot: KubernetesSnapshot =
            serde_json::from_slice(&tokio::fs::read(&self.path).await?)?;
        Ok(SourceSnapshot {
            workloads: snapshot.topology().into_values().collect(),
            observations: snapshot.detect(Utc::now(), 60),
        })
    }
}
