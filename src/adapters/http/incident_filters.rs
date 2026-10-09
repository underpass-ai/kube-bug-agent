use crate::{
    application::{IncidentQuery, QueryLimit},
    domain::{DeploymentName, Namespace},
};
use anyhow::Result;
#[derive(serde::Deserialize)]
pub struct IncidentFilters {
    namespace: Option<String>,
    deployment: Option<String>,
    limit: Option<u32>,
}
impl IncidentFilters {
    pub fn into_query(self) -> Result<IncidentQuery> {
        Ok(IncidentQuery {
            namespace: self.namespace.map(Namespace::new).transpose()?,
            deployment: self.deployment.map(DeploymentName::new).transpose()?,
            limit: self
                .limit
                .map(QueryLimit::new)
                .transpose()?
                .unwrap_or_default(),
        })
    }
}
