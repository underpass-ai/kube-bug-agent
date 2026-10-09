use super::super::{
    adapters::{EnvoyAdmin, EnvoyAdminEndpoint, SqliteDecisionRepository},
    application::ControlBackpressure,
    ports::DecisionRepository,
};
use super::BackpressureCommand;
use crate::{adapters::runtime::ShutdownSignal, application::QueryLimit};
use anyhow::Result;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

#[derive(clap::Parser)]
#[command(
    name = "backpressure-agent",
    about = "Independent Envoy backpressure controller"
)]
pub struct BackpressureCli {
    #[command(subcommand)]
    pub command: BackpressureCommand,
}

impl BackpressureCli {
    pub async fn execute(self) -> Result<Value> {
        match self.command {
            BackpressureCommand::History(options) => {
                let repository = SqliteDecisionRepository::open(&options.database)?;
                Ok(serde_json::to_value(
                    repository.recent(QueryLimit::new(options.limit)?).await?,
                )?)
            }
            BackpressureCommand::Run(options) => {
                let controller = options.controller()?;
                let envoy = Arc::new(EnvoyAdmin::new(EnvoyAdminEndpoint::new(
                    &options.envoy_admin,
                )?)?);
                let repository = Arc::new(SqliteDecisionRepository::open(&options.database)?);
                let mut control = ControlBackpressure::new(
                    controller,
                    envoy.clone(),
                    envoy,
                    repository,
                    options.dry_run,
                );
                if options.once {
                    return Ok(serde_json::to_value(control.tick().await?)?);
                }
                let mut shutdown = ShutdownSignal::new()?;
                loop {
                    tokio::select! {
                        _ = shutdown.wait() => break,
                        result = control.tick() => {
                            match result {
                                Ok(record) => tracing::info!(
                                    backend=%record.decision.backend,
                                    before=record.decision.before.value(),
                                    after=record.decision.after.value(),
                                    reason=?record.decision.reason,
                                    status=?record.status,
                                    "backpressure decision"
                                ),
                                Err(error) => tracing::warn!(%error, "control failed; check runtime and pending audit before further intervention"),
                            }
                        }
                    }
                    tokio::select! {
                        _ = shutdown.wait() => break,
                        _ = tokio::time::sleep(Duration::from_secs(options.poll_seconds)) => {},
                    }
                }
                Ok(json!({"status":"stopped","limits":"retained"}))
            }
        }
    }
}
