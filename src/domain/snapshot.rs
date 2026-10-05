use super::*;
use serde::{Deserialize, Serialize};

/// One slice of the storage overview. `bytes == None` means not measured.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StorageSlice {
    pub label: String,
    pub bytes: Option<i64>,
}

/// Bounded read model for dashboard and `list --json`. Every list is paged
/// to [`crate::limits::UI_PAGE`] entries or the prototype capacity.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub demo: bool,
    pub sqlite_version: String,
    pub projects: Vec<Project>,
    pub resources: Vec<Resource>,
    pub ai: Vec<AiEntry>,
    pub activity: Vec<Activity>,
    #[serde(default)]
    pub scopes: Vec<Scope>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub hints: Vec<ReviewHint>,
    #[serde(default)]
    pub plans: Vec<PlanSummary>,
    #[serde(default)]
    pub owners: Vec<OwnerLink>,
    #[serde(default)]
    pub ecosystems: Vec<EcosystemCount>,
    #[serde(default)]
    pub storage: Vec<StorageSlice>,
    #[serde(default)]
    pub settings: Option<Settings>,
}

impl Snapshot {
    /// Sum of resource sizes, or `None` if any size is unknown.
    pub fn total_bytes(&self) -> Option<i64> {
        if self.resources.is_empty() || self.resources.iter().any(|r| r.bytes.is_none()) {
            return None;
        }
        self.resources.iter().try_fold(0_i64, |sum, r| sum.checked_add(r.bytes?))
    }

    /// Package count across projects, or `None` if any project is unknown.
    pub fn package_count(&self) -> Option<i64> {
        if self.projects.is_empty() || self.projects.iter().any(|p| p.packages.is_none()) {
            return None;
        }
        self.projects.iter().try_fold(0_i64, |sum, p| sum.checked_add(p.packages?))
    }

    pub fn review_count(&self) -> usize {
        self.hints.len()
    }

    pub fn owners_of(&self, resource_id: i64) -> impl Iterator<Item = &OwnerLink> {
        self.owners.iter().filter(move |o| o.resource_id == resource_id)
    }
}

/// Byte formatting for German UI text. Unknown is never rendered as zero.
pub fn format_bytes(bytes: Option<i64>) -> String {
    match bytes {
        None => "Unbekannt".into(),
        Some(value) if value < 1_000_000_000 => format!("{:.0} MB", value as f64 / 1_000_000.0),
        Some(value) => format!("{:.1} GB", value as f64 / 1_000_000_000.0).replace('.', ","),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_sizes_are_not_zero() {
        assert_eq!(format_bytes(None), "Unbekannt");
        assert_eq!(format_bytes(Some(0)), "0 MB");
        assert_eq!(format_bytes(Some(142_000_000_000)), "142,0 GB");
    }

    #[test]
    fn empty_inventory_has_unknown_totals() {
        let s = Snapshot::default();
        assert_eq!(s.total_bytes(), None);
        assert_eq!(s.package_count(), None);
    }
}
