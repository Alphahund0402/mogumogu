//! Tidy-up report: what in an approved project can be rebuilt and what needs
//! a look. A class comes from folder names plus the project file that names
//! how to rebuild it (a lockfile next to `node_modules`, `Cargo.toml` next to
//! `target`). Names and age alone never make anything removable.
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    pub enum TidyClass {
        /// A known output, cache or environment whose project names how to
        /// rebuild it.
        Regenerable => "regenerable",
        /// Possibly needed: test data, databases, logs, large files and
        /// environments or outputs without a file that rebuilds them.
        Review => "review",
    }
}

impl TidyClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Regenerable => "Wiederherstellbar",
            Self::Review => "Prüfen",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TidyItem {
    /// Relative to the project, `/`-separated.
    pub rel_path: String,
    pub class: TidyClass,
    pub label: String,
    /// Logical size below this entry.
    pub bytes: u64,
    /// Why this class: the names that support it.
    pub evidence: String,
    pub modified_at: Option<Timestamp>,
}

/// One measured project. Sizes are logical file sizes from directory
/// listings; an incomplete walk keeps `total` unknown.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectTidy {
    pub project_id: i64,
    pub name: String,
    pub path: String,
    pub total: Option<u64>,
    pub regenerable: u64,
    pub review: u64,
    /// Version control data (`.git` …): kept, but shown for orientation.
    pub vcs: u64,
    /// Test data, part of `review`.
    #[serde(default)]
    pub test_data: u64,
    /// Largest entries first, bounded.
    pub items: Vec<TidyItem>,
    /// Newest file outside outputs, caches and version control.
    pub last_change: Option<Timestamp>,
    pub complete: bool,
    pub note: String,
    pub checked_at: Timestamp,
}

/// The label of test data items; the context filter keeps them out too.
pub const TEST_DATA_LABEL: &str = "Testdaten";

/// Test data this large are worth a look on their own.
const TEST_DATA_LARGE: u64 = 1 << 30;
/// Below this, test data never stand out.
const TEST_DATA_NOTABLE: u64 = 100 << 20;

impl ProjectTidy {
    /// The project's own content: everything but outputs, caches,
    /// environments and version control. Unknown for an incomplete walk.
    pub fn own_content(&self) -> Option<u64> {
        self.total.map(|total| total.saturating_sub(self.regenerable.saturating_add(self.vcs)))
    }

    /// Test data over 1 GB, or over 100 MB and at least half of the
    /// project's own content. A reason to look, not a verdict.
    pub fn heavy_test_data(&self) -> bool {
        self.test_data >= TEST_DATA_LARGE
            || (self.test_data >= TEST_DATA_NOTABLE
                && self.own_content().is_some_and(|own| self.test_data.saturating_mul(2) >= own))
    }
}

/// An approved global store, measured as a whole.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreTidy {
    pub scope_id: i64,
    pub label: String,
    pub path: String,
    pub bytes: Option<u64>,
    pub class: TidyClass,
    pub evidence: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TidyReport {
    pub projects: Vec<ProjectTidy>,
    pub stores: Vec<StoreTidy>,
    /// Registered projects without a read approval: nothing is measured there.
    pub unapproved: usize,
    pub checked_at: Option<Timestamp>,
}

impl TidyReport {
    pub fn regenerable(&self) -> u64 {
        self.projects.iter().map(|p| p.regenerable).sum::<u64>()
            + self.stores.iter().filter(|s| s.class == TidyClass::Regenerable).filter_map(|s| s.bytes).sum::<u64>()
    }

    pub fn review(&self) -> u64 {
        self.projects.iter().map(|p| p.review).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(total: u64, regenerable: u64, test_data: u64) -> ProjectTidy {
        ProjectTidy {
            project_id: 1,
            name: "app".into(),
            path: String::new(),
            total: Some(total),
            regenerable,
            review: test_data,
            vcs: 0,
            test_data,
            items: Vec::new(),
            last_change: None,
            complete: true,
            note: String::new(),
            checked_at: 0,
        }
    }

    #[test]
    fn test_data_stand_out_by_size_or_share_of_the_own_content() {
        let mb = 1 << 20;
        assert!(project(300 * mb, 100 * mb, 100 * mb).heavy_test_data(), "half of 200 MB own content");
        assert!(!project(2_000 * mb, 100 * mb, 200 * mb).heavy_test_data(), "a small share");
        assert!(!project(60 * mb, 0, 50 * mb).heavy_test_data(), "below 100 MB never");
        assert!(project(20_000 * mb, 0, 1_024 * mb).heavy_test_data(), "1 GB always");
        let unknown = ProjectTidy { total: None, ..project(0, 0, 200 * mb) };
        assert!(!unknown.heavy_test_data(), "an incomplete walk gives no share");
    }
}
