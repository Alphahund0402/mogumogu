//! Cleanup plan requests, settings and backups.
//! Cleanup authorization and execution remain in `crate::cleanup`.
use super::Core;
use crate::cleanup::{self, Candidate, DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT, NoHooks};
use crate::domain::{PlanState, PlanSummary, ScopeKind, ScopeStatus, Settings, TidyClass};
use crate::fsread::{self, Budget, ScopeRoot};
use crate::limits::{CLEANUP_MAX_TARGETS, ScanLimits};
use crate::platform;
use crate::{Error, Result, validation};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

impl Core {
    /// Drafts a plan that moves recoverable folders of one approved project
    /// to the recycle bin: the largest ones, or exactly `paths`. The project
    /// is walked again first; an older report never decides.
    pub fn cleanup_plan(&mut self, project_id: i64, paths: &[String]) -> Result<PlanSummary> {
        let report = self.tidy_report(Some(project_id))?;
        let project = report
            .projects
            .iter()
            .find(|p| p.project_id == project_id)
            .ok_or_else(|| Error::not_found("Projekt nicht gemessen."))?;
        let recoverable: Vec<_> = project.items.iter().filter(|i| i.class == TidyClass::Regenerable).collect();
        let missing: Vec<&str> =
            paths.iter().filter(|p| !recoverable.iter().any(|i| &i.rel_path == *p)).map(String::as_str).collect();
        if !missing.is_empty() {
            return Err(Error::blocked(format!("Nicht als wiederherstellbar belegt: {}", missing.join(", "))));
        }
        let candidates: Vec<Candidate> = recoverable
            .iter()
            .filter(|i| paths.is_empty() || paths.contains(&i.rel_path))
            .take(CLEANUP_MAX_TARGETS)
            .map(|i| Candidate { rel_path: i.rel_path.clone(), bytes: i.bytes })
            .collect();
        if candidates.is_empty() {
            return Err(Error::blocked("In diesem Projekt ist nichts als wiederherstellbar belegt."));
        }
        let scope = self
            .repo
            .scopes()?
            .into_iter()
            .find(|s| {
                s.project_id == Some(project_id) && s.kind == ScopeKind::Project && s.status == ScopeStatus::Approved
            })
            .ok_or_else(|| Error::blocked("Für dieses Projekt fehlt eine Lesefreigabe."))?;
        let id = cleanup::draft(&mut self.repo, &scope, &candidates)?;
        cleanup::summary(&mut self.repo, id)
    }

    pub fn cleanup_approve(&mut self, plan_id: i64, confirmation: &str) -> Result<PlanSummary> {
        cleanup::approve(&mut self.repo, plan_id, confirmation)?;
        cleanup::summary(&mut self.repo, plan_id)
    }

    pub fn cleanup_apply(&mut self, plan_id: i64) -> Result<PlanSummary> {
        let applied = cleanup::apply(&mut self.repo, plan_id, self.recycler.as_mut(), &mut NoHooks);
        let summary = cleanup::summary(&mut self.repo, plan_id)?;
        applied.map(|_| summary)
    }

    pub fn cleanup_cancel(&mut self, plan_id: i64) -> Result<PlanSummary> {
        let plan = cleanup::summary(&mut self.repo, plan_id)?;
        if !matches!(plan.state, PlanState::Draft | PlanState::Approved) {
            return Err(Error::conflict(
                "Nur Entwürfe und bestätigte, noch nicht gestartete Pläne können abgebrochen werden.",
            ));
        }
        self.repo.transition_plan(plan_id, plan.state, PlanState::Cancelled, Some("Lokal abgebrochen"))?;
        cleanup::summary(&mut self.repo, plan_id)
    }

    pub fn cleanup_reconcile(&mut self, plan_id: i64) -> Result<PlanSummary> {
        cleanup::reconcile(&mut self.repo, plan_id)?;
        cleanup::summary(&mut self.repo, plan_id)
    }

    /// Registers an explicitly disposable test root. It must carry the
    /// marker file and must not contain or lie inside a registered project.
    pub fn register_disposable_root(&mut self, path: &str) -> Result<i64> {
        self.repo.require_local()?;
        let path = validation::windows_path(path)?;
        // Read like every later check: from the folder's own handle, never
        // through a link.
        let marker = ScopeRoot::open(Path::new(&path), None)
            .ok()
            .and_then(|root| {
                let mut budget = Budget::new(ScanLimits::default());
                fsread::read_text(root.handle(), OsStr::new(DISPOSABLE_MARKER), 4096, &mut budget).ok()
            })
            .unwrap_or_default();
        if marker.trim() != DISPOSABLE_MARKER_TEXT {
            return Err(Error::blocked(format!(
                "Die Datei {DISPOSABLE_MARKER} mit dem Inhalt „{DISPOSABLE_MARKER_TEXT}“ fehlt. Nur ausdrücklich \
                 markierte Wegwerf-Ordner sind zulässig."
            )));
        }
        let lower = path.to_lowercase();
        for project in self.repo.projects()? {
            let project_path = project.path.to_lowercase();
            if project_path.starts_with(&format!("{lower}\\"))
                || lower.starts_with(&format!("{project_path}\\"))
                || project_path == lower
            {
                return Err(Error::blocked(
                    "Eine Testwurzel darf kein registriertes Projekt enthalten oder in einem liegen.",
                ));
            }
        }
        let identity = platform::path_identity(Path::new(&path))?;
        self.repo.register_disposable_root(&path, identity)
    }

    pub fn settings(&mut self) -> Result<Settings> {
        if self.is_demo() {
            return Ok(Settings::default());
        }
        self.repo.settings()
    }

    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        self.repo.save_settings(settings)
    }

    pub fn backup(&mut self, path: &str) -> Result<String> {
        let target = PathBuf::from(path);
        if !target.is_absolute() {
            return Err(Error::invalid("Sicherungspfad muss absolut sein."));
        }
        self.repo.backup_to(&target)?;
        self.repo.log("backup", "Sicherung erstellt", "Konsistente Kopie über die SQLite-Backup-API.")?;
        Ok(target.to_string_lossy().into_owned())
    }

    /// Starting with Windows was removed. An entry this owner wrote earlier
    /// would keep starting it at sign-in with no way to switch it off, so it
    /// is removed once; an entry that starts anything else stays untouched.
    pub(super) fn retire_autostart(&mut self) {
        if self.is_demo() {
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        let own = platform::autostart_command(&exe, self.config.custom_directory(), false).ok();
        let found = platform::autostart_entry().ok().flatten();
        if found.zip(own).is_some_and(|(found, own)| found.eq_ignore_ascii_case(&own))
            && platform::remove_autostart().is_ok()
        {
            let _ = self.repo.log("settings", "Start bei Anmeldung entfernt", "Die Funktion gibt es nicht mehr.");
        }
    }
}
