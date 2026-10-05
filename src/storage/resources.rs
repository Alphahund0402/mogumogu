//! Resources (scratchpads, session folders, environments, outputs) and their
//! ownership links. Protection is the default for every new resource.
use super::Repository;
use super::activity::log_in;
use super::projects::refresh_inferred_owners;
use crate::clock::{DAY, Timestamp};
use crate::domain::{Acquisition, OwnerKind, OwnerLink, Resource, ResourceKind};
use crate::platform::FileIdentity;
use crate::{Error, Result, limits, privacy, validation};
use rusqlite::{OptionalExtension, Row, Transaction, params};

const COLUMNS: &str = "id, project_id, name, path, kind, bytes, state, evidence, protected, origin,
    acquisition, parent_id, purpose, review_at, expendable, last_activity_at, created_at";

fn map_resource(row: &Row<'_>) -> rusqlite::Result<Resource> {
    Ok(Resource {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        path: row.get(3)?,
        kind: row.get(4)?,
        bytes: row.get(5)?,
        state: row.get(6)?,
        evidence: row.get(7)?,
        protected: row.get(8)?,
        origin: row.get(9)?,
        acquisition: row.get(10)?,
        parent_id: row.get(11)?,
        purpose: row.get(12)?,
        review_at: row.get(13)?,
        expendable: row.get(14)?,
        last_activity_at: row.get(15)?,
        created_at: row.get(16)?,
    })
}

/// A folder of a managed scratchpad, created on disk before it is recorded.
#[derive(Clone, Debug)]
pub struct ManagedFolder {
    pub kind: ResourceKind,
    pub path: String,
    pub identity: FileIdentity,
}

fn require_project(tx: &Transaction<'_>, project_id: i64) -> Result<()> {
    let exists: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)", [project_id], |r| r.get(0))?;
    if !exists {
        return Err(Error::not_found("Projekt-ID nicht gefunden."));
    }
    Ok(())
}

fn check_capacity(tx: &Transaction<'_>, adding: i64) -> Result<()> {
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))?;
    if count + adding > limits::MAX_RESOURCES {
        return Err(Error::blocked("Prototyp-Limit: höchstens 5.000 registrierte Ressourcen."));
    }
    Ok(())
}

impl Repository {
    /// Registers an existing scratchpad path for a known owner. Idempotent
    /// for the same owner; a different owner is rejected, never reassigned.
    pub fn register_scratch(&mut self, project_id: i64, name: &str, path: &str) -> Result<i64> {
        self.adopt_or_register(project_id, name, path, None, Acquisition::Registered)
    }

    /// Takes over an existing folder as scratchpad. Adoption is no cleanup
    /// approval: the resource stays protected.
    pub fn adopt_scratch(&mut self, project_id: i64, name: &str, path: &str, purpose: Option<&str>) -> Result<i64> {
        self.adopt_or_register(project_id, name, path, purpose, Acquisition::Adopted)
    }

    fn adopt_or_register(
        &mut self,
        project_id: i64,
        name: &str,
        path: &str,
        purpose: Option<&str>,
        acquisition: Acquisition,
    ) -> Result<i64> {
        self.require_local()?;
        let name = validation::name(name)?;
        let path = validation::windows_path(path)?;
        let purpose = purpose.map(privacy::label);
        let now = self.now();
        self.write(|tx| {
            require_project(tx, project_id)?;
            let existing: Option<(i64, Option<i64>)> = tx
                .query_row("SELECT id, project_id FROM resources WHERE path = ?1", [&path], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            if let Some((id, owner)) = existing {
                if owner != Some(project_id) {
                    return Err(Error::conflict("Dieser Pfad hat bereits einen anderen registrierten Besitzer."));
                }
                return Ok(id);
            }
            check_capacity(tx, 1)?;
            let evidence = match acquisition {
                Acquisition::Adopted => "Bestehender Ordner übernommen; Inhalt nicht gelesen, Schutz aktiv.",
                _ => "Explizit registriert; keine Nutzung oder Entbehrlichkeit nachgewiesen.",
            };
            tx.execute(
                "INSERT INTO resources(project_id, name, path, kind, evidence, origin, acquisition, purpose, created_at)
                 VALUES(?1, ?2, ?3, 'scratchpad', ?4, 'registered', ?5, ?6, ?7)",
                params![project_id, name, path, evidence, acquisition, purpose, now],
            )?;
            let id = tx.last_insert_rowid();
            add_owner(tx, id, project_id, OwnerKind::Explicit, "Bei der Registrierung ausdrücklich angegeben.")?;
            refresh_inferred_owners(tx)?;
            let title =
                if acquisition == Acquisition::Adopted { "Scratchpad übernommen" } else { "Scratchpad registriert" };
            log_in(tx, now, "registration", title, "Besitzer gespeichert; Inhalt nicht gelesen oder verändert.")?;
            Ok(id)
        })
    }

    /// Records a managed scratchpad whose folders mogumogu has just created.
    pub fn record_managed_scratch(
        &mut self,
        project_id: i64,
        name: &str,
        purpose: &str,
        root: &ManagedFolder,
        children: &[ManagedFolder],
        review_days: i64,
    ) -> Result<i64> {
        self.require_local()?;
        let name = validation::name(name)?;
        let purpose = privacy::label(purpose);
        let now = self.now();
        self.write(|tx| {
            require_project(tx, project_id)?;
            check_capacity(tx, 1 + children.len() as i64)?;
            tx.execute(
                "INSERT INTO resources(project_id, name, path, kind, evidence, origin, acquisition, purpose,
                                       review_at, created_at, identity)
                 VALUES(?1, ?2, ?3, 'scratchpad', ?4, 'registered', 'managed', ?5, ?6, ?7, ?8)",
                params![
                    project_id,
                    name,
                    root.path,
                    "Von mogumogu angelegt; Quelle, Umgebung, Temporär und Ergebnisse getrennt.",
                    purpose,
                    now + review_days * DAY,
                    now,
                    root.identity.to_string()
                ],
            )?;
            let scratch_id = tx.last_insert_rowid();
            add_owner(
                tx,
                scratch_id,
                project_id,
                OwnerKind::Explicit,
                "Scratchpad über mogumogu für dieses Projekt angelegt.",
            )?;
            for child in children {
                let evidence = match child.kind {
                    ResourceKind::Temporary => {
                        "Verwalteter Temp-Ordner; erst nach ausdrücklicher Freigabe entbehrlich."
                    }
                    ResourceKind::Results => "Ergebnisse und Diagnosen bleiben erhalten.",
                    ResourceKind::Source => "Quellcode und Notizen bleiben erhalten.",
                    _ => "Separat prüfen und klassifizieren.",
                };
                tx.execute(
                    "INSERT INTO resources(project_id, name, path, kind, evidence, origin, acquisition, parent_id,
                                           created_at, identity)
                     VALUES(?1, ?2, ?3, ?4, ?5, 'registered', 'managed', ?6, ?7, ?8)",
                    params![
                        project_id,
                        format!("{name} · {}", child.kind.label()),
                        child.path,
                        child.kind,
                        evidence,
                        scratch_id,
                        now,
                        child.identity.to_string()
                    ],
                )?;
                let child_id = tx.last_insert_rowid();
                add_owner(tx, child_id, project_id, OwnerKind::Explicit, "Teil eines verwalteten Scratchpads.")?;
            }
            log_in(
                tx,
                now,
                "scratchpad",
                "Scratchpad angelegt",
                &format!("{name} · Prüftermin in {review_days} Tagen"),
            )?;
            Ok(scratch_id)
        })
    }

    pub fn resources(&mut self) -> Result<Vec<Resource>> {
        let sql = format!("SELECT {COLUMNS} FROM resources ORDER BY id LIMIT {}", limits::MAX_RESOURCES);
        self.read(|tx| {
            let mut statement = tx.prepare(&sql)?;
            let rows = statement.query_map([], map_resource)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn resource(&mut self, id: i64) -> Result<Resource> {
        let sql = format!("SELECT {COLUMNS} FROM resources WHERE id = ?1");
        self.read(|tx| {
            tx.query_row(&sql, [id], map_resource)
                .optional()?
                .ok_or_else(|| Error::not_found("Ressourcen-ID nicht gefunden."))
        })
    }

    pub fn children(&mut self, parent_id: i64) -> Result<Vec<Resource>> {
        let sql = format!("SELECT {COLUMNS} FROM resources WHERE parent_id = ?1 ORDER BY id");
        self.read(|tx| {
            let mut statement = tx.prepare(&sql)?;
            let rows = statement.query_map([parent_id], map_resource)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Stored identity of a folder mogumogu created.
    pub fn resource_identity(&mut self, id: i64) -> Result<Option<FileIdentity>> {
        let text: Option<String> = self.read(|tx| {
            Ok(tx.query_row("SELECT identity FROM resources WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten())
        })?;
        text.map(|t| t.parse()).transpose()
    }

    /// Protection always wins; protecting also withdraws expendability.
    pub fn protect(&mut self, id: i64) -> Result<()> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            let changed = tx.execute("UPDATE resources SET protected = 1, expendable = 0 WHERE id = ?1", [id])?;
            if changed == 0 {
                return Err(Error::not_found("Ressourcen-ID nicht gefunden."));
            }
            log_in(tx, now, "protection", "Ressource geschützt", "Schutz gewinnt gegenüber jeder Prüfregel.")
        })
    }

    /// Explicit local decision that a *managed temporary* folder may be
    /// emptied by a later, separately approved plan. Nothing else qualifies.
    pub fn mark_expendable(&mut self, id: i64) -> Result<()> {
        self.require_local()?;
        let resource = self.resource(id)?;
        if resource.kind != ResourceKind::Temporary || resource.acquisition != Acquisition::Managed {
            return Err(Error::blocked(
                "Nur verwaltete Temp-Ordner können als entbehrlich markiert werden. Quellen, Ergebnisse, \
                 Umgebungen, KI-Konfiguration und unbekannte Inhalte bleiben geschützt.",
            ));
        }
        let now = self.now();
        self.write(|tx| {
            tx.execute("UPDATE resources SET protected = 0, expendable = 1 WHERE id = ?1", [id])?;
            log_in(
                tx,
                now,
                "protection",
                "Temp-Ordner als entbehrlich markiert",
                "Lokale Entscheidung; ein Plan muss separat bestätigt werden.",
            )
        })
    }

    pub fn extend_review(&mut self, id: i64, days: i64) -> Result<Timestamp> {
        self.require_local()?;
        if !(1..=365).contains(&days) {
            return Err(Error::invalid("Verlängerung: 1–365 Tage."));
        }
        let now = self.now();
        let resource = self.resource(id)?;
        if resource.kind != ResourceKind::Scratchpad {
            return Err(Error::invalid("Prüftermine gelten für Scratchpads."));
        }
        let next = resource.review_at.unwrap_or(now).max(now) + days * DAY;
        self.write(|tx| {
            tx.execute("UPDATE resources SET review_at = ?1 WHERE id = ?2", params![next, id])?;
            log_in(tx, now, "scratchpad", "Prüftermin verlängert", &format!("Um {days} Tage verschoben."))
        })?;
        Ok(next)
    }

    /// Turns a scratchpad into a project: removes the review date, keeps the
    /// history and keeps protection.
    pub fn promote(&mut self, id: i64) -> Result<i64> {
        self.require_local()?;
        let resource = self.resource(id)?;
        if resource.kind != ResourceKind::Scratchpad {
            return Err(Error::invalid("Nur Scratchpads können zum Projekt gemacht werden."));
        }
        let now = self.now();
        self.write(|tx| {
            let existing: Option<i64> =
                tx.query_row("SELECT id FROM projects WHERE path = ?1", [&resource.path], |r| r.get(0)).optional()?;
            let project_id = match existing {
                Some(project) => project,
                None => {
                    tx.execute(
                        "INSERT INTO projects(name, path, origin, evidence) VALUES(?1, ?2, 'registered', ?3)",
                        params![
                            resource.name,
                            resource.path,
                            "Aus Scratchpad hervorgegangen; Historie bleibt erhalten."
                        ],
                    )?;
                    tx.last_insert_rowid()
                }
            };
            tx.execute("UPDATE resources SET review_at = NULL, protected = 1 WHERE id = ?1", [id])?;
            add_owner(tx, id, project_id, OwnerKind::Explicit, "Scratchpad wurde zu diesem Projekt befördert.")?;
            log_in(
                tx,
                now,
                "scratchpad",
                "Scratchpad zum Projekt gemacht",
                "Ablaufregel entfernt; Historie und Schutz bleiben.",
            )?;
            Ok(project_id)
        })
    }

    pub fn record_size(&mut self, id: i64, bytes: Option<i64>) -> Result<()> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            tx.execute("UPDATE resources SET bytes = ?1, measured_at = ?2 WHERE id = ?3", params![bytes, now, id])?;
            Ok(())
        })
    }

    pub fn touch_activity(&mut self, id: i64) -> Result<()> {
        let now = self.now();
        self.write(|tx| {
            tx.execute(
                "UPDATE resources SET last_activity_at = ?1 WHERE id = ?2 OR id = (SELECT parent_id FROM resources WHERE id = ?2)",
                params![now, id],
            )?;
            Ok(())
        })
    }

    pub fn owners(&mut self) -> Result<Vec<OwnerLink>> {
        self.read(|tx| {
            let mut statement = tx.prepare(
                "SELECT o.resource_id, o.project_id, p.name, o.kind, o.evidence
                 FROM owner_links o JOIN projects p ON p.id = o.project_id
                 ORDER BY o.resource_id, o.kind, p.name LIMIT 20000",
            )?;
            let rows = statement.query_map([], |r| {
                Ok(OwnerLink {
                    resource_id: r.get(0)?,
                    project_id: r.get(1)?,
                    project_name: r.get(2)?,
                    kind: r.get(3)?,
                    evidence: r.get(4)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }
}

pub(crate) fn add_owner(
    tx: &Transaction<'_>,
    resource_id: i64,
    project_id: i64,
    kind: OwnerKind,
    evidence: &str,
) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO owner_links(resource_id, project_id, kind, evidence) VALUES(?1, ?2, ?3, ?4)",
        params![resource_id, project_id, kind, evidence],
    )?;
    Ok(())
}
