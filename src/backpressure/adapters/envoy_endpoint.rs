use anyhow::{Result, ensure};
use reqwest::Url;
use std::net::IpAddr;

#[derive(Clone, Debug)]
pub struct EnvoyAdminEndpoint(Url);

impl EnvoyAdminEndpoint {
    pub fn new(input: &str) -> Result<Self> {
        let url = Url::parse(input)?;
        let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
        ensure!(
            url.scheme() == "http"
                && host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
                && url.username().is_empty()
                && url.password().is_none()
                && matches!(url.path(), "" | "/")
                && url.query().is_none()
                && url.fragment().is_none(),
            "Envoy admin must be an HTTP loopback IP without credentials, path or query"
        );
        Ok(Self(url))
    }
    pub fn route(&self, path: &str) -> Result<Url> {
        Ok(self.0.join(path)?)
    }
}
