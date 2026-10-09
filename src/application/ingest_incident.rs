use crate::{
    domain::{IngestReceipt, Observation},
    ports::IncidentRepository,
};
use anyhow::Result;
use std::sync::Arc;
pub struct IngestIncident {
    repository: Arc<dyn IncidentRepository>,
}
impl IngestIncident {
    pub fn new(repository: Arc<dyn IncidentRepository>) -> Self {
        Self { repository }
    }
    pub async fn execute(&self, observation: Observation) -> Result<IngestReceipt> {
        self.repository.record(observation).await
    }
}
