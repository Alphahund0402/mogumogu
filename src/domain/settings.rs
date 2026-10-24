/// Local settings. Defaults are the conservative choice everywhere. Fields
/// added later carry serde defaults, so older documents stay readable; fields
/// that were removed (watcher, tray hints, update sources, scratch root,
/// review periods without a rule, the guided start) are ignored.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub scratch_review_days: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { scratch_review_days: 7 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_documents_stay_readable_without_removed_fields() {
        let old = r#"{"monitoring_enabled":true,"notifications_enabled":false,"network_updates_enabled":true,
            "update_sources":["pypi.org"],"scratch_root":null,"scratch_review_days":7,"environment_review_days":30,
            "global_review_days":90,"monitoring_paused_until":5,"daily_summary":true,"summary_hour":9,
            "quiet_hours":{"from":19,"to":8},"guide_enabled":false}"#;
        let settings: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.scratch_review_days, 7);
        let saved = serde_json::to_string(&settings).unwrap();
        for removed in [
            "monitoring",
            "notifications",
            "update",
            "quiet",
            "summary",
            "scratch_root",
            "global",
            "environment",
            "guide",
        ] {
            assert!(!saved.contains(removed), "{removed}");
        }
    }
}
