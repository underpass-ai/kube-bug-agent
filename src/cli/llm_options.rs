use crate::{
    adapters::{
        config::{AccessToken, ApiEndpoint, ModelId},
        openai::OpenAiDiagnosisProvider,
    },
    ports::DiagnosisProvider,
};
use anyhow::Result;
use std::sync::Arc;
#[derive(clap::Args)]
pub struct LlmOptions {
    #[arg(long, env = "LLM_BASE_URL")]
    pub llm_base_url: Option<String>,
    #[arg(long, env = "LLM_MODEL")]
    pub llm_model: Option<String>,
    #[arg(long)]
    pub disable_thinking: bool,
}
impl LlmOptions {
    pub fn provider(&self, default_local: bool) -> Result<Option<Arc<dyn DiagnosisProvider>>> {
        let endpoint = match self.llm_base_url.as_deref() {
            Some(url) => url,
            None if default_local => "http://127.0.0.1:8080/v1",
            None => return Ok(None),
        };
        let token = std::env::var("LLM_API_KEY")
            .ok()
            .map(AccessToken::new)
            .transpose()?;
        Ok(Some(Arc::new(OpenAiDiagnosisProvider::new(
            ApiEndpoint::new(endpoint)?,
            self.llm_model.clone().map(ModelId::new).transpose()?,
            token,
            self.disable_thinking,
        )?)))
    }
}
