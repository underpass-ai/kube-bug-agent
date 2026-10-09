use super::super::{
    domain::{BackpressureController, ExecutionStatus, RecordedDecision},
    ports::{ConcurrencyActuator, DecisionRepository, PressureSource},
};
use anyhow::{Result, ensure};
use std::sync::Arc;

pub struct ControlBackpressure {
    controller: BackpressureController,
    source: Arc<dyn PressureSource>,
    actuator: Arc<dyn ConcurrencyActuator>,
    repository: Arc<dyn DecisionRepository>,
    dry_run: bool,
}

impl ControlBackpressure {
    pub fn new(
        controller: BackpressureController,
        source: Arc<dyn PressureSource>,
        actuator: Arc<dyn ConcurrencyActuator>,
        repository: Arc<dyn DecisionRepository>,
        dry_run: bool,
    ) -> Self {
        Self {
            controller,
            source,
            actuator,
            repository,
            dry_run,
        }
    }
    pub async fn tick(&mut self) -> Result<RecordedDecision> {
        let backend = self.controller.backend();
        let current = self.actuator.current(backend).await?;
        let snapshot = self.source.sample(backend).await?;
        let decision = self.controller.observe(snapshot, current)?;
        // Write intent before actuation; an interrupted operation remains Pending.
        self.repository.prepare(&decision).await?;
        let status = if self.dry_run {
            ExecutionStatus::DryRun
        } else if decision.changes_limit() {
            let result = async {
                self.actuator
                    .apply(&decision.backend, decision.before, decision.after)
                    .await?;
                ensure!(
                    self.actuator.current(&decision.backend).await? == decision.after,
                    "actuator read-back mismatch"
                );
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if let Err(error) = result {
                self.repository
                    .finish(&decision.id, ExecutionStatus::Failed)
                    .await?;
                return Err(error);
            }
            self.controller.confirm(&decision);
            ExecutionStatus::Applied
        } else {
            ExecutionStatus::Observed
        };
        self.repository.finish(&decision.id, status).await?;
        Ok(RecordedDecision { decision, status })
    }
}
