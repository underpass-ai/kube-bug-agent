use async_trait::async_trait;
use kube_bug_agent::{
    domain::{Diagnosis, Observation},
    ports::DiagnosisProvider,
};
pub struct FixedProvider;
#[async_trait]
impl DiagnosisProvider for FixedProvider {
    async fn analyze(&self, observation: &Observation) -> anyhow::Result<Diagnosis> {
        Ok(super::diagnosis(&observation.event_id))
    }
}
