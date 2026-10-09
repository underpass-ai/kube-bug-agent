use super::LlmOptions;
use std::{net::SocketAddr, path::PathBuf};
#[derive(clap::Args)]
pub struct CollectorOptions {
    #[arg(long, default_value = "data/incidents.db")]
    pub database: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8787")]
    pub listen: SocketAddr,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(long, default_value = "bug-agent.io/enabled=true")]
    pub selector: String,
    #[arg(long,default_value_t=30,value_parser=clap::value_parser!(u64).range(1..))]
    pub poll_seconds: u64,
    #[command(flatten)]
    pub llm: LlmOptions,
}
