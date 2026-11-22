//! Read approvals ("Suchbereiche"). Registration and read approval are
//! separate decisions (F-05); an approval is bound to the identity of the
//! approved folder, never to the path text alone (F-06).
use super::Repository;
use super::activity::log_in;
use crate::domain::{GenerationSummary, Scope, ScopeKind, ScopeStatus};
use crate::platform::FileIdentity;
use crate::{Error, Result, privacy};
use rusqlite::{OptionalExtension, Row, params};

const SCOPES_LIST_SQL: &str = "
    SELECT s.id, s.project_id, s.kind, s.path, s.status, s.approved_at, s.last_error, s.profile, s.realm, s.cache, s.analysis,
           p.id, p.state, p.started_at, p.finished_at, p.entries_seen, p.files_read, p.item_count, p.limit_reason, p.error_count,
           l.id, l.state, l.started_at, l.finished_at, l.entries_seen, l.files_read, l.item_count, l.limit_reason, l.error_count
    FROM scopes s
    LEFT JOIN generations p ON p.id = (
        SELECT id FROM generations WHERE scope_id = s.id AND state = 'complete' ORDER BY id DESC LIMIT 1
    )
    LEFT JOIN generations l ON l.id = (
        SELECT id FROM generations WHERE scope_id = s.id ORDER BY id DESC LIMIT 1
    )
    WHERE s.status <> 'revoked' ORDER BY s.id LIMIT ?1";

const ADVISORY_SCOPES_SQL: &str = "
    SELECT s.id, s.project_id, s.kind, s.path, s.status, s.approved_at, s.last_error, s.profile, s.realm, s.cache, s.analysis,
           p.id, p.state, p.started_at, p.finished_at, p.entries_seen, p.files_read, p.item_count, p.limit_reason, p.error_count,
           l.id, l.state, l.started_at, l.finished_at, l.entries_seen, l.files_read, l.item_count, l.limit_reason, l.error_count
    FROM scopes s
    LEFT JOIN generations p ON p.id = (
        SELECT id FROM generations WHERE scope_id = s.id AND state = 'complete' ORDER BY id DESC LIMIT 1
    )
    LEFT JOIN generations l ON l.id = (
        SELECT id FROM generations WHERE scope_id = s.id ORDER BY id DESC LIMIT 1
    )
    WHERE s.status <> 'revoked' AND s.project_id IS NOT NULL AND (?1 IS NULL OR s.project_id = ?1)
      AND s.kind = 'project' AND s.profile = '' AND s.realm = '' AND s.cache = '' AND s.analysis = 0
    ORDER BY s.id LIMIT ?2";

const SCOPE_BY_ID_SQL: &str = "
    SELECT s.id, s.project_id, s.kind, s.path, s.status, s.approved_at, s.last_error, s.profile, s.realm, s.cache, s.analysis,
           p.id, p.state, p.started_at, p.finished_at, p.entries_seen, p.files_read, p.item_count, p.limit_reason, p.error_count,
           l.id, l.state, l.started_at, l.finished_at, l.entries_seen, l.files_read, l.item_count, l.limit_reason, l.error_count
    FROM scopes s
    LEFT JOIN generations p ON p.id = (
        SELECT id FROM generations WHERE scope_id = s.id AND state = 'complete' ORDER BY id DESC LIMIT 1
    )
    LEFT JOIN generations l ON l.id = (
        SELECT id FROM generations WHERE scope_id = s.id ORDER BY id DESC LIMIT 1
    )
    WHERE s.id = ?1";

fn map_generation_at(row: &Row<'_>, offset: usize) -> rusqlite::Result<Option<GenerationSummary>> {
    let Some(id) = row.get::<_, Option<i64>>(offset)? else {
        return Ok(None);
    };
    Ok(Some(GenerationSummary {
        id,
        state: row.get(offset + 1)?,
        started_at: row.get(offset + 2)?,
        finished_at: row.get(offset + 3)?,
        entries_seen: row.get(offset + 4)?,
        files_read: row.get(offset + 5)?,
        items: row.get(offset + 6)?,
        limit_reason: row.get(offset + 7)?,
        error_count: row.get(offset + 8)?,
    }))
}

fn map_scope(row: &Row<'_>) -> rusqlite::Result<Scope> {
    let id: i64 = row.get(0)?;
    let published = map_generation_at(row, 11)?;
    let latest = map_generation_at(row, 20)?;
    Ok(Scope {
        id,
        project_id: row.get(1)?,
        kind: if row.get::<_, bool>(10)? {
            ScopeKind::Storage
        } else {
            match (row.get::<_, String>(8)?.as_str(), row.get::<_, String>(7)?.as_str(), row.get::<_, String>(9)?) {
                ("user", ..) => ScopeKind::UserAi,
                ("", "", cache) if !cache.is_empty() => {
                    ScopeKind::parse(&cache).filter(|k| k.is_cache()).ok_or_else(|| rusqlite::Error::InvalidQuery)?
                }
                ("", "", _) => row.get(2)?,
                ("", profile, _) => ScopeKind::parse(profile).ok_or_else(|| rusqlite::Error::InvalidQuery)?,
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
        },
        path: row.get(3)?,
        status: row.get(4)?,
        approved_at: row.get(5)?,
        last_error: row.get(6)?,
        published,
        latest,
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
        let storage_kind = kind.storage_kind();
        let profile = kind.profile();
        let realm = kind.realm();
        let cache = kind.cache();
        let analysis = kind == ScopeKind::Storage;
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
                    "SELECT id, identity FROM scopes
                     WHERE kind = ?1 AND path = ?2 AND profile = ?3 AND realm = ?4 AND cache = ?5 AND analysis = ?6
                       AND status <> 'revoked'",
                    params![storage_kind, path, profile, realm, cache, analysis],
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
                "INSERT INTO scopes(project_id, kind, path, identity, status, approved_at, profile, realm, cache, analysis)
                 VALUES(?1, ?2, ?3, ?4, 'approved', ?5, ?6, ?7, ?8, ?9)",
                params![project_id, storage_kind, path, identity, now, profile, realm, cache, analysis],
            )?;
            let id = tx.last_insert_rowid();
            log_in(
                tx,
                now,
                "scope",
                "Lesefreigabe erteilt",
                &format!("{} · neue Vergleichsbasis beginnt", kind.label()),
            )?;
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

    /// Revokes approvals for sources that are no longer read (installed
    /// programs and their managers). Their stored inventory stays as history.
    pub fn retire_removed_scopes(&mut self) -> Result<usize> {
        if self.is_demo() {
            return Ok(0);
        }
        let now = self.now();
        self.write(|tx| {
            let changed = tx.execute(
                "UPDATE scopes SET status = 'revoked'
                 WHERE kind IN ('winget', 'scoop', 'chocolatey') AND status <> 'revoked'",
                [],
            )?;
            if changed > 0 {
                log_in(
                    tx,
                    now,
                    "scope",
                    "Freigaben für installierte Programme widerrufen",
                    "WinGet-, Scoop- und Chocolatey-Listen erfasst mogumogu nicht mehr.",
                )?;
            }
            Ok(changed)
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
        self.query(SCOPES_LIST_SQL, [crate::limits::MAX_VISIBLE_SCOPES as i64], map_scope)
    }

    /// Fetch project inventory approvals without consuming the limit on other scope kinds.
    pub(crate) fn advisory_project_scopes(&mut self, project_id: Option<i64>) -> Result<Vec<Scope>> {
        self.query(ADVISORY_SCOPES_SQL, params![project_id, crate::limits::MAX_VISIBLE_SCOPES as i64 + 1], map_scope)
    }

    pub fn scope(&mut self, id: i64) -> Result<Scope> {
        self.read(|tx| {
            let mut stmt = tx.prepare_cached(SCOPE_BY_ID_SQL)?;
            stmt.query_row([id], map_scope).optional()?.ok_or_else(|| Error::not_found("Suchbereich nicht gefunden."))
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
