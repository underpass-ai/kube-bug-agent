use super::DiagnosisDto;
use crate::domain::AnalysisState;
use chrono::{DateTime, Utc};
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum AnalysisDto {
    Pending {
        attempts: u8,
        next_attempt: DateTime<Utc>,
        last_error: Option<String>,
    },
    Processing {
        attempts: u8,
    },
    Complete {
        attempts: u8,
        diagnosis: DiagnosisDto,
    },
    Failed {
        attempts: u8,
        error: String,
    },
}
impl From<&AnalysisState> for AnalysisDto {
    fn from(value: &AnalysisState) -> Self {
        match value {
            AnalysisState::Pending {
                attempts,
                next_attempt,
                last_error,
            } => Self::Pending {
                attempts: attempts.value(),
                next_attempt: *next_attempt,
                last_error: last_error.as_ref().map(|value| value.as_str().into()),
            },
            AnalysisState::Processing { attempts } => Self::Processing {
                attempts: attempts.value(),
            },
            AnalysisState::Complete {
                attempts,
                diagnosis,
            } => Self::Complete {
                attempts: attempts.value(),
                diagnosis: diagnosis.into(),
            },
            AnalysisState::Failed { attempts, error } => Self::Failed {
                attempts: attempts.value(),
                error: error.as_str().into(),
            },
        }
    }
}
