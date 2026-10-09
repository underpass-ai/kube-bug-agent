use super::{
    AnalysisAttempts, AnalysisFailure, AnalysisState, Diagnosis, DomainError, IncidentId,
    Observation, OccurrenceCount,
};
use chrono::{DateTime, Duration, Utc};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Incident {
    id: IncidentId,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
    occurrences: OccurrenceCount,
    observation: Observation,
    analysis: AnalysisState,
}
impl Incident {
    pub fn new(observation: Observation, now: DateTime<Utc>) -> Self {
        Self {
            id: observation.fingerprint(),
            first_seen: observation.observed_at,
            last_seen: observation.observed_at,
            occurrences: OccurrenceCount::one(),
            observation,
            analysis: AnalysisState::Pending {
                attempts: AnalysisAttempts::default(),
                next_attempt: now,
                last_error: None,
            },
        }
    }
    pub fn id(&self) -> &IncidentId {
        &self.id
    }
    pub fn observation(&self) -> &Observation {
        &self.observation
    }
    pub fn occurrences(&self) -> OccurrenceCount {
        self.occurrences
    }
    pub fn first_seen(&self) -> DateTime<Utc> {
        self.first_seen
    }
    pub fn last_seen(&self) -> DateTime<Utc> {
        self.last_seen
    }
    pub fn analysis(&self) -> &AnalysisState {
        &self.analysis
    }
    pub fn register(&mut self, observation: &Observation) -> Result<(), DomainError> {
        if observation.fingerprint() != self.id {
            return Err(DomainError::WrongIncident);
        }
        self.occurrences.increment()?;
        self.first_seen = self.first_seen.min(observation.observed_at);
        self.last_seen = self.last_seen.max(observation.observed_at);
        Ok(())
    }
    pub fn claim_analysis(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        if !self.analysis.pending_at(now) {
            return Err(DomainError::InvalidTransition);
        }
        self.analysis = AnalysisState::Processing {
            attempts: self.analysis.attempts().increment()?,
        };
        Ok(())
    }
    pub fn complete_analysis(&mut self, diagnosis: Diagnosis) -> Result<(), DomainError> {
        if !matches!(self.analysis, AnalysisState::Processing { .. }) {
            return Err(DomainError::InvalidTransition);
        }
        diagnosis.validate_for(&self.observation.event_id)?;
        self.analysis = AnalysisState::Complete {
            attempts: self.analysis.attempts(),
            diagnosis,
        };
        Ok(())
    }
    pub fn fail_analysis(
        &mut self,
        error: AnalysisFailure,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        if !matches!(self.analysis, AnalysisState::Processing { .. }) {
            return Err(DomainError::InvalidTransition);
        }
        let attempts = self.analysis.attempts();
        self.analysis = if attempts.value() >= 3 {
            AnalysisState::Failed { attempts, error }
        } else {
            AnalysisState::Pending {
                attempts,
                next_attempt: now + Duration::seconds(10 * 2_i64.pow(attempts.value().into())),
                last_error: Some(error),
            }
        };
        Ok(())
    }
    pub fn recover_analysis(&mut self, now: DateTime<Utc>) {
        if let AnalysisState::Processing { attempts } = self.analysis {
            self.analysis = if attempts.value() >= 3 {
                AnalysisState::Failed {
                    attempts,
                    error: AnalysisFailure::new("Analysis interrupted on final attempt")
                        .expect("valid failure"),
                }
            } else {
                AnalysisState::Pending {
                    attempts,
                    next_attempt: now,
                    last_error: None,
                }
            };
        }
    }
}
