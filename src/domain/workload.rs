use super::{ContainerName, DeploymentName, DeploymentUid, Namespace, PodName, PodUid, Revision};

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workload {
    pub namespace: Namespace,
    pub deployment: DeploymentName,
    #[serde(default)]
    pub deployment_uid: Option<DeploymentUid>,
    pub revision: Revision,
    pub pod: PodName,
    pub pod_uid: PodUid,
    pub container: ContainerName,
}
