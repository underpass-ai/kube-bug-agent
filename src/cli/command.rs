use super::{AnalyzeOptions, CollectorOptions, ListOptions, ScanOptions, SidecarOptions};
#[derive(clap::Subcommand)]
pub enum Command {
    Collector(CollectorOptions),
    Sidecar(SidecarOptions),
    Scan(ScanOptions),
    Analyze(AnalyzeOptions),
    Incidents(ListOptions),
}
