use super::super::domain::{BackendName, Latency, RequestCount, TrafficSnapshot};
use anyhow::{Result, anyhow, ensure};
use chrono::Utc;
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

#[derive(serde::Deserialize)]
pub struct EnvoyStatsDto {
    pub stats: Vec<Value>,
}

impl EnvoyStatsDto {
    pub fn into_snapshot(self, backend: &BackendName) -> Result<TrafficSnapshot> {
        let mut counters = BTreeMap::new();
        for entry in &self.stats {
            if let Some(name) = entry.get("name").and_then(Value::as_str) {
                let value = entry
                    .get("value")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| anyhow!("invalid Envoy counter"))?;
                ensure!(counters.insert(name, value).is_none(), "duplicate counter");
            }
        }
        let prefix = format!("cluster.{}.", backend.as_str());
        let required = |name: &str| {
            counters
                .get(format!("{prefix}{name}").as_str())
                .copied()
                .ok_or_else(|| anyhow!("missing cluster metric: {name}"))
        };
        let count = |name: &str| {
            RequestCount::new(
                counters
                    .get(format!("{prefix}{name}").as_str())
                    .copied()
                    .unwrap_or_default(),
            )
        };
        let uptime = *counters
            .get("server.uptime")
            .ok_or_else(|| anyhow!("missing Envoy uptime"))?;
        let p95 = self
            .stats
            .iter()
            .filter_map(|entry| entry.get("histograms"))
            .find_map(|histograms| {
                let quantiles = histograms.get("supported_quantiles")?.as_array()?;
                let index = quantiles
                    .iter()
                    .position(|value| value.as_f64() == Some(95.0))?;
                histograms
                    .get("computed_quantiles")?
                    .as_array()?
                    .iter()
                    .find(|histogram| {
                        histogram.get("name").and_then(Value::as_str)
                            == Some(format!("{prefix}upstream_rq_time").as_str())
                    })?
                    .get("values")?
                    .get(index)?
                    .get("interval")?
                    .as_f64()
            })
            .map(Latency::milliseconds)
            .transpose()?;
        Ok(TrafficSnapshot {
            observed_at: Utc::now(),
            uptime: Duration::from_secs(uptime),
            completed: count("upstream_rq_completed"),
            failed: count("upstream_rq_5xx"),
            timed_out: count("upstream_rq_timeout"),
            rejected: count("upstream_rq_pending_overflow"),
            active: RequestCount::new(required("upstream_rq_active")?),
            pending: RequestCount::new(required("upstream_rq_pending_active")?),
            p95,
        })
    }
}
