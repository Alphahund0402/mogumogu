//! Central budgets from the plan (PROJEKTPLAN §12.2). They are starting values,
//! not measured optima. Profiles and callers may only tighten them; hitting a
//! limit means incomplete coverage, never absence.
use std::time::Duration;

/// Prototype capacity of the metadata registry.
pub const MAX_PROJECTS: i64 = 2_000;
pub const MAX_RESOURCES: i64 = 5_000;
/// Activity history kept in SQLite.
pub const ACTIVITY_KEEP: i64 = 200;
/// Default UI page size; the dashboard never holds a full inventory copy.
pub const UI_PAGE: i64 = 200;
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

/// Filesystem watcher queue.
pub const EVENT_QUEUE: usize = 4_096;
pub const WATCH_DEBOUNCE: Duration = Duration::from_secs(2);
pub const WATCH_MIN_RESCAN_INTERVAL: Duration = Duration::from_secs(30);

/// Network update hints.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
pub const HTTP_MAX_BODY: u64 = 4 << 20;
pub const UPDATE_CHECKS_PER_RUN: usize = 50;
pub const UPDATE_RUN_BUDGET: Duration = Duration::from_secs(60);

/// Cleanup manifests.
pub const CLEANUP_MAX_ENTRIES: usize = 50_000;
pub const CLEANUP_MAX_DEPTH: u32 = 32;
pub const CLEANUP_JOURNAL_BATCH: usize = 64;

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
}
