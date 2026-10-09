use chrono::{DateTime, Utc};
use k8s_openapi::{
    api::{
        apps::v1::{Deployment, ReplicaSet},
        core::v1::{Event, Pod},
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::domain::{
    ContainerName, DeploymentName, DeploymentUid, DetectorKind, ErrorSignature, Evidence,
    Namespace, Observation, PodName, PodUid, Revision, Severity, Workload,
};

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KubernetesSnapshot {
    #[serde(default)]
    pub pods: Vec<Pod>,
    #[serde(default)]
    pub replica_sets: Vec<ReplicaSet>,
    #[serde(default)]
    pub deployments: Vec<Deployment>,
    #[serde(default)]
    pub events: Vec<Event>,
}

impl KubernetesSnapshot {
    pub fn workload(&self, pod: &Pod) -> Option<Workload> {
        let owner = pod
            .metadata
            .owner_references
            .as_ref()?
            .iter()
            .find(|owner| owner.kind == "ReplicaSet" && owner.controller == Some(true))?;
        let rs = self
            .replica_sets
            .iter()
            .find(|rs| rs.metadata.uid.as_deref() == Some(owner.uid.as_str()))?;
        let owner = rs
            .metadata
            .owner_references
            .as_ref()?
            .iter()
            .find(|owner| owner.kind == "Deployment" && owner.controller == Some(true))?;
        let deployment = self
            .deployments
            .iter()
            .find(|deployment| deployment.metadata.uid.as_deref() == Some(owner.uid.as_str()))?;
        Some(Workload {
            namespace: Namespace::new(pod.metadata.namespace.clone()?).ok()?,
            deployment: DeploymentName::new(deployment.metadata.name.clone()?).ok()?,
            deployment_uid: deployment
                .metadata
                .uid
                .clone()
                .map(DeploymentUid::new)
                .transpose()
                .ok()?,
            revision: Revision::new(
                rs.metadata
                    .annotations
                    .as_ref()
                    .and_then(|a| a.get("deployment.kubernetes.io/revision"))
                    .cloned()
                    .or_else(|| rs.metadata.uid.clone())?,
            )
            .ok()?,
            pod: PodName::new(pod.metadata.name.clone()?).ok()?,
            pod_uid: PodUid::new(pod.metadata.uid.clone()?).ok()?,
            container: ContainerName::new("pod").expect("valid name"),
        })
    }

    pub fn topology(&self) -> HashMap<PodUid, Workload> {
        self.pods
            .iter()
            .filter_map(|pod| self.workload(pod))
            .map(|workload| (workload.pod_uid.clone(), workload))
            .collect()
    }

    pub fn detect(&self, now: DateTime<Utc>, startup_grace: i64) -> Vec<Observation> {
        let mut observations = Vec::new();
        for pod in &self.pods {
            if pod.metadata.deletion_timestamp.is_some() {
                continue;
            }
            let Some(workload) = self.workload(pod) else {
                continue;
            };
            let Some(status) = &pod.status else {
                continue;
            };
            for container in status.container_statuses.as_deref().unwrap_or_default() {
                if container.name == "bug-agent" {
                    continue;
                }
                let mut workload = workload.clone();
                let Ok(name) = ContainerName::new(container.name.clone()) else {
                    continue;
                };
                workload.container = name;
                let terminated = container
                    .state
                    .as_ref()
                    .and_then(|s| s.terminated.as_ref())
                    .or_else(|| {
                        container
                            .last_state
                            .as_ref()
                            .and_then(|s| s.terminated.as_ref())
                    });
                if let Some(terminated) =
                    terminated.filter(|state| state.reason.as_deref() == Some("OOMKilled"))
                {
                    observations.push(signal(&workload, DetectorKind::ContainerOom, Severity::Critical, "Container terminated: OOMKilled",
                        json!({"reason": "OOMKilled", "exit_code": terminated.exit_code, "restart_count": container.restart_count,
                               "finished_at": terminated.finished_at}),
                        terminated.finished_at.as_ref().map(|time| instant(time.0)).unwrap_or_else(|| created(&pod.metadata, now))));
                }
                if let Some(waiting) = container
                    .state
                    .as_ref()
                    .and_then(|state| state.waiting.as_ref())
                {
                    let reason = waiting.reason.as_deref().unwrap_or("");
                    let detector = DetectorKind::waiting(reason);
                    if let Some(detector) = detector {
                        observations.push(signal(
                            &workload,
                            detector,
                            Severity::Error,
                            reason,
                            json!({"reason": reason, "restart_count": container.restart_count}),
                            created(&pod.metadata, now),
                        ));
                    }
                }
            }
            if (now - created(&pod.metadata, now)).num_seconds() >= startup_grace {
                for condition in status.conditions.as_deref().unwrap_or_default() {
                    let detector = match (
                        condition.type_.as_str(),
                        condition.status.as_str(),
                        condition.reason.as_deref(),
                    ) {
                        ("PodScheduled", "False", Some("Unschedulable")) => {
                            Some(DetectorKind::PodUnschedulable)
                        }
                        ("Ready", "False", _) if status.phase.as_deref() == Some("Running") => {
                            Some(DetectorKind::PodNotReady)
                        }
                        _ => None,
                    };
                    if let Some(detector) = detector {
                        observations.push(signal(&workload, detector, Severity::Warning, "Pod condition failed",
                            json!({"condition": condition.type_, "reason": condition.reason,
                                   "message": condition.message.as_deref().map(|s| s.chars().take(2048).collect::<String>())}),
                            condition.last_transition_time.as_ref().map(|time| instant(time.0)).unwrap_or_else(|| created(&pod.metadata, now))));
                    }
                }
            }
        }
        for deployment in &self.deployments {
            if deployment.metadata.deletion_timestamp.is_some()
                || deployment
                    .spec
                    .as_ref()
                    .is_some_and(|s| s.paused == Some(true) || s.replicas == Some(0))
            {
                continue;
            }
            let Some(status) = &deployment.status else {
                continue;
            };
            if status.observed_generation.unwrap_or(-1)
                < deployment.metadata.generation.unwrap_or(0)
            {
                continue;
            }
            for condition in status.conditions.as_deref().unwrap_or_default() {
                if condition.type_ != "Progressing"
                    || condition.status != "False"
                    || !matches!(
                        condition.reason.as_deref(),
                        Some("ProgressDeadlineExceeded" | "ReplicaSetCreateError")
                    )
                {
                    continue;
                }
                if let Some(workload) = deployment_workload(deployment) {
                    observations.push(signal(&workload, DetectorKind::RolloutStalled, Severity::Error, condition.reason.as_deref().unwrap_or("Rollout stalled"),
                        json!({"reason": condition.reason, "message": condition.message.as_deref().map(|s| s.chars().take(2048).collect::<String>())}),
                        condition.last_transition_time.as_ref().map(|time| instant(time.0)).unwrap_or_else(|| created(&deployment.metadata, now))));
                }
            }
        }
        for event in &self.events {
            if event.type_.as_deref() != Some("Warning") {
                continue;
            }
            let uid = event.involved_object.uid.as_deref().unwrap_or("");
            let workload = match event.involved_object.kind.as_deref() {
                Some("Pod") => self
                    .pods
                    .iter()
                    .find(|pod| pod.metadata.uid.as_deref() == Some(uid))
                    .and_then(|pod| self.workload(pod)),
                Some("Deployment") => self
                    .deployments
                    .iter()
                    .find(|d| d.metadata.uid.as_deref() == Some(uid))
                    .and_then(deployment_workload),
                Some("ReplicaSet") => self
                    .replica_sets
                    .iter()
                    .find(|rs| rs.metadata.uid.as_deref() == Some(uid))
                    .and_then(|rs| rs.metadata.owner_references.as_ref())
                    .and_then(|owners| {
                        owners.iter().find(|owner| {
                            owner.kind == "Deployment" && owner.controller == Some(true)
                        })
                    })
                    .and_then(|owner| {
                        self.deployments
                            .iter()
                            .find(|d| d.metadata.uid.as_deref() == Some(owner.uid.as_str()))
                    })
                    .and_then(deployment_workload),
                _ => None,
            };
            if let Some(workload) = workload {
                observations.push(signal(&workload, DetectorKind::KubernetesWarning, Severity::Warning,
                    event.reason.as_deref().unwrap_or("Kubernetes warning"),
                    json!({"event_uid": event.metadata.uid, "reason": event.reason, "count": event.count,
                           "message": event.message.as_deref().map(|s| s.chars().take(2048).collect::<String>())}),
                    event.last_timestamp.as_ref().map(|time| instant(time.0))
                        .or_else(|| event.event_time.as_ref().map(|time| instant(time.0))).unwrap_or_else(|| created(&event.metadata, now))));
            }
        }
        observations
    }
}

fn deployment_workload(deployment: &Deployment) -> Option<Workload> {
    let uid = deployment.metadata.uid.clone()?;
    Some(Workload {
        namespace: Namespace::new(deployment.metadata.namespace.clone()?).ok()?,
        deployment: DeploymentName::new(deployment.metadata.name.clone()?).ok()?,
        deployment_uid: Some(DeploymentUid::new(uid.clone()).ok()?),
        revision: Revision::new(
            deployment
                .metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get("deployment.kubernetes.io/revision"))
                .cloned()
                .unwrap_or_else(|| deployment.metadata.generation.unwrap_or(0).to_string()),
        )
        .ok()?,
        pod: PodName::new("deployment").expect("valid name"),
        pod_uid: PodUid::new(uid).ok()?,
        container: ContainerName::new("deployment").expect("valid name"),
    })
}

fn created(metadata: &ObjectMeta, fallback: DateTime<Utc>) -> DateTime<Utc> {
    metadata
        .creation_timestamp
        .as_ref()
        .map(|time| instant(time.0))
        .unwrap_or(fallback)
}

fn signal(
    workload: &Workload,
    detector: DetectorKind,
    severity: Severity,
    signature: &str,
    evidence: Value,
    observed_at: DateTime<Utc>,
) -> Observation {
    let evidence = Evidence::new(evidence).unwrap_or_else(|_| {
        Evidence::new(json!({"detail": "Evidence exceeded the collection limit"}))
            .expect("bounded fallback")
    });
    let event_id = Observation::source_event_id(
        &serde_json::to_vec(&(workload, &detector, &evidence, observed_at))
            .expect("serializable signal"),
    );
    Observation {
        event_id,
        observed_at,
        workload: workload.clone(),
        detector,
        severity,
        signature: ErrorSignature::new(signature.chars().take(256).collect::<String>())
            .unwrap_or_else(|_| ErrorSignature::new("Kubernetes signal").expect("valid signature")),
        evidence,
    }
}

fn instant(time: k8s_openapi::jiff::Timestamp) -> DateTime<Utc> {
    DateTime::from_timestamp(time.as_second(), time.subsec_nanosecond() as u32)
        .expect("Kubernetes timestamp within chrono range")
}
