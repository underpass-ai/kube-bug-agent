#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
    Fatal,
    Panic,
    Critical,
    Unknown,
}
impl LogLevel {
    pub fn is_error(self) -> bool {
        matches!(
            self,
            Self::Error | Self::Fatal | Self::Panic | Self::Critical
        )
    }

    pub fn is_critical(self) -> bool {
        matches!(self, Self::Fatal | Self::Panic | Self::Critical)
    }
}
