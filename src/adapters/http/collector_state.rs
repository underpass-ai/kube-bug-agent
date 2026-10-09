use crate::{
    adapters::config::AccessToken,
    domain::{PodUid, Workload},
    ports::IncidentRepository,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
#[derive(Clone)]
pub struct CollectorState {
    pub repository: Arc<dyn IncidentRepository>,
    pub token: Option<AccessToken>,
    pub topology: Option<Arc<RwLock<HashMap<PodUid, Workload>>>>,
}
