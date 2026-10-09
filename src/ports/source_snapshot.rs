use crate::domain::{Observation, Workload};
pub struct SourceSnapshot {
    pub observations: Vec<Observation>,
    pub workloads: Vec<Workload>,
}
