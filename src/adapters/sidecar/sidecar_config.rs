use super::SpoolCapacity;
use crate::{adapters::config::LocalHealthEndpoint, domain::Workload};
use std::path::PathBuf;
pub struct SidecarConfig {
    pub log: PathBuf,
    pub spool: PathBuf,
    pub workload: Workload,
    pub health: Option<LocalHealthEndpoint>,
    pub capacity: SpoolCapacity,
}
