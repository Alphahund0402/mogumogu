//! Cleanup plan requests, settings, update hints, exports and backups.
//! Cleanup authorization and execution remain in `crate::cleanup`.
use super::Core;
use crate::cleanup::{self, DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT, NoHooks};
use crate::domain::{Autostart, ItemCategory, PlanState, PlanSummary, Settings};
use crate::platform;
use crate::updates::{self, UpdateSummary};
use crate::{Error, Result, validation};
use std::io::Write;
use std::path::{Path, PathBuf};

impl Core {
    pub fn cleanup_plan(&mut self, resource_ids: &[i64]) -> Result<PlanSummary> {
        self.reconcile_sessions()?;
        let id = cleanup::draft(&mut self.repo, resource_ids)?;
        cleanup::summary(&mut self.repo, id)
    }

    pub fn cleanup_approve(&mut self, plan_id: i64, confirmation: &str) -> Result<PlanSummary> {
        cleanup::approve(&mut self.repo, plan_id, confirmation)?;
        cleanup::summary(&mut self.repo, plan_id)
    }

    pub fn cleanup_apply(&mut self, plan_id: i64) -> Result<PlanSummary> {
        self.reconcile_sessions()?;
        let applied = cleanup::apply(&mut self.repo, plan_id, &mut NoHooks);
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
        let marker = std::fs::read_to_string(Path::new(&path).join(DISPOSABLE_MARKER)).unwrap_or_default();
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

    pub fn check_updates(&mut self) -> Result<UpdateSummary> {
        self.repo.require_local()?;
        let settings = self.repo.settings()?;
        if !settings.network_updates_enabled {
            return Err(Error::blocked("Updatehinweise sind ausgeschaltet. In den Einstellungen bewusst aktivieren."));
        }
        updates::run(&mut self.repo, self.registry.as_ref(), &settings)
    }

    pub fn update_records(&mut self) -> Result<Vec<crate::storage::UpdateRecord>> {
        let now = self.repo.now();
        let mut records = self.repo.update_records()?;
        for record in &mut records {
            record.status = updates::effective_status(record, now);
        }
        Ok(records)
    }

    pub fn export(&mut self, include_paths: bool) -> Result<serde_json::Value> {
        let snapshot = self.snapshot()?;
        let packages = self.repo.published_items_all(Some(ItemCategory::Package), 100_000)?;
        Ok(crate::export::redacted(&snapshot, &packages, include_paths))
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

    /// Writes the redacted export into a new file; an existing file is never
    /// overwritten. The dashboard shows what is left out before it calls this.
    pub fn export_to(&mut self, path: &str, include_paths: bool) -> Result<String> {
        let target = PathBuf::from(validation::windows_path(path)?);
        let value = self.export(include_paths)?;
        let text = serde_json::to_string_pretty(&value)?;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::conflict("Datei existiert bereits; nichts überschrieben.")
            } else {
                Error::from(e)
            }
        })?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        let note = if include_paths {
            "Mit Pfaden und Namen – nur bewusst teilen."
        } else {
            "Ohne Pfade und private Namen."
        };
        self.repo.log("export", "Export geschrieben", note)?;
        Ok(target.to_string_lossy().into_owned())
    }

    /// The per-user start entry as found; `None` where it cannot be read.
    pub fn autostart(&self) -> Option<Autostart> {
        let entry = platform::autostart_entry().ok()?;
        let own = std::env::current_exe()
            .ok()
            .and_then(|exe| platform::autostart_command(&exe, self.config.custom_directory(), self.is_demo()).ok());
        let current = entry.as_deref().zip(own.as_deref()).is_some_and(|(found, own)| found.eq_ignore_ascii_case(own));
        Some(Autostart { enabled: entry.is_some(), current })
    }

    /// Starts this owner in the tray at sign-in, or stops doing so. Only the
    /// owner itself is registered, for the current user; no service, no task.
    pub fn set_autostart(&mut self, enabled: bool) -> Result<Autostart> {
        self.repo.require_local()?;
        let exe = std::env::current_exe()?;
        platform::set_autostart(&exe, self.config.custom_directory(), false, enabled)
            .map_err(|e| Error::blocked(format!("Starteintrag nicht geändert: {e}")))?;
        let title = if enabled { "Start bei Anmeldung eingeschaltet" } else { "Start bei Anmeldung ausgeschaltet" };
        self.repo.log("settings", title, "Nur der eigene Tray-Besitzer, nur für diesen Benutzer.")?;
        self.autostart().ok_or_else(|| Error::unsupported("Starteintrag nicht lesbar."))
    }
}
