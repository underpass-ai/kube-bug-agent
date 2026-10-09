use super::{FileLogReader, SidecarConfig, SqliteOutbox};
use crate::{
    adapters::runtime::ShutdownSignal,
    application::FlushOutbox,
    domain::{DetectorKind, ErrorSignature, EventId, Evidence, Observation, Severity},
    ports::ObservationSink,
};
use anyhow::Result;
use reqwest::Client;
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub struct SidecarRuntime {
    config: SidecarConfig,
    outbox: Arc<SqliteOutbox>,
    flush: FlushOutbox,
    probe: Client,
    failures: u32,
    reported: bool,
}
impl SidecarRuntime {
    pub fn new(config: SidecarConfig, sink: Arc<dyn ObservationSink>) -> Result<Self> {
        let outbox = Arc::new(SqliteOutbox::open(&config.spool, config.capacity)?);
        Ok(Self {
            flush: FlushOutbox::new(outbox.clone(), sink),
            outbox,
            config,
            probe: Client::builder()
                .timeout(Duration::from_secs(2))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            failures: 0,
            reported: false,
        })
    }
    pub async fn run_once(&self) -> Result<()> {
        loop {
            self.flush.execute().await?;
            if FileLogReader::read(&self.config, &self.outbox, true).await? == 0 {
                break;
            }
        }
        while self.flush.execute().await? > 0 {}
        Ok(())
    }
    pub async fn tick(&mut self, check_probe: bool) -> Result<()> {
        FileLogReader::read(&self.config, &self.outbox, false).await?;
        if check_probe && let Some(endpoint) = &self.config.health {
            let healthy = self
                .probe
                .get(endpoint.url().clone())
                .send()
                .await
                .is_ok_and(|response| response.status().is_success());

            if healthy {
                self.failures = 0;
                self.reported = false;
            } else {
                self.failures = self.failures.saturating_add(1);
            }
            if self.failures >= 3 && !self.reported {
                let observation = Observation {
                    event_id: EventId::new(uuid::Uuid::new_v4().to_string())?,
                    observed_at: chrono::Utc::now(),
                    workload: self.config.workload.clone(),
                    detector: DetectorKind::LocalProbeFailed,
                    severity: Severity::Warning,
                    signature: ErrorSignature::new("Local HTTP health check failed repeatedly")?,
                    evidence: Evidence::new(json!({"consecutive_failures":self.failures}))?,
                };
                self.reported = self.outbox.enqueue(observation)?;
            }
        }
        self.flush.execute().await?;
        Ok(())
    }
    pub async fn run(&mut self) -> Result<()> {
        let mut shutdown = ShutdownSignal::new()?;
        let startup = Instant::now();
        let mut last_probe = Instant::now();
        let mut backoff = 1_u64;
        loop {
            let check = startup.elapsed() >= Duration::from_secs(60)
                && last_probe.elapsed() >= Duration::from_secs(10);
            if check {
                last_probe = Instant::now();
            }
            tokio::select! {
                _ = shutdown.wait() => break,
                result = self.tick(check) => {
                    match result {
                        Ok(()) => backoff = 1,
                        Err(_) => {
                            tracing::warn!(
                                "sidecar iteration failed; pending observations remain in SQLite"
                            );
                            backoff = (backoff * 2).min(60);
                        }
                    }
                }
            }
            tokio::select! {
                _=shutdown.wait()=>break,
                _=tokio::time::sleep(Duration::from_secs(backoff))=>{}
            }
        }
        let _ = tokio::time::timeout(Duration::from_secs(3), self.flush.execute()).await;
        Ok(())
    }
}
