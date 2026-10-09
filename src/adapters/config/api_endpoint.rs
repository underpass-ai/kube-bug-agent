use anyhow::{Result, ensure};
use reqwest::Url;
#[derive(Clone)]
pub struct ApiEndpoint(Url);
impl ApiEndpoint {
    pub fn new(input: &str) -> Result<Self> {
        let mut url = Url::parse(input)?;
        ensure!(
            matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
            "endpoint must use HTTP(S)"
        );
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "endpoint must not contain credentials, query or fragment"
        );
        url.set_path(&format!("{}/", url.path().trim_end_matches('/')));
        Ok(Self(url))
    }
    pub fn join(&self, path: &str) -> Result<Url> {
        Ok(self.0.join(path)?)
    }
    pub fn is_loopback(&self) -> bool {
        matches!(self.0.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
    }
}
