use super::Command;
use crate::{
    adapters::{
        config::{AccessToken, ApiEndpoint},
        http::{CollectorServer, CollectorState, HttpObservationSink},
        kubernetes::{FixtureSource, KubernetesSource},
        sidecar::SidecarRuntime,
        sqlite::SqliteIncidentRepository,
    },
    application::{AnalyzeIncident, CollectIncidents, IncidentQuery},
    domain::{DeploymentName, Namespace},
    ports::IncidentRepository,
};
use anyhow::Result;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};

#[derive(clap::Parser)]
#[command(
    version,
    about = "Kubernetes incident detector with local or OpenAI-compatible LLM diagnosis"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
impl Cli {
    pub async fn execute(self) -> Result<Value> {
        match self.command {
            Command::Collector(options) => {
                let repository: Arc<dyn IncidentRepository> =
                    Arc::new(SqliteIncidentRepository::open(&options.database)?);
                let token = std::env::var("AGENT_TOKEN")
                    .ok()
                    .map(AccessToken::new)
                    .transpose()?;
                let provider = options.llm.provider(false)?;
                let mut tasks = Vec::new();
                let topology = if let Some(namespace) = options.namespace {
                    let source = Arc::new(KubernetesSource::new(
                        kube::Client::try_default().await?,
                        Namespace::new(namespace)?,
                        options.selector,
                    ));
                    let collect = CollectIncidents::new(repository.clone(), source);
                    let topology = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
                    let target = topology.clone();
                    tasks.push(tokio::spawn(async move {
                        let mut interval =
                            tokio::time::interval(Duration::from_secs(options.poll_seconds));
                        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        loop {
                            interval.tick().await;
                            match collect.execute().await {
                                Ok(snapshot) => {
                                    *target.write().await = snapshot
                                        .workloads
                                        .into_iter()
                                        .map(|workload| (workload.pod_uid.clone(), workload))
                                        .collect();
                                }
                                Err(_) => tracing::warn!(
                                    "Kubernetes collection failed; retrying on next poll"
                                ),
                            }
                        }
                    }));
                    Some(topology)
                } else {
                    None
                };
                if let Some(provider) = provider {
                    let analyze = AnalyzeIncident::new(repository.clone(), provider);
                    tasks.push(tokio::spawn(async move {
                        loop {
                            if analyze.execute_next().await.is_err() {
                                tracing::warn!("analysis persistence failed");
                            }
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                    }));
                }
                let result = CollectorServer::serve(
                    options.listen,
                    CollectorState {
                        repository,
                        token,
                        topology,
                    },
                )
                .await;
                for task in tasks {
                    task.abort();
                    let _ = task.await;
                }
                result?;
                Ok(json!({"status":"stopped"}))
            }
            Command::Sidecar(options) => {
                let token = std::env::var("AGENT_TOKEN")
                    .ok()
                    .map(AccessToken::new)
                    .transpose()?;
                let sink = Arc::new(HttpObservationSink::new(
                    ApiEndpoint::new(&options.collector_url)?,
                    token,
                )?);
                let mut runtime = SidecarRuntime::new(options.config()?, sink)?;
                if options.once {
                    runtime.run_once().await?;
                } else {
                    runtime.run().await?;
                }
                Ok(json!({"status":"delivered"}))
            }
            Command::Scan(options) => {
                let repository: Arc<dyn IncidentRepository> =
                    Arc::new(SqliteIncidentRepository::open(&options.database)?);
                let source = Arc::new(FixtureSource::new(options.fixture));
                let snapshot = CollectIncidents::new(repository, source).execute().await?;
                Ok(
                    json!({"observations":snapshot.observations.len(),"pods":snapshot.workloads.len()}),
                )
            }
            Command::Analyze(options) => {
                let repository: Arc<dyn IncidentRepository> =
                    Arc::new(SqliteIncidentRepository::open(&options.database)?);
                let provider = options.llm.provider(true)?.expect("local provider default");
                let analyze = AnalyzeIncident::new(repository.clone(), provider);
                for _ in 0..options.limit {
                    if !analyze.execute_next().await? {
                        break;
                    }
                }
                Ok(serde_json::to_value(
                    repository.list(IncidentQuery::default()).await?,
                )?)
            }
            Command::Incidents(options) => {
                let repository = SqliteIncidentRepository::open(&options.database)?;
                let query = IncidentQuery {
                    namespace: options.namespace.map(Namespace::new).transpose()?,
                    deployment: options.deployment.map(DeploymentName::new).transpose()?,
                    ..Default::default()
                };
                Ok(serde_json::to_value(repository.list(query).await?)?)
            }
        }
    }
}
