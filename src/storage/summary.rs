//! Small aggregate read model for status; never loads inventory or UI pages.
use super::Repository;
use crate::Result;
use crate::clock::Timestamp;

#[derive(Debug)]
pub(crate) struct StatusCounts {
    pub projects: usize,
    pub resources: usize,
    pub scopes: usize,
}

impl Repository {
    /// Finish time of the newest complete generation of an approved scope.
    pub(crate) fn last_complete_scan(&mut self) -> Result<Option<Timestamp>> {
        self.read(|tx| {
            Ok(tx.query_row(
                "SELECT MAX(g.finished_at) FROM generations g JOIN scopes s ON s.id = g.scope_id
                 WHERE g.state = 'complete' AND s.status = 'approved'",
                [],
                |r| r.get(0),
            )?)
        })
    }

    /// Totals in one read transaction, independent of dashboard paging.
    pub(crate) fn status_counts(&mut self) -> Result<StatusCounts> {
        self.read(|tx| {
            Ok(tx.query_row(
                "SELECT (SELECT COUNT(*) FROM projects),
                        (SELECT COUNT(*) FROM resources),
                        (SELECT COUNT(*) FROM scopes)",
                [],
                |row| {
                    let count = |column| {
                        let value = row.get::<_, i64>(column)?;
                        usize::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
                    };
                    Ok(StatusCounts { projects: count(0)?, resources: count(1)?, scopes: count(2)? })
                },
            )?)
        })
    }
}
