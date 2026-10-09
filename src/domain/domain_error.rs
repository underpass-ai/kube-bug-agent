#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("invalid value for {0}")]
    InvalidValue(&'static str),
    #[error("observation belongs to a different incident")]
    WrongIncident,
    #[error("conflicting payload for an existing event ID")]
    EventConflict,
    #[error("invalid analysis state transition")]
    InvalidTransition,
    #[error("diagnosis cites an unknown observation")]
    UnknownEvidence,
}
