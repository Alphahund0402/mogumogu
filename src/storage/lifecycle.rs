//! Where a project stands: active or finished, as the person decided
//! (docs/LEBENSZYKLUS.md). One row per change, the newest [`KEEP`] per project;
//! the newest row is the state and no row means active. Nothing here changes
//! protection or anything on disk.
use super::Repository;
use super::activity::log_in;
use crate::domain::{Lifecycle, LifecycleEvent};
use crate::{Error, Result};
use rusqlite::{OptionalExtension, params};

/// Changes kept per project.
const KEEP: i64 = 50;

impl Repository {
    /// The project's newest change, `None` while it was never finished.
    pub fn lifecycle(&mut self, project_id: i64) -> Result<Option<LifecycleEvent>> {
        Ok(self.lifecycle_history(project_id)?.into_iter().next())
    }

    /// The project's changes, newest first.
    pub fn lifecycle_history(&mut self, project_id: i64) -> Result<Vec<LifecycleEvent>> {
        self.query(
            "SELECT state, at FROM project_lifecycle WHERE project_id = ?1 ORDER BY id DESC",
            [project_id],
            |r| Ok(LifecycleEvent { state: r.get(0)?, at: r.get(1)? }),
        )
    }

    /// Records a change. A finish carries the fingerprint of the confirmed
    /// report; the same state twice in a row is refused.
    pub fn record_lifecycle(&mut self, project_id: i64, state: Lifecycle, fingerprint: Option<&str>) -> Result<()> {
        self.require_local()?;
        if (state == Lifecycle::Finished) != fingerprint.is_some() {
            return Err(Error::invalid("Abschließen braucht den bestätigten Bericht, Reaktivieren keinen."));
        }
        let now = self.now();
        self.write(|tx| {
            let name: Option<String> = tx
                .query_row("SELECT name FROM projects WHERE id = ?1 AND origin = 'registered'", [project_id], |r| {
                    r.get(0)
                })
                .optional()?;
            let Some(name) = name else {
                return Err(Error::NotFound(format!("Projekt {project_id}")));
            };
            let current: Option<Lifecycle> = tx
                .query_row(
                    "SELECT state FROM project_lifecycle WHERE project_id = ?1 ORDER BY id DESC LIMIT 1",
                    [project_id],
                    |r| r.get(0),
                )
                .optional()?;
            if current.unwrap_or(Lifecycle::Active) == state {
                return Err(Error::blocked(match state {
                    Lifecycle::Finished => "Das Projekt ist schon abgeschlossen.",
                    Lifecycle::Active => "Das Projekt ist nicht abgeschlossen.",
                }));
            }
            tx.execute(
                "INSERT INTO project_lifecycle(project_id, state, at, fingerprint) VALUES(?1, ?2, ?3, ?4)",
                params![project_id, state, now, fingerprint],
            )?;
            tx.execute(
                "DELETE FROM project_lifecycle WHERE project_id = ?1 AND id NOT IN
                    (SELECT id FROM project_lifecycle WHERE project_id = ?1 ORDER BY id DESC LIMIT ?2)",
                params![project_id, KEEP],
            )?;
            let title = match state {
                Lifecycle::Finished => "Projekt abgeschlossen",
                Lifecycle::Active => "Projekt reaktiviert",
            };
            log_in(tx, now, "project", title, &format!("{name}; Schutz unverändert."))?;
            Ok(())
        })
    }
}
