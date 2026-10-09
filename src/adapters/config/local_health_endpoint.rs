use anyhow::{Result, ensure};
use reqwest::Url;
#[derive(Clone)]
pub struct LocalHealthEndpoint(Url);
impl LocalHealthEndpoint {
    pub fn new(value: &str) -> Result<Self> {
        let url = Url::parse(value)?;
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
            "health endpoint must address localhost"
        );
        ensure!(
            url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
            "invalid health endpoint"
        );
        Ok(Self(url))
    }
    pub fn url(&self) -> &Url {
        &self.0
    }
}
