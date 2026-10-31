//! Saved tidy-up reports per project, to show what grows. Only sizes and the
//! paths of the largest entries are kept; the last [`KEEP`] saved per project
//! (by order of saving, so a clock set wrong once does not freeze them).
use super::Repository;
use crate::domain::{TidyClass, TidyPoint};
use crate::{Result, privacy};
use rusqlite::params;

/// Saved reports per project.
const KEEP: i64 = 30;

fn clamp(bytes: u64) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}

impl Repository {
    pub fn record_tidy(&mut self, project_id: i64, point: &TidyPoint) -> Result<()> {
        self.require_local()?;
        let items: Vec<(String, TidyClass, u64)> =
            point.items.iter().map(|(rel, class, bytes)| (privacy::label(rel), *class, *bytes)).collect();
        let items = serde_json::to_string(&items)?;
        self.write(|tx| {
            tx.execute(
                "INSERT INTO tidy_history(project_id, checked_at, complete, total, regenerable, review, vcs, test_data,
                                          agent, items)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    project_id,
                    point.checked_at,
                    point.complete,
                    point.total.map(clamp),
                    clamp(point.regenerable),
                    clamp(point.review),
                    clamp(point.vcs),
                    clamp(point.test_data),
                    clamp(point.agent),
                    items
                ],
            )?;
            tx.execute(
                "DELETE FROM tidy_history WHERE project_id = ?1 AND id NOT IN
                    (SELECT id FROM tidy_history WHERE project_id = ?1 ORDER BY id DESC LIMIT ?2)",
                params![project_id, KEEP],
            )?;
            Ok(())
        })
    }

    /// Saved reports of a project, oldest first.
    pub fn tidy_history(&mut self, project_id: i64) -> Result<Vec<TidyPoint>> {
        let rows: Vec<(TidyPoint, String)> = self.query(
            "SELECT checked_at, complete, total, regenerable, review, vcs, test_data, agent, items
             FROM tidy_history WHERE project_id = ?1 ORDER BY checked_at, id",
            [project_id],
            |r| {
                let size = |i: usize| r.get::<_, i64>(i).map(|b| b.max(0) as u64);
                Ok((
                    TidyPoint {
                        checked_at: r.get(0)?,
                        complete: r.get(1)?,
                        total: r.get::<_, Option<i64>>(2)?.map(|b| b.max(0) as u64),
                        regenerable: size(3)?,
                        review: size(4)?,
                        vcs: size(5)?,
                        test_data: size(6)?,
                        agent: size(7)?,
                        items: Vec::new(),
                    },
                    r.get(8)?,
                ))
            },
        )?;
        // A damaged row loses its entries, not the whole history.
        Ok(rows
            .into_iter()
            .map(|(point, items)| TidyPoint { items: serde_json::from_str(&items).unwrap_or_default(), ..point })
            .collect())
    }
}
