use crate::domain::{
    ApplicationLog, DetectorKind, EventId, HttpStatus, Observation, Severity, Workload,
};

pub struct LogDetector;
impl LogDetector {
    pub fn detect(
        log: ApplicationLog,
        event_id: EventId,
        workload: &Workload,
    ) -> Option<Observation> {
        let detector = if log.level.is_error() {
            DetectorKind::ApplicationError
        } else if log.status.is_some_and(HttpStatus::is_server_error) {
            DetectorKind::Http5xx
        } else {
            return None;
        };
        Some(Observation {
            event_id,
            observed_at: log.observed_at,
            workload: workload.clone(),
            detector,
            severity: if log.level.is_critical() {
                Severity::Critical
            } else {
                Severity::Error
            },
            signature: log.signature,
            evidence: log.evidence,
        })
    }
}
