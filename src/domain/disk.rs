//! Read-only storage observations. Logical file sizes are separate from the
//! operating system's volume usage and never represent reclaimable space.
use super::{ScopeKind, ScopeStatus};
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct StorageOverview {
    pub checked_at: Option<Timestamp>,
    pub drives: Vec<DriveUsage>,
    pub projects: Vec<ProjectStorage>,
    pub scopes: Vec<StorageScope>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DriveUsage {
    pub root: String,
    pub label: String,
    pub kind: String,
    pub total_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
    /// Logical sizes of measured, non-overlapping registered projects. None
    /// if at least one uncovered project has no measurement.
    pub project_bytes: Option<u64>,
    pub status: String,
    /// A local disk, which may be searched as a whole for projects.
    #[serde(default)]
    pub searchable: bool,
    #[serde(default)]
    pub search: Option<DriveSearch>,
}

/// The names-only search of a whole drive, once approved.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DriveSearch {
    pub scope_id: i64,
    pub status: ScopeStatus,
    /// Findings of the last complete pass; None before the first one.
    pub findings: Option<i64>,
    pub checked_at: Option<Timestamp>,
    /// The newest pass stopped early or met reading errors.
    pub partial: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProjectStorage {
    pub project_id: i64,
    pub name: String,
    pub path: String,
    pub drive: String,
    pub bytes: Option<u64>,
    /// Percentage of user-visible volume capacity, based on logical bytes.
    pub share: Option<f64>,
    pub scope_id: Option<i64>,
    /// False when this project is already contained in a measured parent.
    pub included: bool,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StorageScope {
    pub scope_id: i64,
    pub path: String,
    pub label: String,
    pub project_id: Option<i64>,
    pub kind: ScopeKind,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FolderTree {
    pub scope_id: i64,
    pub rel_path: String,
    pub entries: Vec<FolderEntry>,
    pub complete: bool,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FolderEntry {
    pub name: String,
    pub rel_path: String,
    pub depth: u32,
    pub is_dir: bool,
    pub bytes: Option<u64>,
    pub status: String,
}
