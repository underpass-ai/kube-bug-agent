use crate::{
    application::IncidentQuery,
    domain::{AnalysisFailure, Diagnosis, Incident, IncidentId, IngestReceipt, Observation},
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait IncidentRepository: Send + Sync {
    async fn record(&self, observation: Observation) -> Result<IngestReceipt>;
    async fn list(&self, query: IncidentQuery) -> Result<Vec<Incident>>;
    async fn occurrences(&self, id: IncidentId) -> Result<Vec<Observation>>;
    async fn claim_pending(&self, now: DateTime<Utc>) -> Result<Option<Incident>>;
    async fn finish_analysis(
        &self,
        id: IncidentId,
        diagnosis: Result<Diagnosis, AnalysisFailure>,
        now: DateTime<Utc>,
    ) -> Result<()>;
    async fn health(&self) -> Result<()>;
}
