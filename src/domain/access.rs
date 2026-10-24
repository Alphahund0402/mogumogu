//! How far AI tools reach in one approved folder, from their settings files.
//! A setting states intent; whether a tool enforces it depends on the tool
//! and the platform, which the findings say where it is known.
use serde::{Deserialize, Serialize};

string_enum! {
    pub enum AccessLevel {
        /// Confirmations or the sandbox are off, or folders outside the
        /// project are granted.
        Wide => "wide",
        /// AI tools are configured, but nothing keeps files from them.
        Open => "open",
        /// Denials, ignore files or a sandbox narrow what the agent reads.
        Limited => "limited",
    }
}

impl AccessLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wide => "Weitreichend",
            Self::Open => "Ohne Filter",
            Self::Limited => "Begrenzt",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectAccess {
    pub scope_id: i64,
    pub name: String,
    pub path: String,
    /// "Projekt" or "Benutzer" (applies to every project).
    pub audience: String,
    /// AI tools with settings or instructions here.
    pub tools: Vec<String>,
    pub level: AccessLevel,
    pub findings: Vec<String>,
    /// Folders an agent rarely needs: environments, outputs, caches, test
    /// data found in this folder, largest kinds first.
    pub keep_out: Vec<String>,
}
