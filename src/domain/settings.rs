use serde::{Deserialize, Serialize};

/// Public update sources that can be approved. Anything else is private or
/// unknown and is never queried (F-20).
pub const PUBLIC_UPDATE_SOURCES: &[&str] = &["registry.npmjs.org", "pypi.org", "crates.io"];

/// Local, opt-in settings. Defaults are the conservative choice everywhere.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Debounced filesystem watcher for approved scopes.
    pub monitoring_enabled: bool,
    /// Generic tray hints only; no private paths or package names.
    pub notifications_enabled: bool,
    /// Global switch for update hints; each source needs its own approval.
    pub network_updates_enabled: bool,
    pub update_sources: Vec<String>,
    /// Root for scratchpads that mogumogu creates itself.
    pub scratch_root: Option<String>,
    pub scratch_review_days: i64,
    pub environment_review_days: i64,
    pub global_review_days: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            monitoring_enabled: false,
            notifications_enabled: false,
            network_updates_enabled: false,
            update_sources: Vec::new(),
            scratch_root: None,
            scratch_review_days: 7,
            environment_review_days: 30,
            global_review_days: 90,
        }
    }
}

impl Settings {
    pub fn source_approved(&self, host: &str) -> bool {
        self.network_updates_enabled && self.update_sources.iter().any(|s| s == host)
    }
}
