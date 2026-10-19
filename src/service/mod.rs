//! Use cases shared by dashboard, CLI and IPC (PROJEKTPLAN §5.2: "UI und
//! CLI rufen dieselben validierten Anwendungsfälle im Kern auf").
//!
//! `Core` is owned by exactly one thread of the owner process; every write
//! goes through it. It holds no UI types. Submodules group operations by
//! responsibility and extend the same `Core`; callers use one interface.
//!
//! `request` defines intent and dispatch, `snapshot` assembles dashboard data,
//! and `status` reads totals. Use cases live in `inventory`, `discovery`,
//! `packages`, `resources`, `sessions` and `maintenance`. Package analysis is
//! private and takes only metadata; repository access stays in its facade.
mod ai_content;
mod discovery;
mod disk;
mod inventory;
mod maintenance;
mod packages;
mod request;
mod resources;
mod search;
mod sessions;
mod snapshot;
mod status;
mod system;

/// Progress and cooperative stop of a running pass; the presentation reads and
/// cancels through these handles instead of touching the read boundary.
pub use crate::fsread::{ScanControl, ScanProgress};
pub use ai_content::AiContent;
pub use inventory::ScanReport;
pub use request::Request;
pub use resources::Explanation;
pub use status::Status;

use crate::Result;
use crate::clock::{Clock, SystemClock};
use crate::config::Config;
use crate::domain::SystemReport;
use crate::platform::Job;
use crate::storage::Repository;
use crate::updates::{self, Registry};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Core {
    config: Config,
    repo: Repository,
    registry: Box<dyn Registry + Send>,
    /// Job handles of managed sessions, held so jobs stay queryable.
    jobs: HashMap<i64, Job>,
    watched: Vec<i64>,
    /// Set when reconciliation changed stored state outside a write request.
    changed: bool,
    /// One bounded evidence page, invalidated by every committed repository write.
    package_cache: Option<packages::CachedPackages>,
    /// Last metadata-only system check; refreshed on request.
    system: Option<SystemReport>,
    /// Cached drive metadata; directory enumeration is always on demand.
    storage_cache: Option<crate::domain::StorageOverview>,
    /// Progress and cancel requests shared with the dashboard.
    scan_control: Arc<ScanControl>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core").field("config", &self.config).field("jobs", &self.jobs.len()).finish()
    }
}

fn default_registry() -> Box<dyn Registry + Send> {
    #[cfg(feature = "network")]
    {
        Box::new(updates::HttpRegistry::default())
    }
    #[cfg(not(feature = "network"))]
    {
        Box::new(updates::NoNetwork)
    }
}

impl Core {
    /// Opens the owner core: database, crash recovery, session reconciliation.
    pub fn open(config: Config) -> Result<Self> {
        Self::open_with(config, Arc::new(SystemClock), default_registry())
    }

    pub fn open_with(config: Config, clock: Arc<dyn Clock>, registry: Box<dyn Registry + Send>) -> Result<Self> {
        let repo = Repository::open(&config, clock)?;
        let mut core = Self {
            config,
            repo,
            registry,
            jobs: HashMap::new(),
            watched: Vec::new(),
            changed: false,
            package_cache: None,
            system: None,
            storage_cache: None,
            scan_control: Arc::new(ScanControl::default()),
        };
        core.repo.recover_after_restart()?;
        core.reconcile_sessions()?;
        Ok(core)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn repository(&mut self) -> &mut Repository {
        &mut self.repo
    }

    pub fn is_demo(&self) -> bool {
        self.repo.is_demo()
    }

    /// True once after reconciliation changed stored state.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Whether sessions still wait for evidence (owner re-checks them).
    pub fn has_open_sessions(&mut self) -> bool {
        !self.is_demo() && self.repo.open_sessions().map(|s| !s.is_empty()).unwrap_or(false)
    }

    /// Scope ids currently covered by the desktop watcher.
    pub fn set_watched(&mut self, scopes: Vec<i64>) {
        self.watched = scopes;
    }

    /// Shared with the dashboard: read progress, request a cancel.
    pub fn scan_control(&self) -> Arc<ScanControl> {
        Arc::clone(&self.scan_control)
    }
}
