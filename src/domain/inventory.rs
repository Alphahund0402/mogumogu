use super::{PackageEvidence, PackageId, package_ecosystem, package_name};
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
    #[serde(default)]
    pub evidence: PackageEvidence,
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
            evidence: PackageEvidence::default(),
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
        let mut evidence = self.evidence.clone();
        evidence.context = crate::privacy::path(&evidence.context);
        evidence.qualifier = crate::privacy::label(&evidence.qualifier);
        evidence.deprecated = evidence.deprecated.as_deref().map(crate::privacy::label);
        evidence.executables = evidence.executables.iter().map(|s| crate::privacy::label(s)).collect();
        evidence.commands = evidence.commands.iter().map(|s| crate::privacy::label(s)).collect();
        for dependency in &mut evidence.dependencies {
            dependency.name = crate::privacy::label(&dependency.name);
            dependency.requirement = crate::privacy::label(&dependency.requirement);
            dependency.kind = crate::privacy::label(&dependency.kind);
        }
        Self {
            name: crate::privacy::label(&self.name),
            version: self.version.as_deref().map(crate::privacy::label),
            detail: crate::privacy::label(&self.detail),
            rel_path: crate::privacy::path(&self.rel_path),
            evidence,
            ..self.clone()
        }
    }

    /// Identity of an item across generations. Version is deliberately not
    /// part of the key so that a version bump is a change, not add+remove.
    pub fn key(&self) -> String {
        serde_json::to_string(&(
            self.category,
            &self.ecosystem,
            &self.rel_path,
            &self.name,
            self.install_state,
            &self.evidence.context,
            &self.evidence.qualifier,
        ))
        .expect("string item key")
    }

    pub fn package_id(&self) -> PackageId {
        let host = match self.source_host.as_deref() {
            Some("files.pythonhosted.org") => "pypi.org",
            Some(host) => host,
            None => "",
        };
        PackageId {
            ecosystem: package_ecosystem(&self.ecosystem).into(),
            name: package_name(&self.ecosystem, &self.name),
            version: self.version.clone().unwrap_or_default(),
            source: self.source,
            source_host: host.to_string(),
        }
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

/// Per-ecosystem counts of the published inventory for the dashboard.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct EcosystemCount {
    pub ecosystem: String,
    pub declared: i64,
    pub resolved: i64,
    pub installed: i64,
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
