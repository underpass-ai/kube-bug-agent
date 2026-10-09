use super::super::{
    domain::{BackendName, ConcurrencyLimit, TrafficSnapshot},
    ports::{ConcurrencyActuator, PressureSource},
};
use super::{EnvoyAdminEndpoint, EnvoyStatsDto};
use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use reqwest::{Client, Response};
use serde_json::Value;
use std::time::Duration;

pub struct EnvoyAdmin {
    endpoint: EnvoyAdminEndpoint,
    client: Client,
}

impl EnvoyAdmin {
    pub fn new(endpoint: EnvoyAdminEndpoint) -> Result<Self> {
        Ok(Self {
            endpoint,
            client: Client::builder()
                .timeout(Duration::from_secs(2))
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()?,
        })
    }
    fn key(backend: &BackendName) -> String {
        format!("circuit_breakers.{}.default.max_requests", backend.as_str())
    }
    async fn json(mut response: Response) -> Result<Value> {
        response.error_for_status_ref()?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                body.len() + chunk.len() <= 1_048_576,
                "admin response too large"
            );
            body.extend_from_slice(&chunk);
        }
        Ok(serde_json::from_slice(&body)?)
    }
}

#[async_trait]
impl PressureSource for EnvoyAdmin {
    async fn sample(&self, backend: &BackendName) -> Result<TrafficSnapshot> {
        let mut url = self.endpoint.route("stats")?;
        let filter = format!(
            "^(server\\.uptime|cluster\\.{}\\.(upstream_rq_(completed|5xx|timeout|pending_overflow|active|pending_active|time)))$",
            backend.as_str()
        );
        url.query_pairs_mut()
            .append_pair("format", "json")
            .append_pair("filter", &filter);
        let value = Self::json(self.client.get(url).send().await?).await?;
        serde_json::from_value::<EnvoyStatsDto>(value)?.into_snapshot(backend)
    }
}

#[async_trait]
impl ConcurrencyActuator for EnvoyAdmin {
    async fn current(&self, backend: &BackendName) -> Result<ConcurrencyLimit> {
        let value = Self::json(
            self.client
                .get(self.endpoint.route("runtime")?)
                .send()
                .await?,
        )
        .await?;
        let limit = value
            .get("entries")
            .and_then(|entries| entries.get(Self::key(backend)))
            .and_then(|entry| entry.get("final_value"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("explicit cluster max_requests runtime key is required"))?;
        Ok(ConcurrencyLimit::new(limit.parse()?)?)
    }
    async fn apply(
        &self,
        backend: &BackendName,
        expected: ConcurrencyLimit,
        desired: ConcurrencyLimit,
    ) -> Result<()> {
        ensure!(
            self.current(backend).await? == expected,
            "runtime limit changed externally"
        );
        let mut url = self.endpoint.route("runtime_modify")?;
        url.query_pairs_mut()
            .append_pair(&Self::key(backend), &desired.value().to_string());
        self.client.post(url).send().await?.error_for_status()?;
        Ok(())
    }
}
