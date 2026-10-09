use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use kube_bug_agent::{
    application::IncidentQuery,
    domain::{AnalysisFailure, Diagnosis, Incident, IncidentId, IngestReceipt, Observation},
    ports::IncidentRepository,
};
pub struct FaultyRepository;
#[async_trait]
impl IncidentRepository for FaultyRepository {
    async fn record(&self, _: Observation) -> Result<IngestReceipt> {
        anyhow::bail!("unavailable")
    }
    async fn list(&self, _: IncidentQuery) -> Result<Vec<Incident>> {
        anyhow::bail!("unavailable")
    }
    async fn occurrences(&self, _: IncidentId) -> Result<Vec<Observation>> {
        anyhow::bail!("unavailable")
    }
    async fn claim_pending(&self, _: DateTime<Utc>) -> Result<Option<Incident>> {
        anyhow::bail!("unavailable")
    }
    async fn finish_analysis(
        &self,
        _: IncidentId,
        _: Result<Diagnosis, AnalysisFailure>,
        _: DateTime<Utc>,
    ) -> Result<()> {
        anyhow::bail!("unavailable")
    }
    async fn health(&self) -> Result<()> {
        anyhow::bail!("unavailable")
    }
}
