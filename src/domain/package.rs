use super::{InstallState, SourceClass, UpdateStatus};
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// One registry identity, independent of the project that consumes it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageId {
    pub ecosystem: String,
    pub name: String,
    pub version: String,
    pub source: SourceClass,
    pub source_host: String,
}

impl PackageId {
    pub fn key(&self) -> String {
        // Opaque, unambiguous and shell-friendly. The readable identity remains
        // in separate fields; no quoting of a JSON object is needed in a CLI.
        let identity = serde_json::to_vec(self).expect("string-only package identity");
        format!("{:x}", Sha256::digest(identity))
    }
}

pub fn package_ecosystem(ecosystem: &str) -> &str {
    match ecosystem {
        "pip" | "uv" | "python" => "python",
        "npm" | "pnpm" => "npm",
        other => other,
    }
}

pub fn package_name(ecosystem: &str, name: &str) -> String {
    let name = name.to_ascii_lowercase();
    if package_ecosystem(ecosystem) != "python" {
        return name;
    }
    let mut normalized = String::new();
    let mut separator = false;
    for ch in name.chars() {
        if matches!(ch, '-' | '_' | '.') {
            separator = true;
        } else {
            if separator && !normalized.is_empty() {
                normalized.push('-');
            }
            separator = false;
            normalized.push(ch);
        }
    }
    normalized
}

/// A static dependency edge. A requirement is not a resolved version.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PackageDependency {
    pub name: String,
    pub requirement: String,
    pub kind: String,
    pub exact: bool,
}

/// Optional selected metadata from a supported package format.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PackageEvidence {
    /// Workspace/framework/installation occurrence, not an absolute path.
    pub context: String,
    pub qualifier: String,
    pub dependencies: Vec<PackageDependency>,
    /// A local lockfile message is separate from fresh registry evidence.
    pub deprecated: Option<String>,
    pub owned_files: Option<u64>,
    pub executables: Vec<String>,
    pub commands: Vec<String>,
    /// Optional filename-only evidence. The surrounding `context` stores
    /// its nearest project path relative to the same approved root.
    pub discovery: Option<DiscoveryEvidence>,
}

/// None means unknown or unsupported, not a negative result.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Lifecycle {
    pub deprecated: Option<bool>,
    pub yanked: Option<bool>,
    pub project_status: Option<String>,
    pub reason: Option<String>,
    pub replacement: Option<String>,
    pub checked_at: Option<Timestamp>,
}

impl Lifecycle {
    pub fn needs_review(&self) -> bool {
        self.deprecated == Some(true)
            || self.yanked == Some(true)
            || self.project_status.as_deref().is_some_and(|s| matches!(s, "archived" | "deprecated" | "quarantined"))
    }
    pub fn label(&self) -> &'static str {
        if self.project_status.as_deref() == Some("quarantined") {
            "Quarantäne laut Quelle"
        } else if self.deprecated == Some(true) || self.project_status.as_deref() == Some("deprecated") {
            "Abgekündigt"
        } else if self.yanked == Some(true) {
            "Zurückgezogen (yanked)"
        } else if self.project_status.as_deref() == Some("archived") {
            "Archiviert"
        } else if self.checked_at.is_some()
            && (self.deprecated.is_some() || self.yanked.is_some() || self.project_status.is_some())
        {
            "Kein Warnhinweis laut Quelle"
        } else if self.checked_at.is_some() {
            "Status unbekannt"
        } else {
            "Nicht geprüft"
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageOccurrence {
    pub scope_id: i64,
    pub project_id: Option<i64>,
    pub project: String,
    pub rel_path: String,
    pub state: Option<InstallState>,
    pub detail: String,
    pub current: bool,
    pub owned_files: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageCoverage {
    pub complete_projects: usize,
    pub incomplete_projects: usize,
    pub discovery_roots: usize,
    pub incomplete_roots: usize,
    pub truncated: bool,
}

impl PackageCoverage {
    pub fn label(&self) -> String {
        format!(
            "{} Projekt{} vollständig · {} ungeprüft/teilweise · {} Suchwurzeln ({} unvollständig){}",
            self.complete_projects,
            if self.complete_projects == 1 { "" } else { "e" },
            self.incomplete_projects,
            self.discovery_roots,
            self.incomplete_roots,
            if self.truncated { " · Auswertung begrenzt" } else { "" }
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageFinding {
    pub id: PackageId,
    pub key: String,
    pub occurrences: Vec<PackageOccurrence>,
    pub references: Vec<String>,
    pub possible_references: Vec<String>,
    pub lifecycle: Lifecycle,
    pub lifecycle_stale: bool,
    pub local_deprecation: Option<String>,
    pub update: Option<UpdateStatus>,
    pub latest_version: Option<String>,
    pub retained: bool,
    pub coverage: String,
    pub next_action: String,
    pub review: bool,
}

impl PackageFinding {
    pub fn reference_label(&self) -> String {
        if !self.references.is_empty() {
            format!("{} Projektbeleg{}", self.references.len(), if self.references.len() == 1 { "" } else { "e" })
        } else if !self.possible_references.is_empty() {
            "Mögliche Verweise".into()
        } else {
            "Keine Projektverweise gefunden".into()
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageReport {
    pub coverage: PackageCoverage,
    pub packages: Vec<PackageFinding>,
    pub total: usize,
    pub truncated: bool,
    pub offset: usize,
    /// The query this page answers; counts follow its text and manager.
    #[serde(default)]
    pub query: super::PackageQuery,
    #[serde(default)]
    pub counts: PackageCounts,
}

/// Findings per filter for the current text and manager, before paging.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageCounts {
    pub all: usize,
    pub review: usize,
    pub deprecated: usize,
    pub unreferenced: usize,
    pub kept: usize,
}

string_enum! {
    /// Filename/structure evidence from an approved root, never proof of use
    /// or expendability. The same facts can be viewed without adoption.
    #[derive(Default)]
    pub enum DiscoveryKind {
        #[default]
        Project => "project",
        Scratchpad => "scratchpad",
        Script => "script",
        Notebook => "notebook",
        Environment => "environment",
        Dependencies => "dependencies",
        BuildOutput => "build_output",
        TestOutput => "test_output",
        TestData => "test_data",
        Database => "database",
        Cache => "cache",
    }
}

impl DiscoveryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Projekt",
            Self::Scratchpad => "Experiment / Scratchpad",
            Self::Script => "Skript / Quelldatei",
            Self::Notebook => "Notebook",
            Self::Environment => "Umgebung",
            Self::Dependencies => "Abhängigkeiten",
            Self::BuildOutput => "Buildausgabe",
            Self::TestOutput => "Testausgabe",
            Self::TestData => "Testdaten",
            Self::Database => "Lokale Datenbank",
            Self::Cache => "Cache",
        }
    }

    pub fn is_project(self) -> bool {
        self == Self::Project
    }

    pub fn resource_kind(self) -> super::ResourceKind {
        match self {
            Self::Environment | Self::Dependencies => super::ResourceKind::Environment,
            Self::Cache => super::ResourceKind::Cache,
            Self::BuildOutput | Self::TestOutput => super::ResourceKind::Output,
            Self::TestData | Self::Database => super::ResourceKind::Results,
            Self::Script | Self::Notebook => super::ResourceKind::Source,
            Self::Project | Self::Scratchpad => super::ResourceKind::Scratchpad,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryEvidence {
    pub kind: DiscoveryKind,
    pub is_dir: bool,
    pub modified_at: Option<Timestamp>,
    #[serde(default)]
    pub has_project_context: bool,
    /// Redacted/truncated paths remain displayable but cannot be turned
    /// back into exact registration paths by an action.
    #[serde(default = "exact_discovery_path")]
    pub path_exact: bool,
}

fn exact_discovery_path() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectCandidate {
    pub scope_id: i64,
    pub rel_path: String,
    pub evidence: String,
    pub current: bool,
    pub registered: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: DiscoveryKind,
    #[serde(default)]
    pub is_dir: bool,
    /// Directory listings only establish sizes for individual files.
    #[serde(default)]
    pub bytes: Option<i64>,
    #[serde(default)]
    pub modified_at: Option<Timestamp>,
    /// A registered project at or above the finding's path. This is a
    /// spatial suggestion until the user explicitly adopts the resource.
    #[serde(default)]
    pub project_id: Option<i64>,
    #[serde(default)]
    pub project: String,
    /// Closest filename-evidenced project within the discovery scope.
    #[serde(default)]
    pub project_rel_path: Option<String>,
    #[serde(default)]
    pub resource_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactFinding {
    pub scope_id: i64,
    pub name: String,
    pub rel_path: String,
    pub category: super::ItemCategory,
    pub project: String,
    pub detail: String,
    pub current: bool,
}
