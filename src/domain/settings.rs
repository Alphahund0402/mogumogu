use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

/// Public update sources that can be approved. Anything else is private or
/// unknown and is never queried (F-20).
pub const PUBLIC_UPDATE_SOURCES: &[&str] = &["registry.npmjs.org", "pypi.org", "crates.io", "api.nuget.org"];

/// Local, opt-in settings. Defaults are the conservative choice everywhere.
/// Fields added later carry serde defaults, so older documents stay readable.
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
    /// Guided start while the inventory is empty. Skipping keeps it; switching
    /// it off hides it until it is switched on again.
    #[serde(default = "enabled")]
    pub guide_enabled: bool,
    /// Temporary pause of the watcher. The monitoring choice itself stays.
    #[serde(default)]
    pub monitoring_paused_until: Option<Timestamp>,
    /// Tray hints once a day at `summary_hour` instead of after each change.
    #[serde(default)]
    pub daily_summary: bool,
    #[serde(default = "default_summary_hour")]
    pub summary_hour: u8,
    /// Local hours in which the tray never draws attention.
    #[serde(default)]
    pub quiet_hours: Option<QuietHours>,
}

fn enabled() -> bool {
    true
}

fn default_summary_hour() -> u8 {
    17
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
            guide_enabled: true,
            monitoring_paused_until: None,
            daily_summary: false,
            summary_hour: default_summary_hour(),
            quiet_hours: None,
        }
    }
}

/// A local time window in whole hours; `from == to` is rejected on save.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuietHours {
    pub from: u8,
    pub to: u8,
}

impl QuietHours {
    /// Whether a local hour (0–23) lies in the window, which may span midnight.
    pub fn contains(self, hour: u8) -> bool {
        if self.from <= self.to { (self.from..self.to).contains(&hour) } else { hour >= self.from || hour < self.to }
    }
}

impl Settings {
    pub fn source_approved(&self, host: &str) -> bool {
        self.network_updates_enabled && self.update_sources.iter().any(|s| s == host)
    }

    /// Monitoring is chosen and not paused at `now`.
    pub fn monitoring_active(&self, now: Timestamp) -> bool {
        self.monitoring_enabled && self.monitoring_paused_until.is_none_or(|until| now >= until)
    }

    /// A pause that has not ended yet.
    pub fn pause_pending(&self, now: Timestamp) -> Option<Timestamp> {
        self.monitoring_paused_until.filter(|until| *until > now)
    }

    /// Whether the tray may draw attention in this local hour.
    pub fn alert_allowed(&self, local_hour: u8) -> bool {
        self.notifications_enabled && !self.quiet_hours.is_some_and(|q| q.contains(local_hour))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_documents_get_conservative_defaults() {
        let old = r#"{"monitoring_enabled":true,"notifications_enabled":false,"network_updates_enabled":false,
            "update_sources":[],"scratch_root":null,"scratch_review_days":7,"environment_review_days":30,
            "global_review_days":90}"#;
        let settings: Settings = serde_json::from_str(old).unwrap();
        assert!(settings.guide_enabled && settings.monitoring_enabled);
        assert_eq!(
            (settings.monitoring_paused_until, settings.daily_summary, settings.quiet_hours),
            (None, false, None)
        );
    }

    #[test]
    fn a_pause_ends_by_itself_and_keeps_the_choice() {
        let settings = Settings { monitoring_enabled: true, monitoring_paused_until: Some(100), ..Default::default() };
        assert!(!settings.monitoring_active(99));
        assert_eq!(settings.pause_pending(99), Some(100));
        assert!(settings.monitoring_active(100));
        assert_eq!(settings.pause_pending(100), None);
        assert!(!Settings { monitoring_enabled: false, ..settings }.monitoring_active(200));
    }

    #[test]
    fn quiet_hours_may_span_midnight() {
        let night = QuietHours { from: 19, to: 8 };
        assert!(night.contains(19) && night.contains(23) && night.contains(0) && night.contains(7));
        assert!(!night.contains(8) && !night.contains(18));
        let lunch = QuietHours { from: 12, to: 13 };
        assert!(lunch.contains(12) && !lunch.contains(13));
        let settings = Settings { notifications_enabled: true, quiet_hours: Some(night), ..Default::default() };
        assert!(!settings.alert_allowed(22) && settings.alert_allowed(10));
    }
}
