//! Central budgets from the plan (PROJEKTPLAN §12.2). They are starting values,
//! not measured optima. Profiles and callers may only tighten them; hitting a
//! limit means incomplete coverage, never absence. The one wider budget is
//! [`ScanLimits::drive_search`], which reads names only.
use std::time::Duration;

/// Prototype capacity of the metadata registry.
pub const MAX_PROJECTS: i64 = 2_000;
pub const MAX_RESOURCES: i64 = 5_000;
/// Activity history kept in SQLite.
pub const ACTIVITY_KEEP: i64 = 200;
/// Default UI page size; the dashboard never holds a full inventory copy.
pub const UI_PAGE: i64 = 200;
/// Package review: bounded facts, one cached page and evidence per finding.
pub const PACKAGE_ANALYSIS_ITEMS: i64 = 200_000;
pub const PACKAGE_PAGE: usize = 60;
pub const PACKAGE_EVIDENCE_ROWS: usize = 6;
/// Inventory events kept per scope.
pub const EVENTS_KEEP: i64 = 2_000;
/// Complete generations kept per scope (the published one plus history).
pub const GENERATIONS_KEEP: i64 = 3;

/// AI/settings files: size and nesting (Standardprofil).
pub const AI_FILE_MAX_BYTES: u64 = 1 << 20;
pub const AI_MAX_NESTING: usize = 32;
/// Package metadata files (lockfiles, manifests).
pub const PACKAGE_FILE_MAX_BYTES: u64 = 32 << 20;
/// Maximum import/reference depth and references listed per artifact.
pub const REFERENCE_MAX_DEPTH: usize = 16;
pub const REFERENCES_PER_ARTIFACT: usize = 50;
/// Items recorded per parsed file; more means a visible partial status.
pub const ITEMS_PER_FILE: usize = 20_000;
/// Stored free text (labels, details) after redaction.
pub const LABEL_MAX_CHARS: usize = 200;

/// Local IPC.
pub const IPC_MAX_MESSAGE: usize = 1 << 20;
pub const IPC_CLIENT_TIMEOUT: Duration = Duration::from_secs(120);
/// Owner job queue.
pub const EVENT_QUEUE: usize = 4_096;

/// Folders in one cleanup plan.
pub const CLEANUP_MAX_TARGETS: usize = 50;

/// One on-demand metadata-only disk index; never part of a dashboard snapshot.
/// 40 bytes per entry plus its name (about 30 bytes on a system drive); it
/// exists only while an analysis is open. Measurements: docs/MESSBERICHT.md.
pub const DISK_MAX_NODES: usize = 4_000_000;
pub const DISK_NAME_BYTES: usize = 160 << 20;
/// Time of one analysis, like the drive search; the owner waits for it longer
/// than for other CLI requests (`Request::reply_within`).
pub const DISK_TIME: Duration = Duration::from_secs(180);
pub const DISK_PAGE: usize = 60;
pub const DISK_MAX_PAGE: usize = 200;
pub const DISK_TILES: usize = 40;
/// Comparison steps a name filter may spend beyond four per name byte.
pub const DISK_MATCH_STEPS: u64 = 20_000_000;
/// Without a dashboard, an unused analysis is released after this time.
pub const DISK_IDLE: Duration = Duration::from_secs(600);

/// SQLite page cache per connection (KiB, negative pragma value).
pub const SQLITE_CACHE_KIB: i64 = 2_048;

/// Budget for one discovery pass over a scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanLimits {
    pub max_depth: u32,
    pub max_entries: u64,
    pub max_total_bytes: u64,
    pub time_budget: Duration,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self { max_depth: 16, max_entries: 200_000, max_total_bytes: 256 << 20, time_budget: Duration::from_secs(60) }
    }
}

impl ScanLimits {
    pub fn storage_analysis() -> Self {
        Self { max_depth: 64, max_entries: DISK_MAX_NODES as u64 - 1, max_total_bytes: 0, time_budget: DISK_TIME }
    }
    /// A whole local drive holds far more names than a project folder. This
    /// budget belongs to the names-only drive search alone: it reads no file
    /// content at all, and the time budget stays the bound of one pass.
    pub fn drive_search() -> Self {
        Self { max_depth: 32, max_entries: 2_000_000, max_total_bytes: 0, time_budget: Duration::from_secs(180) }
    }

    /// Combine two budgets; the stricter value always wins.
    pub fn tighten(self, other: Self) -> Self {
        Self {
            max_depth: self.max_depth.min(other.max_depth),
            max_entries: self.max_entries.min(other.max_entries),
            max_total_bytes: self.max_total_bytes.min(other.max_total_bytes),
            time_budget: self.time_budget.min(other.time_budget),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tightening_never_widens() {
        let wide = ScanLimits {
            max_depth: 99,
            max_entries: u64::MAX,
            max_total_bytes: u64::MAX,
            time_budget: Duration::from_secs(9_999),
        };
        assert_eq!(ScanLimits::default().tighten(wide), ScanLimits::default());
        let narrow = ScanLimits { max_depth: 2, ..ScanLimits::default() };
        assert_eq!(ScanLimits::default().tighten(narrow).max_depth, 2);
    }

    #[test]
    fn a_drive_search_reads_no_content() {
        assert_eq!(ScanLimits::drive_search().max_total_bytes, 0);
        assert!(ScanLimits::drive_search().time_budget <= Duration::from_secs(180));
    }
}
