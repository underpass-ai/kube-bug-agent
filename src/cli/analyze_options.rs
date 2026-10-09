use super::LlmOptions;
use std::path::PathBuf;
#[derive(clap::Args)]
pub struct AnalyzeOptions {
    #[arg(long, default_value = "data/incidents.db")]
    pub database: PathBuf,
    #[arg(long,default_value_t=10,value_parser=clap::value_parser!(u32).range(1..=500))]
    pub limit: u32,
    #[command(flatten)]
    pub llm: LlmOptions,
}
