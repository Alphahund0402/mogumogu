//! Cleanup plans, identity manifests and the durable action journal (F-22,
//! F-23). Every journal state change is committed with `synchronous=FULL`
//! *before* the corresponding filesystem action starts.
use super::Repository;
use super::activity::log_in;
use crate::clock::Timestamp;
use crate::domain::{JournalState, PlanState};
use crate::platform::FileIdentity;
use crate::{Error, Result, privacy};
use rusqlite::{OptionalExtension, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestEntry {
    /// `/`-separated path below the target root.
    pub rel_path: String,
    pub identity: FileIdentity,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
    pub depth: u32,
}

#[derive(Clone, Debug)]
pub struct StoredTarget {
    pub id: i64,
    pub resource_id: i64,
    pub path: String,
    pub identity: FileIdentity,
    pub entries: i64,
    pub bytes: i64,
    pub blockers: Vec<String>,
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

#[derive(Clone, Debug)]
pub struct JournalEntry {
    pub id: i64,
    pub target_id: i64,
    pub rel_path: String,
    pub identity: FileIdentity,
    pub is_dir: bool,
    pub depth: u32,
    pub state: JournalState,
    pub error: Option<String>,
}

/// A target with its manifest, as drafted by the planner.
#[derive(Clone, Debug)]
pub struct NewTarget {
    pub resource_id: i64,
    pub path: String,
    pub identity: FileIdentity,
    pub blockers: Vec<String>,
    pub manifest: Vec<ManifestEntry>,
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
            let mut manifest = tx.prepare(
                "INSERT INTO cleanup_manifest(target_id, rel_path, identity, is_dir, size, modified, depth)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for target in targets {
                let bytes: u64 = target.manifest.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
                tx.execute(
                    "INSERT INTO cleanup_targets(plan_id, resource_id, path, identity, entries, bytes, blockers)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        plan_id,
                        target.resource_id,
                        target.path,
                        target.identity.to_string(),
                        target.manifest.len() as i64,
                        bytes as i64,
                        serde_json::to_string(&target.blockers)?
                    ],
                )?;
                let target_id = tx.last_insert_rowid();
                for entry in &target.manifest {
                    manifest.execute(params![
                        target_id,
                        entry.rel_path,
                        entry.identity.to_string(),
                        entry.is_dir,
                        entry.size as i64,
                        entry.modified,
                        entry.depth
                    ])?;
                }
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
                "SELECT id, resource_id, path, identity, entries, bytes, blockers FROM cleanup_targets WHERE plan_id = ?1 ORDER BY id",
            )?;
            let rows = statement.query_map([id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?, r.get(4)?, r.get(5)?, r.get::<_, String>(6)?))
            })?;
            let mut targets = Vec::new();
            for row in rows {
                let (id, resource_id, path, identity, entries, bytes, blockers) = row?;
                targets.push(StoredTarget {
                    id,
                    resource_id,
                    path,
                    identity: identity.parse()?,
                    entries,
                    bytes,
                    blockers: serde_json::from_str(&blockers)?,
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

    pub fn manifest(&mut self, target_id: i64) -> Result<Vec<ManifestEntry>> {
        self.read(|tx| {
            let mut statement = tx.prepare(
                "SELECT rel_path, identity, is_dir, size, modified, depth FROM cleanup_manifest WHERE target_id = ?1 ORDER BY rel_path",
            )?;
            let rows = statement.query_map([target_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get::<_, i64>(3)?, r.get(4)?, r.get(5)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (rel_path, identity, is_dir, size, modified, depth) = row?;
                out.push(ManifestEntry { rel_path, identity: identity.parse()?, is_dir, size: size as u64, modified, depth });
            }
            Ok(out)
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

    /// Writes all manifest entries of a plan as `planned` journal rows.
    pub fn journal_create(&mut self, plan_id: i64) -> Result<usize> {
        let now = self.now();
        self.write(|tx| {
            let existing: i64 =
                tx.query_row("SELECT COUNT(*) FROM action_journal WHERE plan_id = ?1", [plan_id], |r| r.get(0))?;
            if existing > 0 {
                return Err(Error::conflict("Für diesen Plan existiert bereits ein Journal; kein erneuter Lauf."));
            }
            Ok(tx.execute(
                "INSERT INTO action_journal(plan_id, target_id, rel_path, identity, is_dir, depth, state, updated_at)
                 SELECT t.plan_id, m.target_id, m.rel_path, m.identity, m.is_dir, m.depth, 'planned', ?2
                 FROM cleanup_manifest m JOIN cleanup_targets t ON t.id = m.target_id WHERE t.plan_id = ?1",
                params![plan_id, now],
            )?)
        })
    }

    /// Durably sets the state of several journal rows in one commit.
    pub fn journal_mark(&mut self, ids: &[i64], state: JournalState, error: Option<&str>) -> Result<()> {
        let now = self.now();
        let error = error.map(privacy::label);
        self.write(|tx| {
            let mut statement =
                tx.prepare("UPDATE action_journal SET state = ?1, error = ?2, updated_at = ?3 WHERE id = ?4")?;
            for id in ids {
                statement.execute(params![state, error, now, id])?;
            }
            Ok(())
        })
    }

    pub fn journal(&mut self, plan_id: i64) -> Result<Vec<JournalEntry>> {
        self.read(|tx| {
            let mut statement = tx.prepare(
                "SELECT id, target_id, rel_path, identity, is_dir, depth, state, error FROM action_journal
                 WHERE plan_id = ?1 ORDER BY depth DESC, is_dir ASC, rel_path",
            )?;
            let rows = statement.query_map([plan_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, target_id, rel_path, identity, is_dir, depth, state, error) = row?;
                out.push(JournalEntry {
                    id,
                    target_id,
                    rel_path,
                    identity: identity.parse()?,
                    is_dir,
                    depth,
                    state,
                    error,
                });
            }
            Ok(out)
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
                "Nur hier ist der Testexecutor zulässig.",
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
