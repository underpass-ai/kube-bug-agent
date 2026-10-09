use crate::domain::{
    ContainerName, DeploymentName, DeploymentUid, DomainError, Namespace, PodName, PodUid,
    Revision, Workload,
};
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadDto {
    namespace: String,
    deployment: String,
    #[serde(default)]
    deployment_uid: Option<String>,
    revision: String,
    pod: String,
    pod_uid: String,
    container: String,
}
impl TryFrom<WorkloadDto> for Workload {
    type Error = DomainError;
    fn try_from(value: WorkloadDto) -> Result<Self, Self::Error> {
        Ok(Self {
            namespace: Namespace::new(value.namespace)?,
            deployment: DeploymentName::new(value.deployment)?,
            deployment_uid: value.deployment_uid.map(DeploymentUid::new).transpose()?,
            revision: Revision::new(value.revision)?,
            pod: PodName::new(value.pod)?,
            pod_uid: PodUid::new(value.pod_uid)?,
            container: ContainerName::new(value.container)?,
        })
    }
}
impl From<&Workload> for WorkloadDto {
    fn from(value: &Workload) -> Self {
        Self {
            namespace: value.namespace.as_str().into(),
            deployment: value.deployment.as_str().into(),
            deployment_uid: value
                .deployment_uid
                .as_ref()
                .map(|value| value.as_str().into()),
            revision: value.revision.as_str().into(),
            pod: value.pod.as_str().into(),
            pod_uid: value.pod_uid.as_str().into(),
            container: value.container.as_str().into(),
        }
    }
}
