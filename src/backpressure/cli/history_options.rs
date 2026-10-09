use std::path::PathBuf;

#[derive(clap::Args)]
pub struct HistoryOptions {
    #[arg(long, default_value = "data/backpressure.db")]
    pub database: PathBuf,
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=200))]
    pub limit: u32,
}
