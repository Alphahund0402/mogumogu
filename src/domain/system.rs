use super::ScopeKind;
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    /// What a system finding is about.
    pub enum FindingGroup {
        Tool => "tool",
        Store => "store",
        AiConfig => "ai_config",
        /// Folders where tools keep quick tries and throwaway work.
        Scratch => "scratch",
    }
}

impl FindingGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tool => "Werkzeug",
            Self::Store => "Globaler Bestand",
            Self::AiConfig => "KI-Konfiguration des Benutzers",
            Self::Scratch => "Ablageort von Werkzeugen",
        }
    }
}

string_enum! {
    /// How far the evidence reaches. None of these says that a tool works:
    /// nothing is started to find out (SYSTEMERKENNUNG, "Ziel und Evidenz").
    pub enum FindingEvidence {
        /// An executable file of that name in a directory of the search path.
        OnPath => "on_path",
        /// The search path entry is a known launcher or shim folder.
        Shim => "shim",
        /// A Windows app execution alias (a reparse point, not followed).
        Alias => "alias",
        /// A known installation location with its marker files.
        Installed => "installed",
        /// A known data or configuration folder exists; its content is unread.
        Location => "location",
    }
}

impl FindingEvidence {
    pub fn label(self) -> &'static str {
        match self {
            Self::OnPath => "Pfad belegt",
            Self::Shim => "Shim",
            Self::Alias => "App-Alias",
            Self::Installed => "Installation belegt",
            Self::Location => "Ort vorhanden",
        }
    }
}

/// One metadata-only finding. `path` is local and private; `location` is
/// the same place written with Windows placeholders for display and export.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SystemFinding {
    pub id: String,
    pub group: FindingGroup,
    pub name: String,
    pub location: String,
    pub path: String,
    pub evidence: FindingEvidence,
    pub note: String,
    /// The read approval that would cover this place, if one exists for it.
    pub scope: Option<ScopeKind>,
    /// An existing approval at this place (filled from stored scopes).
    #[serde(default)]
    pub scope_id: Option<i64>,
    #[serde(default)]
    pub coverage: String,
}

/// Result of one check of the search path and the known locations. Missing
/// entries are not listed: an unchecked or unreachable place is not absent.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SystemReport {
    pub checked_at: Option<Timestamp>,
    pub path_entries: usize,
    pub locations: usize,
    pub findings: Vec<SystemFinding>,
}

impl SystemReport {
    pub fn of(&self, group: FindingGroup) -> impl Iterator<Item = &SystemFinding> {
        self.findings.iter().filter(move |f| f.group == group)
    }

    /// Findings that could be approved for reading but are not yet.
    pub fn suggestions(&self) -> usize {
        self.findings.iter().filter(|f| f.scope.is_some() && f.scope_id.is_none()).count()
    }
}
