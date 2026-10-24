//! Cleanup plans with one durable journal row per folder (F-22, F-23).
//! Every state change is committed with `synchronous=FULL` *before* the
//! folder is moved.
use super::Repository;
use super::activity::log_in;
use crate::clock::Timestamp;
use crate::domain::{JournalState, PlanState};
use crate::platform::FileIdentity;
use crate::{Error, Result, privacy};
use rusqlite::{OptionalExtension, params};

#[derive(Clone, Debug)]
pub struct StoredTarget {
    pub id: i64,
    pub scope_id: i64,
    /// `/`-separated path below the scope root.
    pub rel_path: String,
    pub identity: FileIdentity,
    pub label: String,
    pub evidence: String,
    pub bytes: i64,
    pub blockers: Vec<String>,
    pub state: JournalState,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StoredPlan {
    pub id: i64,
    pub state: PlanState,
    pub created_at: Timestamp,
    pub approved_at: Option<Timestamp>,
    pub rule_version: String,
    pub fingerprint: String,
    pub note: Option<String>,
    pub targets: Vec<StoredTarget>,
}

/// A folder as drafted by the planner.
#[derive(Clone, Debug)]
pub struct NewTarget {
    pub scope_id: i64,
    pub rel_path: String,
    pub identity: FileIdentity,
    pub label: String,
    pub evidence: String,
    pub bytes: u64,
    pub blockers: Vec<String>,
}

impl Repository {
    pub fn insert_plan(&mut self, rule_version: &str, fingerprint: &str, targets: &[NewTarget]) -> Result<i64> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO cleanup_plans(state, created_at, rule_version, fingerprint) VALUES('draft', ?1, ?2, ?3)",
                params![now, rule_version, fingerprint],
            )?;
            let plan_id = tx.last_insert_rowid();
            let mut insert = tx.prepare(
                "INSERT INTO recycle_targets(plan_id, scope_id, rel_path, identity, label, evidence, bytes, blockers,
                                             updated_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for target in targets {
                insert.execute(params![
                    plan_id,
                    target.scope_id,
                    target.rel_path,
                    target.identity.to_string(),
                    target.label,
                    target.evidence,
                    target.bytes as i64,
                    serde_json::to_string(&target.blockers)?,
                    now
                ])?;
            }
            log_in(tx, now, "cleanup", "Bereinigungsplan entworfen", "Entwurf; nichts wurde verändert.")?;
            Ok(plan_id)
        })
    }

    pub fn plan(&mut self, id: i64) -> Result<StoredPlan> {
        self.read(|tx| {
            let plan = tx
                .query_row(
                    "SELECT id, state, created_at, approved_at, rule_version, fingerprint, note FROM cleanup_plans WHERE id = ?1",
                    [id],
                    |r| {
                        Ok(StoredPlan {
                            id: r.get(0)?,
                            state: r.get(1)?,
                            created_at: r.get(2)?,
                            approved_at: r.get(3)?,
                            rule_version: r.get(4)?,
                            fingerprint: r.get(5)?,
                            note: r.get(6)?,
                            targets: Vec::new(),
                        })
                    },
                )
                .optional()?
                .ok_or_else(|| Error::not_found("Plan nicht gefunden."))?;
            let mut statement = tx.prepare(
                "SELECT id, scope_id, rel_path, identity, label, evidence, bytes, blockers, state, error
                 FROM recycle_targets WHERE plan_id = ?1 ORDER BY id",
            )?;
            let rows = statement.query_map([id], |r| {
                Ok((
                    (r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?),
                    (r.get(4)?, r.get(5)?, r.get(6)?, r.get::<_, String>(7)?, r.get(8)?, r.get(9)?),
                ))
            })?;
            let mut targets = Vec::new();
            for row in rows {
                let ((id, scope_id, rel_path, identity), (label, evidence, bytes, blockers, state, error)) = row?;
                targets.push(StoredTarget {
                    id,
                    scope_id,
                    rel_path,
                    identity: identity.parse()?,
                    label,
                    evidence,
                    bytes,
                    blockers: serde_json::from_str(&blockers)?,
                    state,
                    error,
                });
            }
            Ok(StoredPlan { targets, ..plan })
        })
    }

    pub fn plan_ids(&mut self, limit: i64) -> Result<Vec<i64>> {
        self.read(|tx| {
            let mut statement = tx.prepare("SELECT id FROM cleanup_plans ORDER BY id DESC LIMIT ?1")?;
            let rows = statement.query_map([limit], |r| r.get(0))?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Compare-and-set on the plan state; enforces the state machine.
    pub fn transition_plan(&mut self, id: i64, from: PlanState, to: PlanState, note: Option<&str>) -> Result<()> {
        self.require_local()?;
        if !from.can_become(to) {
            return Err(Error::conflict(format!("Plan kann nicht von {} zu {} wechseln.", from.label(), to.label())));
        }
        let now = self.now();
        let note = note.map(privacy::label);
        self.write(|tx| {
            let changed = tx.execute(
                "UPDATE cleanup_plans SET state = ?1, note = COALESCE(?2, note),
                    approved_at = CASE WHEN ?1 = 'approved' THEN ?3 ELSE approved_at END,
                    finished_at = CASE WHEN ?1 IN ('completed','partial','failed','cancelled','invalidated') THEN ?3 ELSE finished_at END
                 WHERE id = ?4 AND state = ?5",
                params![to, note, now, id, from],
            )?;
            if changed == 0 {
                return Err(Error::conflict("Planzustand wurde zwischenzeitlich geändert."));
            }
            log_in(tx, now, "cleanup", &format!("Plan: {}", to.label()), note.as_deref().unwrap_or(""))?;
            Ok(())
        })
    }

    /// Durably moves one folder's journal row from `from` to `to`. A row in
    /// any other state is left alone: no step runs twice.
    pub fn mark_target(&mut self, id: i64, from: JournalState, to: JournalState, error: Option<&str>) -> Result<()> {
        self.require_local()?;
        let now = self.now();
        let error = error.map(privacy::label);
        self.write(|tx| {
            let changed = tx.execute(
                "UPDATE recycle_targets SET state = ?1, error = ?2, updated_at = ?3 WHERE id = ?4 AND state = ?5",
                params![to, error, now, id, from],
            )?;
            if changed == 0 {
                return Err(Error::conflict("Journalzustand wurde zwischenzeitlich geändert."));
            }
            Ok(())
        })
    }

    pub fn register_disposable_root(&mut self, path: &str, identity: FileIdentity) -> Result<i64> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO disposable_roots(path, identity, registered_at) VALUES(?1, ?2, ?3)
                 ON CONFLICT(path) DO UPDATE SET identity = excluded.identity, registered_at = excluded.registered_at",
                params![path, identity.to_string(), now],
            )?;
            log_in(
                tx,
                now,
                "cleanup",
                "Wegwerfbare Testwurzel registriert",
                "Nur hier darf ein Plan vor dem Review ausgeführt werden.",
            )?;
            Ok(tx.query_row("SELECT id FROM disposable_roots WHERE path = ?1", [path], |r| r.get(0))?)
        })
    }

    pub fn disposable_roots(&mut self) -> Result<Vec<(String, FileIdentity)>> {
        let rows: Vec<(String, String)> = self.read(|tx| {
            let mut statement = tx.prepare("SELECT path, identity FROM disposable_roots ORDER BY id")?;
            let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })?;
        rows.into_iter().map(|(p, i)| Ok((p, i.parse()?))).collect()
    }
}
