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
        let mut report = finish_report(&tidy, &history, &holds, runs, self.repo.now());
        report.finished_at = self.repo.lifecycle(project_id)?.filter(|e| e.state == Lifecycle::Finished).map(|e| e.at);
        Ok(report)
    }

    /// Finishes a project after the person confirmed its report. The report
    /// is made again: a changed point or folder needs a new confirmation.
    /// Changes no protection, no hold and nothing on disk. Only the
    /// dashboard asks for this (the owner refuses it over the pipe).
    pub fn finish_project(&mut self, project_id: i64, confirmation: &str) -> Result<FinishReport> {
        self.repo.require_local()?;
        let mut report = self.finish_report(project_id)?;
        if report.finished_at.is_some() {
            return Err(Error::blocked("Das Projekt ist schon abgeschlossen."));
        }
        if confirmation != report.fingerprint {
            return Err(Error::blocked(
                "Seit dem Bericht hat sich etwas geändert. Bitte den neuen Abschlussbericht prüfen und dann abschließen.",
            ));
        }
        self.repo.record_lifecycle(project_id, Lifecycle::Finished, Some(&report.fingerprint))?;
        report.finished_at = self.repo.lifecycle(project_id)?.map(|e| e.at);
        Ok(report)
    }

    /// Makes a finished project active again; nothing else changes.
    pub fn reactivate_project(&mut self, project_id: i64) -> Result<()> {
        self.repo.require_local()?;
        self.repo.record_lifecycle(project_id, Lifecycle::Active, None)
    }

    /// The project's changes, newest first.
    pub fn lifecycle_history(&mut self, project_id: i64) -> Result<Vec<LifecycleEvent>> {
        self.repo.project(project_id)?;
        self.repo.lifecycle_history(project_id)
    }
}
