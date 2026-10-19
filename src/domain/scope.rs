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
        ProjectSearch => "projects",
        NpmGlobal => "npm-global",
        UvTools => "uv-tools",
        Python => "python",
        CargoTools => "cargo-tools",
        NugetCache => "nuget-cache",
        /// One narrow folder of a client's user configuration (`.claude`, …),
        /// read only along the catalog's patterns.
        UserAi => "user-ai",
    }
}

impl ScopeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Projektordner",
            Self::Scoop => "Scoop-Installation",
            Self::Chocolatey => "Chocolatey-Installation",
            Self::Winget => "Windows-Softwareliste (WinGet-Kontext)",
            Self::ProjectSearch => "Projektsuche (nur Dateinamen)",
            Self::NpmGlobal => "Globale npm-Pakete",
            Self::UvTools => "uv-Toolumgebungen",
            Self::Python => "Python-Paketbestand",
            Self::CargoTools => "Cargo-Tools",
            Self::NugetCache => "Gemeinsamer NuGet-Paketspeicher",
            Self::UserAi => "KI-Konfiguration des Benutzers",
        }
    }

    pub fn storage_kind(self) -> Self {
        match self {
            Self::Project | Self::Scoop | Self::Chocolatey | Self::Winget => self,
            _ => Self::Project,
        }
    }
    pub fn profile(self) -> &'static str {
        if self == self.storage_kind() || !self.realm().is_empty() { "" } else { self.as_str() }
    }
    /// Stored separately from `profile`, whose values are fixed by schema v3.
    pub fn realm(self) -> &'static str {
        if self == Self::UserAi { "user" } else { "" }
    }
    /// Whose configuration this is, for scope labels in the dashboard.
    pub fn audience(self) -> &'static str {
        match self {
            Self::Project | Self::ProjectSearch => "Projekt",
            Self::UserAi => "Benutzer",
            _ => "System",
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
    pub fn current(&self) -> bool {
        self.status == ScopeStatus::Approved
            && self
                .published
                .as_ref()
                .zip(self.latest.as_ref())
                .is_some_and(|(p, l)| p.id == l.id && l.state == GenerationState::Complete)
    }
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
