//! SQLite persistence. One connection per process, one coordinated writer
//! (the owner), short immediate transactions, `synchronous=FULL`.
//!
//! The repository is split by area; each submodule adds an `impl Repository`
//! block. SQL never leaves this module and never runs in UI callbacks.
mod activity;
mod cleanup;
mod demo;
mod generations;
mod migrations;
mod projects;
mod resources;
mod scopes;
mod sessions;
mod settings;
mod updates;

pub use cleanup::{JournalEntry, ManifestEntry, NewTarget, StoredPlan, StoredTarget};
pub use demo::FIXTURE as DEMO_FIXTURE;
pub use generations::{GenerationResult, PublishReport, PublishedItem};
pub use migrations::SCHEMA_VERSION;
pub use resources::ManagedFolder;
pub use sessions::SessionUpdate;
pub use updates::UpdateRecord;

use crate::clock::{Clock, SystemClock, Timestamp};
use crate::config::Config;
use crate::{Error, Result, limits};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub struct Repository {
    connection: Connection,
    demo: bool,
    clock: Arc<dyn Clock>,
}

impl Repository {
    /// Opens the database of `config`, creating the data directory. A
    /// consistent backup is written before any schema migration.
    pub fn open(config: &Config, clock: Arc<dyn Clock>) -> Result<Self> {
        std::fs::create_dir_all(&config.directory)?;
        let connection = Connection::open(config.database_path())?;
        Self::from_connection(connection, config.demo, clock, Some(config.backup_directory()))
    }

    pub fn open_path(path: &Path, demo: bool, clock: Arc<dyn Clock>) -> Result<Self> {
        let backups = path.parent().map(|p| p.join("backups"));
        Self::from_connection(Connection::open(path)?, demo, clock, backups)
    }

    /// In-memory database for tests.
    pub fn memory(demo: bool) -> Result<Self> {
        Self::memory_with_clock(demo, Arc::new(SystemClock))
    }

    pub fn memory_with_clock(demo: bool, clock: Arc<dyn Clock>) -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?, demo, clock, None)
    }

    fn from_connection(
        mut connection: Connection,
        demo: bool,
        clock: Arc<dyn Clock>,
        backups: Option<PathBuf>,
    ) -> Result<Self> {
        connection.busy_timeout(Duration::from_secs(5))?;
        // Refuse unknown newer schemas *before* changing any database option.
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::conflict(
                "Die Datenbank gehört zu einer neueren mogumogu-Version; sie wird nicht verändert.",
            ));
        }
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "cache_size", -limits::SQLITE_CACHE_KIB)?;
        connection.pragma_update(None, "journal_size_limit", 32 * 1024 * 1024)?;
        migrations::migrate(&mut connection, version, backups.as_deref(), clock.now())?;
        let mut repo = Self { connection, demo, clock };
        repo.guard_mode()?;
        if demo {
            repo.seed_demo()?;
        }
        Ok(repo)
    }

    /// Demo and local databases must never be mixed.
    fn guard_mode(&mut self) -> Result<()> {
        let mode = if self.demo { "demo" } else { "local" };
        self.write(|tx| {
            tx.execute("INSERT OR IGNORE INTO app_meta(key,value) VALUES('mode',?1)", [mode])?;
            let existing: String = tx.query_row("SELECT value FROM app_meta WHERE key='mode'", [], |r| r.get(0))?;
            if existing != mode {
                return Err(Error::conflict("Demo- und lokale Datenbanken dürfen nicht gemischt werden."));
            }
            Ok(())
        })
    }

    pub fn is_demo(&self) -> bool {
        self.demo
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.clock)
    }

    pub fn sqlite_version(&self) -> &'static str {
        rusqlite::version()
    }

    /// The demo inventory is read-only.
    pub(crate) fn require_local(&self) -> Result<()> {
        if self.demo {
            return Err(Error::blocked(
                "Demomodus ist schreibgeschützt. Für eigene Daten mogumogu im lokalen Modus starten.",
            ));
        }
        Ok(())
    }

    /// Runs `f` in one immediate (write-locking) transaction.
    pub(crate) fn write<T>(&mut self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let tx = self.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// Runs `f` in one deferred read transaction for a coherent view.
    pub(crate) fn read<T>(&mut self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let tx = self.connection.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// Consistent online copy via the SQLite backup API (never a raw file
    /// copy of an open database).
    pub fn backup_to(&self, target: &Path) -> Result<()> {
        if target.exists() {
            return Err(Error::invalid("Die Sicherungsdatei existiert bereits und wird nicht überschrieben."));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut destination = Connection::open(target)?;
        let backup = rusqlite::backup::Backup::new(&self.connection, &mut destination)?;
        backup.run_to_completion(256, Duration::from_millis(5), None)?;
        Ok(())
    }

    /// Requests a passive WAL checkpoint (bounded WAL growth).
    pub fn checkpoint(&self) -> Result<()> {
        self.connection.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// On owner start: work interrupted by a crash is marked, never resumed.
    pub fn recover_after_restart(&mut self) -> Result<RecoveryReport> {
        if self.demo {
            return Ok(RecoveryReport::default());
        }
        let now = self.now();
        let report = self.write(|tx| {
            let generations = tx.execute(
                "UPDATE generations SET state='failed', finished_at=?1,
                 error_summary='Erfassung durch Programmende unterbrochen' WHERE state='running'",
                [now],
            )?;
            let revalidating = tx.execute(
                "UPDATE cleanup_plans SET state='cancelled', finished_at=?1,
                 note='Vor Ausführung unterbrochen; nichts entfernt' WHERE state='revalidating'",
                [now],
            )?;
            let executing = tx.execute(
                "UPDATE cleanup_plans SET state='recovery_required',
                 note='Ausführung unterbrochen; Journal mit Dateisystem abgleichen' WHERE state='executing'",
                [],
            )?;
            Ok(RecoveryReport { generations, plans_cancelled: revalidating, plans_recovery: executing })
        })?;
        if report != RecoveryReport::default() {
            let detail = format!(
                "{} Erfassungen als fehlgeschlagen markiert, {} Pläne abgebrochen, {} Pläne benötigen Abgleich.",
                report.generations, report.plans_cancelled, report.plans_recovery
            );
            self.log("recovery", "Wiederanlauf geprüft", &detail)?;
        }
        Ok(report)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub generations: usize,
    pub plans_cancelled: usize,
    pub plans_recovery: usize,
}
