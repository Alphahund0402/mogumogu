//! Local settings as one validated JSON document.
use super::Repository;
use super::activity::log_in;
use crate::domain::Settings;
use crate::{Error, Result};
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
    if !(1..=365).contains(&settings.scratch_review_days) {
        return Err(Error::invalid("Prüfzeitraum: 1–365 Tage."));
    }
    Ok(())
}
