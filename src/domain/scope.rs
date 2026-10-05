use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    /// What an approved read root contains. Each kind is approved separately.
    pub enum ScopeKind {
        Project => "project",
        Scoop => "scoop",
        Chocolatey => "chocolatey",
        /// Registry-based software list; read-only, no filesystem root.
        Winget => "winget",
    }
}

impl ScopeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Projektordner",
            Self::Scoop => "Scoop-Installation",
            Self::Chocolatey => "Chocolatey-Installation",
            Self::Winget => "Windows-Softwareliste (WinGet-Kontext)",
        }
    }
}

string_enum! {
    pub enum ScopeStatus {
        Approved => "approved",
        Revoked => "revoked",
        /// The object at the approved location is not the approved object.
        IdentityChanged => "identity_changed",
        Unavailable => "unavailable",
    }
}

impl ScopeStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Approved => "Freigegeben",
            Self::Revoked => "Widerrufen",
            Self::IdentityChanged => "Blockiert: Ordner ersetzt",
            Self::Unavailable => "Nicht erreichbar",
        }
    }
}

string_enum! {
    pub enum GenerationState {
        Running => "running",
        Partial => "partial",
        Complete => "complete",
        Failed => "failed",
        Superseded => "superseded",
    }
}

impl GenerationState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Läuft",
            Self::Partial => "Teilweise",
            Self::Complete => "Vollständig",
            Self::Failed => "Fehlgeschlagen",
            Self::Superseded => "Abgelöst",
        }
    }

    /// Generations only move forward; a finished result is never reopened.
    pub fn can_become(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Running, Self::Partial | Self::Complete | Self::Failed)
                | (Self::Complete, Self::Superseded)
                | (Self::Partial, Self::Superseded)
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GenerationSummary {
    pub id: i64,
    pub state: GenerationState,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub entries_seen: i64,
    pub files_read: i64,
    pub items: i64,
    pub limit_reason: Option<String>,
    pub error_count: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Scope {
    pub id: i64,
    pub project_id: Option<i64>,
    pub kind: ScopeKind,
    pub path: String,
    pub status: ScopeStatus,
    pub approved_at: Timestamp,
    pub last_error: Option<String>,
    /// Last complete generation: the published inventory.
    pub published: Option<GenerationSummary>,
    /// Newest generation of any state (may be partial or failed).
    pub latest: Option<GenerationSummary>,
    #[serde(default)]
    pub watched: bool,
}

impl Scope {
    /// Honest coverage text: partial or failed passes never hide behind an
    /// older complete state.
    pub fn coverage_label(&self) -> String {
        if self.status != ScopeStatus::Approved {
            return self.status.label().into();
        }
        match (&self.published, &self.latest) {
            (None, None) => "Freigegeben, noch nicht erfasst".into(),
            (Some(p), Some(l)) if p.id == l.id => {
                format!("Vollständig erfasst · {} Einträge", p.items)
            }
            (Some(_), Some(l)) => {
                format!("Letzter Lauf {} · älterer vollständiger Stand bleibt sichtbar", l.state.label().to_lowercase())
            }
            (None, Some(l)) => format!("{} · noch kein vollständiger Stand", l.state.label()),
            (Some(_), None) => "Vollständig erfasst".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generations_never_reopen() {
        use GenerationState::*;
        assert!(Running.can_become(Complete));
        assert!(Complete.can_become(Superseded));
        assert!(!Complete.can_become(Running));
        assert!(!Failed.can_become(Complete));
        assert!(!Superseded.can_become(Complete));
    }
}
