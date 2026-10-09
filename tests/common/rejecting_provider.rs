use async_trait::async_trait;
use kube_bug_agent::{
    domain::{Diagnosis, Observation},
    ports::DiagnosisProvider,
};
pub struct RejectingProvider;
#[async_trait]
impl DiagnosisProvider for RejectingProvider {
    async fn analyze(&self, _: &Observation) -> anyhow::Result<Diagnosis> {
        anyhow::bail!("Provider unavailable")
    }
}
