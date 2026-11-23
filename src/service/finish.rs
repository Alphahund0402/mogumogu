//! The finishing report of a project (docs/LEBENSZYKLUS.md, step 1). It
//! measures the project anew, like a recycle-bin plan, and changes nothing
//! but the saved report history that every tidy-up report adds to.
use super::Core;
use crate::domain::*;
use crate::{Error, Result};

/// Unreleased runs named in the report.
const RUNS_IN_REPORT: i64 = 50;

impl Core {
    pub fn finish_report(&mut self, project_id: i64) -> Result<FinishReport> {
        let started = self.repo.now();
        let (report, held) = self.measure_tidy(Some(project_id))?;
        // A cancelled walk leaves the previous report in place: never use it.
        let tidy = report
            .projects
            .into_iter()
            .find(|p| p.project_id == project_id && p.checked_at >= started)
            .ok_or_else(|| Error::blocked("Erfassung abgebrochen; ohne frischen Bericht kein Abschlussbericht."))?;
        // What could not be determined holds the whole project, as in the report.
        let mut holds = held.holds;
        holds.extend(held.unknown.into_iter().map(|reason| Hold {
            kind: HoldKind::Docker,
            location: tidy.path.clone(),
            reason,
        }));
        let history = self.repo.tidy_history(project_id)?;
        let runs = self
            .repo
            .runs(Some(project_id), RUNS_IN_REPORT, 0)?
            .into_iter()
            .filter(|run| run.released_at.is_none())
            .map(|run| RunBrief {
                id: run.id,
                title: run.title(),
                started_at: run.started_at,
                artifacts: run.artifact_count,
            })
            .collect();
        Ok(finish_report(&tidy, &history, &holds, runs, self.repo.now()))
    }
}
