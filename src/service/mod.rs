//! Use cases shared by dashboard, CLI and IPC (PROJEKTPLAN §5.2: "UI und
//! CLI rufen dieselben validierten Anwendungsfälle im Kern auf").
//!
//! `Core` is owned by exactly one thread of the owner process; every write
//! goes through it. It holds no UI types. Submodules group operations by
//! responsibility and extend the same `Core`; callers use one interface.
//!
//! `request` defines intent and dispatch, `snapshot` assembles dashboard data,
//! and `status` reads totals. Use cases live in `inventory`, `discovery`,
//! `packages`, `resources`, `tidy`, `filters` and `maintenance`. Package analysis is
//! private and takes only metadata; repository access stays in its facade.
mod access;
mod ai_content;
mod discovery;
mod disk;
mod filters;
mod inventory;
mod maintenance;
mod packages;
mod request;
mod resources;
mod search;
mod snapshot;
mod status;
mod system;
mod tidy;

/// Progress and cooperative stop of a running pass; the presentation reads and
/// cancels through these handles instead of touching the read boundary.
pub use crate::fsread::{ScanControl, ScanProgress};
pub use ai_content::AiContent;
pub use discovery::{FoundProject, FoundSkip, ProjectBatch};
pub use filters::{FilterBatch, FilterFile, FilterPlan, FilterSkip};
pub use inventory::ScanReport;
pub use request::Request;
pub use resources::Explanation;
pub use status::Status;

use crate::Result;
use crate::cleanup::{RecycleBin, Recycler};
use crate::clock::{Clock, SystemClock};
use crate::config::Config;
use crate::domain::SystemReport;
use crate::storage::Repository;
use std::sync::Arc;

pub struct Core {
    config: Config,
    repo: Repository,
    /// Where an approved cleanup moves folders: the real recycle bin unless
    /// a test replaces it.
    recycler: Box<dyn Recycler + Send>,
    /// Set when a refresh changed shown state outside a write request.
    changed: bool,
    /// One bounded evidence page, invalidated by every committed repository write.
    package_cache: Option<packages::CachedPackages>,
    /// Last metadata-only system check; refreshed on request.
    system: Option<SystemReport>,
    /// Cached drive metadata; directory enumeration is always on demand.
    storage_cache: Option<crate::domain::StorageOverview>,
    /// Last tidy-up report; measured only on request, never persisted.
    tidy: Option<crate::domain::TidyReport>,
    /// Progress and cancel requests shared with the dashboard.
    scan_control: Arc<ScanControl>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core").field("config", &self.config).finish_non_exhaustive()
    }
}

impl Core {
    /// Opens the owner core: database and crash recovery.
    pub fn open(config: Config) -> Result<Self> {
        let mut core = Self::open_with(config, Arc::new(SystemClock))?;
        core.retire_autostart();
        Ok(core)
    }

    pub fn open_with(config: Config, clock: Arc<dyn Clock>) -> Result<Self> {
        let repo = Repository::open(&config, clock)?;
        let mut core = Self {
            config,
            repo,
            recycler: Box::new(RecycleBin),
            changed: false,
            package_cache: None,
            system: None,
            storage_cache: None,
            tidy: None,
            scan_control: Arc::new(ScanControl::default()),
        };
        core.repo.recover_after_restart()?;
        core.repo.retire_removed_scopes()?;
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

    /// True once after a refresh changed shown state.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Replaces the recycle bin for tests, which must never fill the real one.
    pub fn use_recycler(&mut self, recycler: Box<dyn Recycler + Send>) {
        self.recycler = recycler;
    }

    /// Shared with the dashboard: read progress, request a cancel.
    pub fn scan_control(&self) -> Arc<ScanControl> {
        Arc::clone(&self.scan_control)
    }
}
