use super::QueryLimit;
use crate::domain::{DeploymentName, Namespace};
#[derive(Default)]
pub struct IncidentQuery {
    pub namespace: Option<Namespace>,
    pub deployment: Option<DeploymentName>,
    pub limit: QueryLimit,
}
