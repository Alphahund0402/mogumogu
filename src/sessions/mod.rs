//! Scratchpads, managed runs and session reconciliation (F-14 – F-16).
//!
//! The owner process records sessions; a managed run is started by the CLI
//! inside a named job object so that all descendants are attributable. A
//! finished starter, a missing heartbeat or a lost connection never counts
//! as completion: only "completion reported" plus "no process left in the
//! job" yields `completed_verified`.
mod run;
mod scratch;

pub use run::{RunReport, SessionControl, StartInfo, run_managed};
pub use scratch::{LAYOUT, create_layout, folder_name};

use crate::clock::Timestamp;
use crate::domain::{ProcessIdentity, Session, SessionOrigin, SessionState};
use crate::platform::{self, Job};

/// What reconciliation decided for one open session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub next: SessionState,
    pub note: String,
}

/// Observable facts about a session's processes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessEvidence {
    /// Job found; number of processes still in it.
    Job(u32),
    /// Named job no longer exists: no process and no handle remain.
    JobGone,
    /// Single tracked process (cooperative sessions).
    Process { alive: bool },
    /// Nothing to check.
    None,
}

/// A starter has this long to create its job and attach after `start`.
pub const ATTACH_GRACE: i64 = 120;

/// Pure decision table for open sessions.
pub fn decide(session: &Session, evidence: ProcessEvidence, now: Timestamp) -> Option<Decision> {
    use SessionState::*;
    let decision = |next, note: &str| Some(Decision { next, note: note.to_string() });
    match (session.state, evidence) {
        // The job does not exist before the starter attached.
        (Starting, ProcessEvidence::Job(0) | ProcessEvidence::JobGone) if now - session.started_at < ATTACH_GRACE => {
            None
        }
        (Starting, ProcessEvidence::Job(0) | ProcessEvidence::JobGone) => {
            decision(InterruptedUnknown, "Starter hat sich nicht angemeldet")
        }
        (Running, ProcessEvidence::Job(0) | ProcessEvidence::JobGone) => {
            decision(InterruptedUnknown, "Keine Prozesse mehr im Job, aber kein Abschluss gemeldet")
        }
        (Starting | Running, ProcessEvidence::Process { alive: false }) => {
            decision(InterruptedUnknown, "Prozess beendet ohne Abschlussmeldung")
        }
        (CompletionRequested, ProcessEvidence::Job(0) | ProcessEvidence::JobGone) => {
            decision(CompletedVerified, "Abschluss gemeldet und kein Prozess im Job mehr aktiv")
        }
        (CompletionRequested, ProcessEvidence::Process { alive: false })
            if session.origin == SessionOrigin::Cooperative =>
        {
            decision(CompletedVerified, "Kooperativ gemeldet; registrierter Prozess beendet (schwächerer Beleg)")
        }
        _ => None,
    }
}

/// Collects process evidence for a session, reopening its job by name.
pub fn evidence(session: &Session, held: Option<&Job>) -> ProcessEvidence {
    if let Some(job) = held {
        return job.active_processes().map_or(ProcessEvidence::None, ProcessEvidence::Job);
    }
    if let Some(name) = &session.job_name {
        return match Job::open(name) {
            Ok(Some(job)) => job.active_processes().map_or(ProcessEvidence::None, ProcessEvidence::Job),
            Ok(None) => ProcessEvidence::JobGone,
            Err(_) => ProcessEvidence::None,
        };
    }
    match session.process {
        Some(process) => match platform::process_alive(process) {
            Ok(alive) => ProcessEvidence::Process { alive },
            Err(_) => ProcessEvidence::None,
        },
        None => ProcessEvidence::None,
    }
}

/// True if a process with this identity is alive (PID reuse aware).
pub fn alive(process: ProcessIdentity) -> bool {
    platform::process_alive(process).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(state: SessionState, origin: SessionOrigin) -> Session {
        Session {
            id: 1,
            scratch_id: 1,
            purpose: String::new(),
            state,
            origin,
            process: None,
            job_name: Some("x".into()),
            started_at: 0,
            heartbeat_at: None,
            ended_at: None,
            exit_code: None,
            remaining_processes: None,
            note: None,
        }
    }

    #[test]
    fn starter_exit_without_report_is_unclear() {
        let s = session(SessionState::Running, SessionOrigin::Managed);
        assert_eq!(decide(&s, ProcessEvidence::JobGone, 0).unwrap().next, SessionState::InterruptedUnknown);
        assert_eq!(decide(&s, ProcessEvidence::Job(2), 0), None, "still running stays running");
    }

    #[test]
    fn a_fresh_start_gets_time_to_attach() {
        let s = session(SessionState::Starting, SessionOrigin::Managed);
        assert_eq!(decide(&s, ProcessEvidence::JobGone, 5), None);
        assert_eq!(
            decide(&s, ProcessEvidence::JobGone, ATTACH_GRACE + 1).unwrap().next,
            SessionState::InterruptedUnknown
        );
    }

    #[test]
    fn reported_completion_needs_an_empty_job() {
        let s = session(SessionState::CompletionRequested, SessionOrigin::Managed);
        assert_eq!(decide(&s, ProcessEvidence::Job(1), 0), None, "worker still writing (AT-12)");
        assert_eq!(decide(&s, ProcessEvidence::Job(0), 0).unwrap().next, SessionState::CompletedVerified);
        assert_eq!(decide(&s, ProcessEvidence::None, 0), None);
    }

    #[test]
    fn managed_sessions_do_not_complete_on_a_dead_pid() {
        let s = session(SessionState::CompletionRequested, SessionOrigin::Managed);
        assert_eq!(decide(&s, ProcessEvidence::Process { alive: false }, 0), None);
    }
}
