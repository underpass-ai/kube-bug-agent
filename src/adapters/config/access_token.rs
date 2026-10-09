use anyhow::{Result, ensure};
#[derive(Clone)]
pub struct AccessToken(String);
impl AccessToken {
    pub fn new(value: String) -> Result<Self> {
        ensure!(
            !value.trim().is_empty() && value.len() <= 16_384,
            "invalid access token"
        );
        Ok(Self(value))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
