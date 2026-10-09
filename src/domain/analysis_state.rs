use super::{AnalysisAttempts, AnalysisFailure, Diagnosis};
use chrono::{DateTime, Utc};
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum AnalysisState {
    Pending {
        attempts: AnalysisAttempts,
        next_attempt: DateTime<Utc>,
        last_error: Option<AnalysisFailure>,
    },
    Processing {
        attempts: AnalysisAttempts,
    },
    Complete {
        attempts: AnalysisAttempts,
        diagnosis: Diagnosis,
    },
    Failed {
        attempts: AnalysisAttempts,
        error: AnalysisFailure,
    },
}
impl AnalysisState {
    pub fn pending_at(&self, now: DateTime<Utc>) -> bool {
        matches!(self, Self::Pending { next_attempt, .. } if *next_attempt <= now)
    }
    pub fn attempts(&self) -> AnalysisAttempts {
        match self {
            Self::Pending { attempts, .. }
            | Self::Processing { attempts }
            | Self::Complete { attempts, .. }
            | Self::Failed { attempts, .. } => *attempts,
        }
    }
}
