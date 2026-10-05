//! Session persistence with enforced state transitions (F-16).
use super::Repository;
use super::activity::log_in;
use super::projects::mark_project_observed;
use crate::clock::display_local;
use crate::domain::{ProcessIdentity, ResourceKind, Session, SessionOrigin, SessionState};
use crate::{Error, Result, limits, privacy};
use rusqlite::{OptionalExtension, Row, params};

const COLUMNS: &str = "id, scratch_id, purpose, state, origin, pid, process_created, job_name, started_at,
    heartbeat_at, ended_at, exit_code, remaining_processes, note";

fn map_session(row: &Row<'_>) -> rusqlite::Result<Session> {
    let pid: Option<i64> = row.get(5)?;
    let created: Option<i64> = row.get(6)?;
    Ok(Session {
        id: row.get(0)?,
        scratch_id: row.get(1)?,
        purpose: row.get(2)?,
        state: row.get(3)?,
        origin: row.get(4)?,
        process: pid.zip(created).map(|(pid, created)| ProcessIdentity { pid: pid as u32, created: created as u64 }),
        job_name: row.get(7)?,
        started_at: row.get(8)?,
        heartbeat_at: row.get(9)?,
        ended_at: row.get(10)?,
        exit_code: row.get(11)?,
        remaining_processes: row.get(12)?,
        note: row.get(13)?,
    })
}

/// Fields written together with a state transition.
#[derive(Clone, Debug, Default)]
pub struct SessionUpdate {
    pub process: Option<ProcessIdentity>,
    pub job_name: Option<String>,
    pub exit_code: Option<i64>,
    pub remaining: Option<i64>,
    pub note: Option<String>,
    pub ended: bool,
}

impl Repository {
    /// Creates a session in `starting`. For managed runs the job name is
    /// stored immediately so a starter crash before attaching stays traceable.
    pub fn create_session(
        &mut self,
        scratch_id: i64,
        purpose: &str,
        origin: SessionOrigin,
        job_prefix: Option<&str>,
    ) -> Result<i64> {
        self.require_local()?;
        let scratch = self.resource(scratch_id)?;
        if scratch.kind != ResourceKind::Scratchpad {
            return Err(Error::invalid("Sessions gehören zu einem Scratchpad."));
        }
        let now = self.now();
        let purpose = privacy::label(purpose);
        self.write(|tx| {
            tx.execute(
                "INSERT INTO sessions(scratch_id, purpose, state, origin, started_at) VALUES(?1, ?2, 'starting', ?3, ?4)",
                params![scratch_id, purpose, origin, now],
            )?;
            let id = tx.last_insert_rowid();
            if let Some(prefix) = job_prefix {
                tx.execute("UPDATE sessions SET job_name = ?1 WHERE id = ?2", params![format!("{prefix}{id}"), id])?;
            }
            log_in(tx, now, "session", "Session gestartet", &format!("{} · Ressourcen gesperrt", scratch.name))?;
            Ok(id)
        })
    }

    pub fn session(&mut self, id: i64) -> Result<Session> {
        self.read(|tx| {
            tx.query_row(&format!("SELECT {COLUMNS} FROM sessions WHERE id = ?1"), [id], map_session)
                .optional()?
                .ok_or_else(|| Error::not_found("Session nicht gefunden."))
        })
    }

    pub fn sessions(&mut self) -> Result<Vec<Session>> {
        self.read(|tx| {
            let mut statement =
                tx.prepare(&format!("SELECT {COLUMNS} FROM sessions ORDER BY id DESC LIMIT {}", limits::UI_PAGE))?;
            let rows = statement.query_map([], map_session)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Sessions that still lock resources and need reconciliation.
    pub fn open_sessions(&mut self) -> Result<Vec<Session>> {
        self.read(|tx| {
            let mut statement = tx.prepare(&format!(
                "SELECT {COLUMNS} FROM sessions WHERE state IN ('starting','running','completion_requested') ORDER BY id"
            ))?;
            let rows = statement.query_map([], map_session)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn sessions_for_scratch(&mut self, scratch_id: i64) -> Result<Vec<Session>> {
        self.read(|tx| {
            let mut statement =
                tx.prepare(&format!("SELECT {COLUMNS} FROM sessions WHERE scratch_id = ?1 ORDER BY id"))?;
            let rows = statement.query_map([scratch_id], map_session)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Moves a session forward; invalid transitions are refused.
    pub fn transition_session(&mut self, id: i64, next: SessionState, update: SessionUpdate) -> Result<Session> {
        self.require_local()?;
        let current = self.session(id)?;
        if current.state == next && next != SessionState::Running {
            return Ok(current);
        }
        if !current.state.can_become(next) {
            return Err(Error::conflict(format!(
                "Sessionzustand {} kann nicht zu {} wechseln.",
                current.state.label(),
                next.label()
            )));
        }
        let now = self.now();
        let scratch = self.resource(current.scratch_id)?;
        self.write(|tx| {
            tx.execute(
                "UPDATE sessions SET state = ?1,
                    pid = COALESCE(?2, pid), process_created = COALESCE(?3, process_created),
                    job_name = COALESCE(?4, job_name), exit_code = COALESCE(?5, exit_code),
                    remaining_processes = COALESCE(?6, remaining_processes), note = COALESCE(?7, note),
                    ended_at = CASE WHEN ?8 THEN ?9 ELSE ended_at END
                 WHERE id = ?10",
                params![
                    next,
                    update.process.map(|p| i64::from(p.pid)),
                    update.process.map(|p| p.created as i64),
                    update.job_name,
                    update.exit_code,
                    update.remaining,
                    update.note.as_deref().map(privacy::label),
                    update.ended,
                    now,
                    id
                ],
            )?;
            tx.execute("UPDATE resources SET last_activity_at = ?1 WHERE id = ?2", params![now, current.scratch_id])?;
            if next == SessionState::CompletedVerified
                && let Some(project) = scratch.project_id
            {
                mark_project_observed(
                    tx,
                    project,
                    &format!("Verwalteter Lauf abgeschlossen am {}.", display_local(now)),
                )?;
            }
            log_in(tx, now, "session", &format!("Session: {}", next.label()), &scratch.name)?;
            Ok(())
        })?;
        self.session(id)
    }

    pub fn session_heartbeat(&mut self, id: i64) -> Result<()> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            let changed = tx.execute(
                "UPDATE sessions SET heartbeat_at = ?1 WHERE id = ?2 AND state IN ('starting','running')",
                params![now, id],
            )?;
            if changed == 0 {
                return Err(Error::conflict("Heartbeat nur für laufende Sessions."));
            }
            Ok(())
        })
    }
}
