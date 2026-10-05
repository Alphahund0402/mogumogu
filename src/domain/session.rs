use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    /// Session lifecycle (PROJEKTPLAN §9.2). A finished starter, a missing
    /// heartbeat or a crash never counts as proof of completion.
    pub enum SessionState {
        Starting => "starting",
        Running => "running",
        CompletionRequested => "completion_requested",
        CompletedVerified => "completed_verified",
        InterruptedUnknown => "interrupted_unknown",
        ReleasedAfterReview => "released_after_review",
    }
}

impl SessionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Starting => "Startet",
            Self::Running => "Läuft",
            Self::CompletionRequested => "Abschluss gemeldet, ungeprüft",
            Self::CompletedVerified => "Abschluss geprüft",
            Self::InterruptedUnknown => "Unklar unterbrochen",
            Self::ReleasedAfterReview => "Nach Prüfung freigegeben",
        }
    }

    /// Whether the session still locks its resources against cleanup.
    pub fn locks_resources(self) -> bool {
        !matches!(self, Self::CompletedVerified | Self::ReleasedAfterReview)
    }

    pub fn can_become(self, next: Self) -> bool {
        use SessionState::*;
        matches!(
            (self, next),
            (Starting, Running | InterruptedUnknown)
                | (Running, CompletionRequested | InterruptedUnknown)
                | (CompletionRequested, CompletedVerified | InterruptedUnknown)
                | (InterruptedUnknown, ReleasedAfterReview | Running)
        )
    }
}

string_enum! {
    /// Evidence strength differs: managed starts are strongest.
    pub enum SessionOrigin {
        Managed => "managed",
        Registered => "registered",
        Cooperative => "cooperative",
    }
}

/// Process identity: a PID alone is not an identity because PIDs are reused.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Creation time as FILETIME ticks.
    pub created: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub id: i64,
    pub scratch_id: i64,
    pub purpose: String,
    pub state: SessionState,
    pub origin: SessionOrigin,
    pub process: Option<ProcessIdentity>,
    pub job_name: Option<String>,
    pub started_at: Timestamp,
    pub heartbeat_at: Option<Timestamp>,
    pub ended_at: Option<Timestamp>,
    pub exit_code: Option<i64>,
    pub remaining_processes: Option<i64>,
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::SessionState::*;

    #[test]
    fn unclear_sessions_keep_locks() {
        for state in [Starting, Running, CompletionRequested, InterruptedUnknown] {
            assert!(state.locks_resources(), "{state}");
        }
        assert!(!CompletedVerified.locks_resources());
    }

    #[test]
    fn no_shortcut_from_running_to_verified() {
        assert!(!Running.can_become(CompletedVerified));
        assert!(!InterruptedUnknown.can_become(CompletedVerified));
        assert!(CompletionRequested.can_become(CompletedVerified));
        assert!(!CompletedVerified.can_become(Running));
    }
}
