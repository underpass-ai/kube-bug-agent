use super::{AnalysisDto, ObservationDto};
use crate::domain::Incident;
use chrono::{DateTime, Utc};
#[derive(serde::Serialize)]
pub struct IncidentDto {
    id: String,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
    occurrences: u64,
    observation: ObservationDto,
    analysis: AnalysisDto,
}
impl From<&Incident> for IncidentDto {
    fn from(value: &Incident) -> Self {
        Self {
            id: value.id().as_str().into(),
            first_seen: value.first_seen(),
            last_seen: value.last_seen(),
            occurrences: value.occurrences().value(),
            observation: value.observation().into(),
            analysis: value.analysis().into(),
        }
    }
}
