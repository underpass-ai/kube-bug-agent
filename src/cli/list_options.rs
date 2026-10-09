use std::path::PathBuf;
#[derive(clap::Args)]
pub struct ListOptions {
    #[arg(long, default_value = "data/incidents.db")]
    pub database: PathBuf,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(long)]
    pub deployment: Option<String>,
}
