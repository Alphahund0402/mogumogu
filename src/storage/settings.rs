//! Local settings as one validated JSON document.
use super::Repository;
use super::activity::log_in;
use crate::domain::{PUBLIC_UPDATE_SOURCES, Settings};
use crate::{Error, Result, validation};
use rusqlite::OptionalExtension;

impl Repository {
    pub fn settings(&mut self) -> Result<Settings> {
        let stored: Option<String> = self.read(|tx| {
            Ok(tx.query_row("SELECT value FROM settings WHERE key = 'settings'", [], |r| r.get(0)).optional()?)
        })?;
        match stored {
            Some(json) => Ok(serde_json::from_str(&json)?),
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        self.require_local()?;
        validate(settings)?;
        let json = serde_json::to_string(settings)?;
        let now = self.now();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO settings(key, value) VALUES('settings', ?1)
                        ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [json],
            )?;
            log_in(
                tx,
                now,
                "settings",
                "Einstellungen gespeichert",
                "Lokale Entscheidung; gilt nur auf diesem Rechner.",
            )
        })
    }
}

fn validate(settings: &Settings) -> Result<()> {
    if let Some(unknown) = settings.update_sources.iter().find(|s| !PUBLIC_UPDATE_SOURCES.contains(&s.as_str())) {
        return Err(Error::invalid(format!("Unbekannte Updatequelle: {unknown}")));
    }
    if let Some(root) = &settings.scratch_root {
        validation::windows_path(root)?;
    }
    for days in [settings.scratch_review_days, settings.environment_review_days, settings.global_review_days] {
        if !(1..=365).contains(&days) {
            return Err(Error::invalid("Prüfzeiträume: 1–365 Tage."));
        }
    }
    Ok(())
}
