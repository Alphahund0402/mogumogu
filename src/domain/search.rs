use serde::{Deserialize, Serialize};

string_enum! {
    /// Where a search hit lives in the dashboard.
    pub enum SearchArea {
        Project => "project",
        Package => "package",
        Resource => "resource",
        Discovery => "discovery",
        Ai => "ai",
        System => "system",
    }
}

impl SearchArea {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Projekt",
            Self::Package => "Paket",
            Self::Resource => "Ressource",
            Self::Discovery => "Entwicklungsfund",
            Self::Ai => "KI-Konfiguration",
            Self::System => "System",
        }
    }
}

/// One bounded hit of the global search. `key` identifies the entry within
/// its area (project id, package name, …); nothing here grants an action.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchHit {
    pub area: SearchArea,
    pub key: String,
    pub title: String,
    pub detail: String,
}

/// Which package findings a page shows. Filters apply before paging.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PackageQuery {
    /// Case-insensitive part of name or version.
    pub text: String,
    pub filter: PackageFilter,
    /// Adapter id such as `npm`; empty for all managers.
    pub ecosystem: String,
}

string_enum! {
    #[derive(Default)]
    pub enum PackageFilter {
        #[default]
        All => "all",
        Review => "review",
        Deprecated => "deprecated",
        Unreferenced => "unreferenced",
        Kept => "kept",
    }
}

impl PackageFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Alle",
            Self::Review => "Prüfen",
            Self::Deprecated => "Abgekündigt",
            Self::Unreferenced => "Ohne Projektbeleg",
            Self::Kept => "Behalten",
        }
    }
}
