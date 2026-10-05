use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    pub enum ItemCategory {
        Package => "package",
        Manifest => "manifest",
        Environment => "environment",
        Output => "output",
        Cache => "cache",
        /// Installed software from a system-wide manager or registry list.
        Software => "software",
        AiArtifact => "ai",
        /// A start/package/file reference found in AI configuration. It is
        /// never evidence of an installation or of use.
        AiReference => "ai_reference",
    }
}

string_enum! {
    /// Declared, resolved and installed are separate facts (F-09).
    pub enum InstallState {
        Declared => "declared",
        Resolved => "resolved",
        Installed => "installed",
    }
}

impl InstallState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Declared => "deklariert",
            Self::Resolved => "aufgelöst",
            Self::Installed => "installiert",
        }
    }
}

string_enum! {
    /// Where a package comes from. Only `PublicRegistry` may ever be sent to
    /// a public update source, and only after local approval.
    pub enum SourceClass {
        PublicRegistry => "public",
        PrivateRegistry => "private",
        Path => "path",
        Git => "git",
        Unknown => "unknown",
    }
}

/// One fact found by a static adapter or profile within one generation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventoryItem {
    pub category: ItemCategory,
    /// Adapter id (`npm`, `cargo`, …) or AI profile id (`claude-code`, …).
    pub ecosystem: String,
    pub name: String,
    pub version: Option<String>,
    pub install_state: Option<InstallState>,
    pub source: SourceClass,
    /// Host of a registry source when known (never credentials or paths).
    pub source_host: Option<String>,
    /// Location relative to the scope root, `/`-separated.
    pub rel_path: String,
    /// Redacted, bounded description. Never raw file content.
    pub detail: String,
    pub bytes: Option<i64>,
}

impl InventoryItem {
    pub fn new(category: ItemCategory, ecosystem: &str, name: impl Into<String>, rel_path: impl Into<String>) -> Self {
        Self {
            category,
            ecosystem: ecosystem.into(),
            name: name.into(),
            version: None,
            install_state: None,
            source: SourceClass::Unknown,
            source_host: None,
            rel_path: rel_path.into(),
            detail: String::new(),
            bytes: None,
        }
    }
    pub fn version(mut self, version: impl Into<String>) -> Self {
        let version = version.into();
        self.version = (!version.is_empty()).then_some(version);
        self
    }
    pub fn state(mut self, state: InstallState) -> Self {
        self.install_state = Some(state);
        self
    }
    pub fn source(mut self, source: SourceClass, host: Option<String>) -> Self {
        self.source = source;
        self.source_host = host;
        self
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = crate::privacy::label(&detail.into());
        self
    }

    /// Copy with every free-text field redacted and bounded; storage keys are
    /// derived from this form so no raw text reaches an index either.
    pub fn sanitized(&self) -> Self {
        Self {
            name: crate::privacy::label(&self.name),
            version: self.version.as_deref().map(crate::privacy::label),
            detail: crate::privacy::label(&self.detail),
            rel_path: crate::privacy::label(&self.rel_path),
            ..self.clone()
        }
    }

    /// Identity of an item across generations. Version is deliberately not
    /// part of the key so that a version bump is a change, not add+remove.
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.category,
            self.ecosystem,
            self.rel_path,
            self.name,
            self.install_state.map_or("", InstallState::as_str)
        )
    }
}

string_enum! {
    pub enum InventoryEventKind {
        /// First complete pass or a new comparison basis; no item events.
        Baseline => "baseline",
        Added => "added",
        Removed => "removed",
        Changed => "changed",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct InventoryEvent {
    pub id: i64,
    pub scope_id: i64,
    pub kind: InventoryEventKind,
    pub summary: String,
    pub created_at: Timestamp,
}

string_enum! {
    /// Result of an update hint. Failures are never reported as "current".
    pub enum UpdateStatus {
        Current => "current",
        UpdateAvailable => "update_available",
        Offline => "offline",
        Stale => "stale",
        Unknown => "unknown",
        Unsupported => "unsupported",
        PrivateSkipped => "private_skipped",
        NotApproved => "not_approved",
    }
}

impl UpdateStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "Aktuell laut Quelle",
            Self::UpdateAvailable => "Neuere Version verfügbar",
            Self::Offline => "Offline",
            Self::Stale => "Veraltet",
            Self::Unknown => "Unbekannt",
            Self::Unsupported => "Nicht unterstützt",
            Self::PrivateSkipped => "Private Quelle – nicht abgefragt",
            Self::NotApproved => "Quelle nicht freigegeben",
        }
    }
}

/// Per-ecosystem counts of the published inventory for the dashboard.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct EcosystemCount {
    pub ecosystem: String,
    pub declared: i64,
    pub resolved: i64,
    pub installed: i64,
    pub updates_available: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_change_keeps_item_identity() {
        let a = InventoryItem::new(ItemCategory::Package, "npm", "left-pad", "package-lock.json")
            .version("1.0.0")
            .state(InstallState::Resolved);
        let b = a.clone().version("1.1.0");
        assert_eq!(a.key(), b.key());
        let installed = a.clone().state(InstallState::Installed);
        assert_ne!(a.key(), installed.key());
    }
}
