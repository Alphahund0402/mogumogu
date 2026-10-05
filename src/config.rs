//! Where mogumogu keeps its own data. Project files are never stored here.
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::{env, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub directory: PathBuf,
    pub demo: bool,
}

impl Config {
    pub fn new(directory: Option<PathBuf>, demo: bool) -> Result<Self> {
        let directory = match directory {
            Some(path) if path.is_absolute() => path,
            Some(_) => return Err(Error::invalid("--data-dir muss absolut sein.")),
            None => default_directory()?,
        };
        Ok(Self { directory, demo })
    }

    /// Demo and local inventories are separate files and never mixed.
    pub fn database_path(&self) -> PathBuf {
        self.directory.join(if self.demo { "demo.sqlite3" } else { "inventory.sqlite3" })
    }

    /// One owner process per data directory (desktop or headless).
    pub fn instance_path(&self) -> PathBuf {
        self.directory.join("owner.lock")
    }

    /// Consistent SQLite backups before migrations and on request.
    pub fn backup_directory(&self) -> PathBuf {
        self.directory.join("backups")
    }

    /// Per-user, per-data-directory pipe name. The user SID is part of the
    /// hash so two accounts never meet on the same name.
    pub fn pipe_name(&self) -> Result<String> {
        let sid = crate::platform::current_user_sid()?;
        let mut hasher = Sha256::new();
        hasher.update(sid.as_bytes());
        hasher.update(b"|");
        hasher.update(self.directory.to_string_lossy().to_lowercase().as_bytes());
        hasher.update(if self.demo { b"|demo" as &[u8] } else { b"|local" });
        let digest = hasher.finalize();
        let short: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
        Ok(format!(r"\\.\pipe\mogumogu-{short}"))
    }

    /// Job-object namespace for managed runs of this data directory.
    pub fn job_prefix(&self) -> Result<String> {
        let pipe = self.pipe_name()?;
        let suffix = pipe.rsplit('-').next().unwrap_or("x");
        Ok(format!(r"Local\mogumogu-{suffix}-session-"))
    }
}

fn default_directory() -> Result<PathBuf> {
    #[cfg(windows)]
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| Error::invalid("LOCALAPPDATA ist nicht gesetzt."))?;
    #[cfg(not(windows))]
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|v| PathBuf::from(v).join(".local/share")))
        .ok_or_else(|| Error::invalid("Kein lokales Datenverzeichnis vorhanden."))?;
    Ok(base.join("mogumogu"))
}
