//! Bounded, redacted activity history.
use super::Repository;
use crate::clock::{Timestamp, iso_utc};
use crate::domain::Activity;
use crate::{Result, limits, privacy};
use rusqlite::{Transaction, params};

impl Repository {
    pub fn log(&mut self, kind: &str, title: &str, detail: &str) -> Result<()> {
        let now = self.now();
        self.write(|tx| log_in(tx, now, kind, title, detail))
    }

    pub fn activity(&mut self, limit: i64) -> Result<Vec<Activity>> {
        self.read(|tx| {
            let mut statement =
                tx.prepare("SELECT title, detail, created_at, kind FROM activity ORDER BY id DESC LIMIT ?1")?;
            let rows = statement.query_map([limit], |r| {
                Ok(Activity { title: r.get(0)?, detail: r.get(1)?, created_at: r.get(2)?, kind: r.get(3)? })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }
}

/// Appends one entry inside an existing transaction and prunes the history.
pub(crate) fn log_in(tx: &Transaction<'_>, now: Timestamp, kind: &str, title: &str, detail: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO activity(title, detail, created_at, kind) VALUES(?1, ?2, ?3, ?4)",
        params![privacy::label(title), privacy::label(detail), iso_utc(now), kind],
    )?;
    tx.execute(
        "DELETE FROM activity WHERE id NOT IN (SELECT id FROM activity ORDER BY id DESC LIMIT ?1)",
        [limits::ACTIVITY_KEEP],
    )?;
    Ok(())
}
