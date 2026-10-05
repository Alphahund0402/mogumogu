//! Read approvals ("Suchbereiche"). Registration and read approval are
//! separate decisions (F-05); an approval is bound to the identity of the
//! approved folder, never to the path text alone (F-06).
use super::Repository;
use super::activity::log_in;
use crate::domain::{GenerationSummary, Scope, ScopeKind, ScopeStatus};
use crate::platform::FileIdentity;
use crate::{Error, Result, privacy};
use rusqlite::{OptionalExtension, Row, Transaction, params};

const SCOPE_COLUMNS: &str = "id, project_id, kind, path, status, approved_at, last_error";
const GENERATION_COLUMNS: &str =
    "id, state, started_at, finished_at, entries_seen, files_read, item_count, limit_reason, error_count";

fn map_generation(row: &Row<'_>) -> rusqlite::Result<GenerationSummary> {
    Ok(GenerationSummary {
        id: row.get(0)?,
        state: row.get(1)?,
        started_at: row.get(2)?,
        finished_at: row.get(3)?,
        entries_seen: row.get(4)?,
        files_read: row.get(5)?,
        items: row.get(6)?,
        limit_reason: row.get(7)?,
        error_count: row.get(8)?,
    })
}

fn load_scope(tx: &Transaction<'_>, row: &Row<'_>) -> rusqlite::Result<Scope> {
    let id: i64 = row.get(0)?;
    let published = tx
        .query_row(
            &format!("SELECT {GENERATION_COLUMNS} FROM generations WHERE scope_id = ?1 AND state = 'complete' ORDER BY id DESC LIMIT 1"),
            [id],
            map_generation,
        )
        .optional()?;
    let latest = tx
        .query_row(
            &format!("SELECT {GENERATION_COLUMNS} FROM generations WHERE scope_id = ?1 ORDER BY id DESC LIMIT 1"),
            [id],
            map_generation,
        )
        .optional()?;
    Ok(Scope {
        id,
        project_id: row.get(1)?,
        kind: row.get(2)?,
        path: row.get(3)?,
        status: row.get(4)?,
        approved_at: row.get(5)?,
        last_error: row.get(6)?,
        published,
        latest,
        watched: false,
    })
}

impl Repository {
    /// Approves reading below `path`. An earlier approval at the same path
    /// for a *different* object is revoked, not inherited.
    pub fn approve_scope(
        &mut self,
        kind: ScopeKind,
        project_id: Option<i64>,
        path: &str,
        identity: Option<FileIdentity>,
    ) -> Result<i64> {
        self.require_local()?;
        let now = self.now();
        let identity = identity.map(|i| i.to_string());
        self.write(|tx| {
            if let Some(project) = project_id {
                let exists: bool =
                    tx.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)", [project], |r| r.get(0))?;
                if !exists {
                    return Err(Error::not_found("Projekt-ID nicht gefunden."));
                }
            }
            let existing: Option<(i64, Option<String>)> = tx
                .query_row(
                    "SELECT id, identity FROM scopes WHERE kind = ?1 AND path = ?2 AND status <> 'revoked'",
                    params![kind, path],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((id, stored)) = existing {
                if stored == identity {
                    tx.execute("UPDATE scopes SET status = 'approved', last_error = NULL WHERE id = ?1", [id])?;
                    return Ok(id);
                }
                tx.execute("UPDATE scopes SET status = 'revoked' WHERE id = ?1", [id])?;
            }
            tx.execute(
                "INSERT INTO scopes(project_id, kind, path, identity, status, approved_at) VALUES(?1, ?2, ?3, ?4, 'approved', ?5)",
                params![project_id, kind, path, identity, now],
            )?;
            let id = tx.last_insert_rowid();
            log_in(tx, now, "scope", "Lesefreigabe erteilt", &format!("{} · neue Vergleichsbasis beginnt", kind.label()))?;
            Ok(id)
        })
    }

    pub fn revoke_scope(&mut self, id: i64) -> Result<()> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            let changed =
                tx.execute("UPDATE scopes SET status = 'revoked' WHERE id = ?1 AND status <> 'revoked'", [id])?;
            if changed == 0 {
                return Err(Error::not_found("Aktiver Suchbereich nicht gefunden."));
            }
            log_in(tx, now, "scope", "Lesefreigabe widerrufen", "Keine weiteren Lesezugriffe in diesem Bereich.")
        })
    }

    pub fn set_scope_status(&mut self, id: i64, status: ScopeStatus, error: Option<&str>) -> Result<()> {
        let error = error.map(privacy::label);
        self.write(|tx| {
            tx.execute(
                "UPDATE scopes SET status = ?1, last_error = ?2 WHERE id = ?3 AND status <> 'revoked'",
                params![status, error, id],
            )?;
            Ok(())
        })
    }

    pub fn scopes(&mut self) -> Result<Vec<Scope>> {
        self.read(|tx| {
            let mut statement = tx.prepare(&format!(
                "SELECT {SCOPE_COLUMNS} FROM scopes WHERE status <> 'revoked' ORDER BY id LIMIT 2000"
            ))?;
            let rows = statement.query_map([], |row| load_scope(tx, row))?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn scope(&mut self, id: i64) -> Result<Scope> {
        self.read(|tx| {
            tx.query_row(&format!("SELECT {SCOPE_COLUMNS} FROM scopes WHERE id = ?1"), [id], |row| load_scope(tx, row))
                .optional()?
                .ok_or_else(|| Error::not_found("Suchbereich nicht gefunden."))
        })
    }

    pub fn scope_identity(&mut self, id: i64) -> Result<Option<FileIdentity>> {
        let text: Option<String> = self.read(|tx| {
            Ok(tx.query_row("SELECT identity FROM scopes WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten())
        })?;
        text.map(|t| t.parse()).transpose()
    }

    /// Approved scopes of one kind (used for system-wide manager scopes).
    pub fn scopes_of_kind(&mut self, kind: ScopeKind) -> Result<Vec<Scope>> {
        Ok(self.scopes()?.into_iter().filter(|s| s.kind == kind).collect())
    }
}
