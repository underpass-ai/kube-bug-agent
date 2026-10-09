use crate::ports::{IncidentRepository, ObservationSource, SourceSnapshot};
use anyhow::Result;
use std::sync::Arc;
pub struct CollectIncidents {
    repository: Arc<dyn IncidentRepository>,
    source: Arc<dyn ObservationSource>,
}
impl CollectIncidents {
    pub fn new(
        repository: Arc<dyn IncidentRepository>,
        source: Arc<dyn ObservationSource>,
    ) -> Self {
        Self { repository, source }
    }
    pub async fn execute(&self) -> Result<SourceSnapshot> {
        let snapshot = self.source.collect().await?;
        for observation in &snapshot.observations {
            self.repository.record(observation.clone()).await?;
        }
        Ok(snapshot)
    }
}
