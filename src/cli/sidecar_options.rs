use crate::{
    adapters::{
        config::LocalHealthEndpoint,
        sidecar::{SidecarConfig, SpoolCapacity},
    },
    domain::{
        ContainerName, DeploymentName, DeploymentUid, Namespace, PodName, PodUid, Revision,
        Workload,
    },
};
use anyhow::Result;
use std::path::PathBuf;
#[derive(clap::Args)]
pub struct SidecarOptions {
    #[arg(long)]
    pub logs: PathBuf,
    #[arg(long, default_value = "data/spool.db")]
    pub spool: PathBuf,
    #[arg(long, env = "COLLECTOR_URL", default_value = "http://127.0.0.1:8787")]
    pub collector_url: String,
    #[arg(long, env = "POD_NAMESPACE", default_value = "default")]
    pub namespace: String,
    #[arg(long, env = "DEPLOYMENT_NAME")]
    pub deployment: String,
    #[arg(long, env = "DEPLOYMENT_UID")]
    pub deployment_uid: Option<String>,
    #[arg(long, env = "DEPLOYMENT_REVISION", default_value = "unknown")]
    pub revision: String,
    #[arg(long, env = "POD_NAME")]
    pub pod: String,
    #[arg(long, env = "POD_UID")]
    pub pod_uid: String,
    #[arg(long, env = "APP_CONTAINER", default_value = "app")]
    pub container: String,
    #[arg(long)]
    pub health_url: Option<String>,
    #[arg(long, default_value_t = 1000)]
    pub spool_capacity: u32,
    #[arg(long)]
    pub once: bool,
}
impl SidecarOptions {
    pub fn config(&self) -> Result<SidecarConfig> {
        Ok(SidecarConfig {
            log: self.logs.clone(),
            spool: self.spool.clone(),
            capacity: SpoolCapacity::new(self.spool_capacity)?,
            health: self
                .health_url
                .as_deref()
                .map(LocalHealthEndpoint::new)
                .transpose()?,

            workload: Workload {
                namespace: Namespace::new(self.namespace.clone())?,
                deployment: DeploymentName::new(self.deployment.clone())?,
                deployment_uid: self
                    .deployment_uid
                    .clone()
                    .map(DeploymentUid::new)
                    .transpose()?,
                revision: Revision::new(self.revision.clone())?,
                pod: PodName::new(self.pod.clone())?,
                pod_uid: PodUid::new(self.pod_uid.clone())?,
                container: ContainerName::new(self.container.clone())?,
            },
        })
    }
}
