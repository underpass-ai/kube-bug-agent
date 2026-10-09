use crate::{
    adapters::{
        config::{AccessToken, ApiEndpoint},
        wire::ObservationDto,
    },
    domain::{IngestReceipt, Observation},
    ports::ObservationSink,
};
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use reqwest::{Client, Url};
use std::time::Duration;
pub struct HttpObservationSink {
    client: Client,
    endpoint: Url,
    token: Option<AccessToken>,
}
impl HttpObservationSink {
    pub fn new(endpoint: ApiEndpoint, token: Option<AccessToken>) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            endpoint: endpoint.join("v1/observations")?,
            token,
        })
    }
}
#[async_trait]
impl ObservationSink for HttpObservationSink {
    async fn deliver(&self, observation: &Observation) -> Result<IngestReceipt> {
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .json(&ObservationDto::from(observation));

        if let Some(token) = &self.token {
            request = request.bearer_auth(token.expose());
        }
        let response = request.send().await.context("collector unavailable")?;
        ensure!(
            response.status().is_success(),
            "collector HTTP {}",
            response.status()
        );
        Ok(response
            .json()
            .await
            .context("invalid collector acknowledgement")?)
    }
}
