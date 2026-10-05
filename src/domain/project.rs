use super::{Acquisition, Observation, OwnerKind, ResourceKind};
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub ecosystem: String,
    pub state: Observation,
    pub evidence: String,
    /// Measured or demo size; `None` means unknown, never zero.
    pub bytes: Option<i64>,
    /// Packages from the last complete inventory; `None` without one.
    pub packages: Option<i64>,
    pub protected: bool,
    pub origin: super::Origin,
    /// Read approval is a separate local decision from registration.
    #[serde(default)]
    pub read_approved: bool,
    #[serde(default)]
    pub coverage: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    pub id: i64,
    pub project_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub kind: ResourceKind,
    pub bytes: Option<i64>,
    pub state: Observation,
    pub evidence: String,
    pub protected: bool,
    pub origin: super::Origin,
    #[serde(default = "default_acquisition")]
    pub acquisition: Acquisition,
    /// Parent scratchpad for managed session folders.
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub purpose: Option<String>,
    /// Review date of a scratchpad; a priority, not a deletion deadline.
    #[serde(default)]
    pub review_at: Option<Timestamp>,
    /// Explicit local decision; only meaningful for managed temporary output.
    #[serde(default)]
    pub expendable: bool,
    #[serde(default)]
    pub last_activity_at: Option<Timestamp>,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
}

fn default_acquisition() -> Acquisition {
    Acquisition::Registered
}

/// "Warum ist das hier?" — one ownership claim with its evidence.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OwnerLink {
    pub resource_id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub kind: OwnerKind,
    pub evidence: String,
}

/// Static AI configuration entry as shown in the dashboard.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AiEntry {
    pub client: String,
    pub label: String,
    pub path: String,
    pub state: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub references: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Activity {
    pub title: String,
    pub detail: String,
    pub created_at: String,
    #[serde(default)]
    pub kind: String,
}
