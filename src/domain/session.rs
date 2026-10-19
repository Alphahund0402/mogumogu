use crate::clock::{DAY, HOUR, MINUTE, Timestamp};
use serde::{Deserialize, Serialize};

/// A session without any process evidence counts as silent after this long
/// without heartbeat (PROJEKTPLAN §9.2: a missing heartbeat triggers review).
pub const SESSION_SILENT_AFTER: i64 = HOUR;
/// A reported completion that still cannot be verified needs a person.
pub const SESSION_UNVERIFIED_AFTER: i64 = 15 * MINUTE;
/// Still running after a day: worth a look, never ended automatically.
pub const SESSION_LONG_RUNNING: i64 = DAY;

string_enum! {
    /// Why an open session waits for a person. Its lock always stays; this
    /// only turns a silent wait into a visible hint.
    pub enum Stall {
        /// Interrupted without report; resolved by a reviewed release.
        Unclear => "unclear",
        /// No job, no process and no recent heartbeat to check.
        Silent => "silent",
        /// Completion reported, but processes or evidence keep it open.
        Unverified => "unverified",
        /// Still running after a day.
        LongRunning => "long_running",
    }
}

impl Stall {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unclear => "Unklar unterbrochen",
            Self::Silent => "Kein Lebenszeichen",
            Self::Unverified => "Abschluss ungeprüft",
            Self::LongRunning => "Läuft sehr lange",
        }
    }
}

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

impl Session {
    /// Whether this open session got stuck at `now`. Pure classification of
    /// stored facts; process evidence is checked again before any release.
    pub fn stall(&self, now: Timestamp) -> Option<Stall> {
        use SessionState::*;
        let traceable = self.job_name.is_some() || self.process.is_some();
        match self.state {
            InterruptedUnknown => Some(Stall::Unclear),
            CompletionRequested if now - self.ended_at.unwrap_or(self.started_at) > SESSION_UNVERIFIED_AFTER => {
                Some(Stall::Unverified)
            }
            Starting | Running
                if !traceable && now - self.heartbeat_at.unwrap_or(self.started_at) > SESSION_SILENT_AFTER =>
            {
                Some(Stall::Silent)
            }
            Starting | Running if now - self.started_at > SESSION_LONG_RUNNING => Some(Stall::LongRunning),
            _ => None,
        }
    }

    /// Seconds since the last sign of life the session itself recorded.
    pub fn quiet_for(&self, now: Timestamp) -> i64 {
        now - self.heartbeat_at.or(self.ended_at).unwrap_or(self.started_at).max(self.started_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SessionState::*;

    fn session(state: SessionState, job: bool) -> Session {
        Session {
            id: 1,
            scratch_id: 1,
            purpose: String::new(),
            state,
            origin: SessionOrigin::Cooperative,
            process: None,
            job_name: job.then(|| "job".into()),
            started_at: 0,
            heartbeat_at: None,
            ended_at: None,
            exit_code: None,
            remaining_processes: None,
            note: None,
        }
    }

    #[test]
    fn stuck_sessions_are_named_but_keep_their_lock() {
        assert_eq!(session(InterruptedUnknown, true).stall(0), Some(Stall::Unclear));
        let silent = session(Running, false);
        assert_eq!(silent.stall(SESSION_SILENT_AFTER), None);
        assert_eq!(silent.stall(SESSION_SILENT_AFTER + 1), Some(Stall::Silent));
        let beating = Session { heartbeat_at: Some(SESSION_SILENT_AFTER), ..silent.clone() };
        assert_eq!(beating.stall(SESSION_SILENT_AFTER + 1), None, "a heartbeat is a sign of life");
        let managed = session(Running, true);
        assert_eq!(managed.stall(SESSION_LONG_RUNNING), None, "the job is checked instead");
        assert_eq!(managed.stall(SESSION_LONG_RUNNING + 1), Some(Stall::LongRunning));
        let reported = Session { ended_at: Some(100), ..session(CompletionRequested, true) };
        assert_eq!(reported.stall(100 + SESSION_UNVERIFIED_AFTER + 1), Some(Stall::Unverified));
        assert_eq!(session(CompletedVerified, true).stall(10 * DAY), None);
        for state in [Starting, Running, CompletionRequested, InterruptedUnknown] {
            assert!(state.locks_resources(), "a stuck {state} session still locks");
        }
    }

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
