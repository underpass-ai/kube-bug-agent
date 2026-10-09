use super::super::domain::{
    BackendName, BackpressureController, BackpressurePolicy, ConcurrencyLimit, Latency, Ratio,
    RequestCount, WindowCount,
};
use anyhow::Result;
use std::{path::PathBuf, time::Duration};

#[derive(clap::Args)]
pub struct RunOptions {
    #[arg(long, default_value = "http://127.0.0.1:9901")]
    pub envoy_admin: String,
    #[arg(long, default_value = "orders")]
    pub cluster: String,
    #[arg(long, default_value = "data/backpressure.db")]
    pub database: PathBuf,
    #[arg(long, default_value_t = 1)]
    pub min_concurrency: u32,
    #[arg(long, default_value_t = 16)]
    pub max_concurrency: u32,
    #[arg(long, default_value_t = 200.0)]
    pub latency_ms: f64,
    #[arg(long, default_value_t = 0.1)]
    pub error_ratio: f64,
    #[arg(long, default_value_t = 10)]
    pub min_samples: u64,
    #[arg(long, default_value_t = 3)]
    pub healthy_windows: u32,
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=300))]
    pub cooldown_seconds: u64,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..=60))]
    pub poll_seconds: u64,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub once: bool,
}

impl RunOptions {
    pub fn controller(&self) -> Result<BackpressureController> {
        Ok(BackpressureController::new(
            BackendName::new(&self.cluster)?,
            BackpressurePolicy {
                minimum: ConcurrencyLimit::new(self.min_concurrency)?,
                maximum: ConcurrencyLimit::new(self.max_concurrency)?,
                latency_budget: Latency::milliseconds(self.latency_ms)?,
                error_budget: Ratio::new(self.error_ratio)?,
                minimum_samples: RequestCount::new(self.min_samples),
                healthy_windows: WindowCount::new(self.healthy_windows)?,
                cooldown: Duration::from_secs(self.cooldown_seconds),
            },
        )?)
    }
}
