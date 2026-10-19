//! Managed session lifecycle and reconciliation of process evidence.
use super::Core;
use crate::domain::*;
use crate::platform::{self, Job};
use crate::sessions::{self, StartInfo};
use crate::storage::SessionUpdate;
use crate::{Error, Result};

impl Core {
    /// Starts a managed session and tells the starter where to route TEMP.
    pub fn session_start(&mut self, scratch_id: i64, purpose: &str) -> Result<StartInfo> {
        let children = self.repo.children(scratch_id)?;
        let folder = |kind| {
            children
                .iter()
                .find(|c| c.kind == kind && c.acquisition == Acquisition::Managed)
                .map(|c| c.path.clone())
                .ok_or_else(|| Error::invalid("Verwaltete Läufe benötigen einen von mogumogu angelegten Scratchpad."))
        };
        let temp_dir = folder(ResourceKind::Temporary)?;
        let results_dir = folder(ResourceKind::Results)?;
        let prefix = self.config.job_prefix()?;
        let session_id = self.repo.create_session(scratch_id, purpose, SessionOrigin::Managed, Some(&prefix))?;
        Ok(StartInfo { session_id, temp_dir, results_dir, job_name: format!("{prefix}{session_id}") })
    }

    pub fn session_attach(&mut self, session_id: i64, process: ProcessIdentity) -> Result<Session> {
        let session = self.repo.session(session_id)?;
        if let Some(name) = &session.job_name
            && let Some(job) = Job::open(name)?
        {
            self.jobs.insert(session_id, job);
        }
        self.repo.transition_session(
            session_id,
            SessionState::Running,
            SessionUpdate { process: Some(process), ..Default::default() },
        )
    }

    pub fn session_heartbeat(&mut self, session_id: i64) -> Result<()> {
        self.repo.session_heartbeat(session_id)
    }

    /// The starter reports completion. This is a claim, not proof.
    pub fn session_report(&mut self, session_id: i64, exit_code: i64, remaining: i64) -> Result<Session> {
        let update =
            SessionUpdate { exit_code: Some(exit_code), remaining: Some(remaining), ended: true, ..Default::default() };
        self.repo.transition_session(session_id, SessionState::CompletionRequested, update)?;
        self.reconcile_sessions()?;
        self.repo.session(session_id)
    }

    /// Cooperative session registration by an integration (weaker evidence).
    pub fn session_register(&mut self, scratch_id: i64, purpose: &str, pid: Option<u32>) -> Result<Session> {
        let id = self.repo.create_session(scratch_id, purpose, SessionOrigin::Cooperative, None)?;
        let process = match pid {
            Some(pid) => {
                Some(platform::process_identity(pid)?.ok_or_else(|| Error::invalid("Prozess nicht gefunden."))?)
            }
            None => None,
        };
        self.repo.transition_session(id, SessionState::Running, SessionUpdate { process, ..Default::default() })
    }

    pub fn session_complete(&mut self, session_id: i64) -> Result<Session> {
        let update = SessionUpdate { ended: true, ..Default::default() };
        self.repo.transition_session(session_id, SessionState::CompletionRequested, update)?;
        self.reconcile_sessions()?;
        self.repo.session(session_id)
    }

    /// Resolves a stuck lock after a person reviewed it. No cleanup approval
    /// follows from this; a new plan is still required. A session that has
    /// not ended is only released when it stalled and no process of it is
    /// alive any more; mogumogu never ends processes itself.
    pub fn session_release(&mut self, session_id: i64, reason: &str) -> Result<Session> {
        let reason = reason.trim();
        if reason.chars().count() < 5 {
            return Err(Error::invalid("Bitte eine Begründung angeben (mindestens 5 Zeichen)."));
        }
        let session = self.repo.session(session_id)?;
        if matches!(session.state, SessionState::Starting | SessionState::Running | SessionState::CompletionRequested) {
            if session.stall(self.repo.now()).is_none_or(|stall| stall == Stall::LongRunning) {
                return Err(Error::blocked(
                    "Diese Session zeigt noch Aktivität. Freigeben ist erst möglich, wenn sie hängt.",
                ));
            }
            match sessions::evidence(&session, self.jobs.get(&session.id)) {
                sessions::ProcessEvidence::Job(n) if n > 0 => {
                    return Err(Error::blocked(format!(
                        "Im Job laufen noch {n} Prozesse. mogumogu beendet sie nicht; erst nach deren Ende freigeben."
                    )));
                }
                sessions::ProcessEvidence::Process { alive: true } => {
                    return Err(Error::blocked(
                        "Der registrierte Prozess läuft noch; erst nach seinem Ende freigeben.",
                    ));
                }
                _ => {}
            }
            let marked =
                SessionUpdate { note: Some("Von einer Person als hängend markiert".into()), ..Default::default() };
            self.repo.transition_session(session_id, SessionState::InterruptedUnknown, marked)?;
            self.jobs.remove(&session_id);
        }
        let update = SessionUpdate { note: Some(format!("Nach Prüfung freigegeben: {reason}")), ..Default::default() };
        self.repo.transition_session(session_id, SessionState::ReleasedAfterReview, update)
    }

    /// Applies the decision table to every open session.
    pub fn reconcile_sessions(&mut self) -> Result<usize> {
        if self.is_demo() {
            return Ok(0);
        }
        let mut changed = 0;
        for session in self.repo.open_sessions()? {
            let evidence = sessions::evidence(&session, self.jobs.get(&session.id));
            if let Some(decision) = sessions::decide(&session, evidence, self.repo.now()) {
                let update = SessionUpdate { note: Some(decision.note), ..Default::default() };
                self.repo.transition_session(session.id, decision.next, update)?;
                if !decision.next.locks_resources() || decision.next == SessionState::InterruptedUnknown {
                    self.jobs.remove(&session.id);
                }
                changed += 1;
            }
        }
        self.changed |= changed > 0;
        Ok(changed)
    }
}
