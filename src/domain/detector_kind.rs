#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorKind {
    ApplicationError,
    Http5xx,
    LocalProbeFailed,
    ContainerOom,
    CrashLoop,
    ImagePullFailed,
    ContainerConfigError,
    PodUnschedulable,
    PodNotReady,
    RolloutStalled,
    KubernetesWarning,
}
impl DetectorKind {
    pub fn waiting(reason: &str) -> Option<Self> {
        match reason {
            "CrashLoopBackOff" => Some(Self::CrashLoop),
            "ImagePullBackOff" | "ErrImagePull" => Some(Self::ImagePullFailed),
            "CreateContainerConfigError" | "CreateContainerError" => {
                Some(Self::ContainerConfigError)
            }
            _ => None,
        }
    }
}
