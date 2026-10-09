use super::KubernetesSnapshot;
use crate::{
    domain::Namespace,
    ports::{ObservationSource, SourceSnapshot},
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use k8s_openapi::api::{
    apps::v1::{Deployment, ReplicaSet},
    core::v1::{Event, Pod},
};
use kube::{Api, Client, api::ListParams};

pub struct KubernetesSource {
    client: Client,
    namespace: Namespace,
    selector: String,
}
impl KubernetesSource {
    pub fn new(client: Client, namespace: Namespace, selector: String) -> Self {
        Self {
            client,
            namespace,
            selector,
        }
    }
}
#[async_trait]
impl ObservationSource for KubernetesSource {
    async fn collect(&self) -> Result<SourceSnapshot> {
        let namespace = self.namespace.as_str();
        let selected = ListParams::default().labels(&self.selector);
        let snapshot = KubernetesSnapshot {
            pods: Api::<Pod>::namespaced(self.client.clone(), namespace)
                .list(&selected)
                .await?
                .items,
            replica_sets: Api::<ReplicaSet>::namespaced(self.client.clone(), namespace)
                .list(&selected)
                .await?
                .items,
            deployments: Api::<Deployment>::namespaced(self.client.clone(), namespace)
                .list(&selected)
                .await?
                .items,
            events: Api::<Event>::namespaced(self.client.clone(), namespace)
                .list(&ListParams::default().fields("type=Warning"))
                .await?
                .items,
        };
        Ok(SourceSnapshot {
            workloads: snapshot.topology().into_values().collect(),
            observations: snapshot.detect(Utc::now(), 60),
        })
    }
}
