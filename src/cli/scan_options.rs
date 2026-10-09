use std::path::PathBuf;
#[derive(clap::Args)]
pub struct ScanOptions {
    #[arg(long)]
    pub fixture: PathBuf,
    #[arg(long, default_value = "data/incidents.db")]
    pub database: PathBuf,
}
