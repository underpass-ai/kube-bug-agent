use super::{HistoryOptions, RunOptions};

#[derive(clap::Subcommand)]
pub enum BackpressureCommand {
    Run(RunOptions),
    History(HistoryOptions),
}
