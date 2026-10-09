use crate::{
    domain::{AnalysisFailure, Evidence},
    ports::{DiagnosisProvider, IncidentRepository},
};
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;

pub struct AnalyzeIncident {
    repository: Arc<dyn IncidentRepository>,
    provider: Arc<dyn DiagnosisProvider>,
}
impl AnalyzeIncident {
    pub fn new(
        repository: Arc<dyn IncidentRepository>,
        provider: Arc<dyn DiagnosisProvider>,
    ) -> Self {
        Self {
            repository,
            provider,
        }
    }
    pub async fn execute_next(&self) -> Result<bool> {
        let Some(incident) = self.repository.claim_pending(Utc::now()).await? else {
            return Ok(false);
        };
        let diagnosis = self
            .provider
            .analyze(incident.observation())
            .await
            .and_then(|diagnosis| {
                diagnosis.validate_for(&incident.observation().event_id)?;
                Ok(diagnosis)
            });
        let result = diagnosis.map_err(|error| {
            let text = Evidence::redact_text(&error.to_string())
                .chars()
                .take(1000)
                .collect::<String>();
            AnalysisFailure::new(if text.trim().is_empty() {
                "Analysis failed".to_owned()
            } else {
                text
            })
            .expect("bounded failure")
        });
        self.repository
            .finish_analysis(incident.id().clone(), result, Utc::now())
            .await?;
        Ok(true)
    }
}
