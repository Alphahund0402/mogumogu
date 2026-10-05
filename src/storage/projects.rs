//! Projects: explicit registration only. Registration stores metadata; it
//! never reads, creates or changes the folder and grants no read approval.
use super::Repository;
use super::activity::log_in;
use crate::domain::{Observation, Origin, Project};
use crate::{Error, Result, limits, validation};
use rusqlite::{OptionalExtension, Row, Transaction, params};

/// One grouped pass over the published generations instead of correlated
/// sub-queries per project (which grow with projects × inventory size).
const PROJECT_QUERY: &str = "
    WITH complete AS (
        SELECT g.id AS gid, s.project_id AS pid FROM generations g JOIN scopes s ON s.id = g.scope_id
        WHERE g.state = 'complete' AND s.project_id IS NOT NULL),
    packages AS (
        SELECT c.pid, COUNT(DISTINCT i.ecosystem || '|' || i.name || '|' || IFNULL(i.version, '')) AS n
        FROM complete c JOIN inventory_items i ON i.generation_id = c.gid
        WHERE i.category = 'package' AND i.install_state IN ('resolved','installed') GROUP BY c.pid),
    ecosystems AS (
        SELECT pid, group_concat(e, ' · ') AS e FROM (
            SELECT DISTINCT c.pid AS pid, i.ecosystem AS e
            FROM complete c JOIN inventory_items i ON i.generation_id = c.gid
            WHERE i.category = 'manifest' ORDER BY c.pid, e)
        GROUP BY pid)
    SELECT p.id, p.name, p.path, p.ecosystem, p.state, p.evidence, p.bytes, p.packages, p.protected, p.origin,
      IFNULL(packages.n, 0) AS inventory_packages,
      EXISTS(SELECT 1 FROM complete c WHERE c.pid = p.id) AS has_complete,
      ecosystems.e AS ecosystems,
      EXISTS(SELECT 1 FROM scopes s WHERE s.project_id = p.id AND s.status = 'approved') AS read_approved
    FROM projects p
    LEFT JOIN packages ON packages.pid = p.id
    LEFT JOIN ecosystems ON ecosystems.pid = p.id";

fn map_project(row: &Row<'_>) -> rusqlite::Result<Project> {
    let origin: Origin = row.get(9)?;
    let has_complete: bool = row.get(11)?;
    let ecosystems: Option<String> = row.get(12)?;
    let stored_packages: Option<i64> = row.get(7)?;
    let packages = match origin {
        Origin::Demo => stored_packages,
        Origin::Registered => has_complete.then(|| row.get::<_, i64>(10)).transpose()?,
    };
    let stored_ecosystem: String = row.get(3)?;
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        ecosystem: ecosystems.filter(|e| !e.is_empty()).unwrap_or(stored_ecosystem),
        state: row.get(4)?,
        evidence: row.get(5)?,
        bytes: row.get(6)?,
        packages,
        protected: row.get(8)?,
        origin,
        read_approved: row.get(13)?,
        coverage: String::new(),
    })
}

impl Repository {
    /// Registers a project; repeating the exact normalized path returns the
    /// existing id (idempotent).
    pub fn register_project(&mut self, name: &str, path: &str) -> Result<i64> {
        self.require_local()?;
        let name = validation::name(name)?;
        let path = validation::windows_path(path)?;
        let now = self.now();
        self.write(|tx| {
            let existing: Option<i64> =
                tx.query_row("SELECT id FROM projects WHERE path = ?1", [&path], |r| r.get(0)).optional()?;
            if let Some(id) = existing {
                return Ok(id);
            }
            let count: i64 = tx.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?;
            if count >= limits::MAX_PROJECTS {
                return Err(Error::blocked("Prototyp-Limit: höchstens 2.000 registrierte Projekte."));
            }
            tx.execute("INSERT INTO projects(name, path, origin) VALUES(?1, ?2, 'registered')", params![name, path])?;
            let id = tx.last_insert_rowid();
            refresh_inferred_owners(tx)?;
            log_in(
                tx,
                now,
                "registration",
                "Projekt registriert",
                "Metadaten gespeichert; kein Dateisystemscan ausgeführt.",
            )?;
            Ok(id)
        })
    }

    pub fn projects(&mut self) -> Result<Vec<Project>> {
        let sql = format!("{PROJECT_QUERY} ORDER BY p.id LIMIT {}", limits::MAX_PROJECTS);
        self.read(|tx| {
            let mut statement = tx.prepare(&sql)?;
            let rows = statement.query_map([], map_project)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Stores a measured size (`None` = measurement incomplete, unknown).
    pub fn record_project_size(&mut self, id: i64, bytes: Option<i64>) -> Result<()> {
        self.require_local()?;
        self.write(|tx| {
            tx.execute("UPDATE projects SET bytes = ?1 WHERE id = ?2 AND origin = 'registered'", params![bytes, id])?;
            Ok(())
        })
    }

    pub fn project(&mut self, id: i64) -> Result<Project> {
        let sql = format!("{PROJECT_QUERY} WHERE p.id = ?1");
        self.read(|tx| {
            tx.query_row(&sql, [id], map_project)
                .optional()?
                .ok_or_else(|| Error::not_found("Projekt-ID nicht gefunden."))
        })
    }
}

/// Records observed activity for a project (e.g. a verified managed run).
pub(crate) fn mark_project_observed(tx: &Transaction<'_>, project_id: i64, evidence: &str) -> Result<()> {
    tx.execute(
        "UPDATE projects SET state = ?1, evidence = ?2 WHERE id = ?3 AND origin = 'registered'",
        params![Observation::Observed, crate::privacy::label(evidence), project_id],
    )?;
    Ok(())
}

/// Recomputes lexical ownership hints: a resource below a registered project
/// path is *probably* owned by it. Explicit owners stay untouched.
pub(crate) fn refresh_inferred_owners(tx: &Transaction<'_>) -> Result<()> {
    tx.execute("DELETE FROM owner_links WHERE kind = 'inferred'", [])?;
    tx.execute(
        "INSERT OR IGNORE INTO owner_links(resource_id, project_id, kind, evidence)
         SELECT r.id, p.id, 'inferred',
                'Liegt unterhalb des registrierten Projektpfads (lexikalischer Hinweis, Identität nicht geprüft).'
         FROM resources r JOIN projects p
           ON lower(substr(r.path, 1, length(p.path) + 1)) = lower(p.path || '\\')
         WHERE r.origin = 'registered' AND p.origin = 'registered'
           AND NOT EXISTS (SELECT 1 FROM owner_links o
                           WHERE o.resource_id = r.id AND o.project_id = p.id AND o.kind = 'explicit')",
        [],
    )?;
    Ok(())
}
