//! Source-bound, version-specific update and lifecycle hints. Scanning is offline.
#[cfg(feature = "network")]
mod http;
mod versions;
use crate::Result;
use crate::clock::{DAY, Timestamp};
use crate::domain::{
    InstallState, InventoryItem, ItemCategory, Lifecycle, PackageId, Settings, SourceClass, UpdateStatus,
};
use crate::limits::{UPDATE_CHECKS_PER_RUN, UPDATE_RUN_BUDGET};
use crate::storage::{PublishedItem, Repository, UpdateRecord};
#[cfg(feature = "network")]
pub use http::HttpRegistry;
use std::collections::BTreeMap;
use std::time::Instant;
pub use versions::compare;

const CACHE_TTL: i64 = DAY;
const STALE_AFTER: i64 = 7 * DAY;

#[derive(Debug)]
pub enum FetchError {
    Offline(String),
    NotFound,
    Rejected(String),
}

#[derive(Clone, Debug, Default)]
pub struct RegistryMetadata {
    pub latest: Option<String>,
    pub lifecycle: Lifecycle,
}

/// Implementations query only this approved public origin and exact version.
pub trait Registry {
    fn metadata(&self, host: &str, name: &str, version: &str) -> std::result::Result<RegistryMetadata, FetchError>;
}
#[derive(Debug, Default)]
pub struct NoNetwork;
impl Registry for NoNetwork {
    fn metadata(&self, _: &str, _: &str, _: &str) -> std::result::Result<RegistryMetadata, FetchError> {
        Err(FetchError::Rejected("Netzwerkfunktion nicht einkompiliert".into()))
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct UpdateSummary {
    pub checked: usize,
    pub available: usize,
    pub lifecycle_warnings: usize,
    pub skipped_private: usize,
    pub not_approved: usize,
    pub failed: usize,
    pub cached: usize,
    pub partial: bool,
}

fn registry_host(id: &PackageId) -> std::result::Result<&str, UpdateStatus> {
    match id.source {
        SourceClass::PrivateRegistry | SourceClass::Path | SourceClass::Git => {
            return Err(UpdateStatus::PrivateSkipped);
        }
        SourceClass::Unknown => return Err(UpdateStatus::Unsupported),
        SourceClass::PublicRegistry => {}
    }
    match (id.ecosystem.as_str(), id.source_host.as_str()) {
        ("npm", "registry.npmjs.org")
        | ("python", "pypi.org")
        | ("cargo", "crates.io")
        | ("nuget", "api.nuget.org") => Ok(&id.source_host),
        _ => Err(UpdateStatus::Unsupported),
    }
}

fn candidates(items: &[PublishedItem]) -> BTreeMap<PackageId, InventoryItem> {
    let mut out = BTreeMap::new();
    for published in items {
        let item = &published.item;
        if (item.category == ItemCategory::Package
            && matches!(item.install_state, Some(InstallState::Resolved | InstallState::Installed))
            || item.category == ItemCategory::Cache && item.ecosystem == "nuget")
            && item.version.as_deref().is_some_and(|v| !v.is_empty())
        {
            out.entry(item.package_id()).or_insert_with(|| item.clone());
        }
    }
    out
}

pub fn run(repo: &mut Repository, registry: &dyn Registry, settings: &Settings) -> Result<UpdateSummary> {
    let now = repo.now();
    let started = Instant::now();
    let mut items = repo.published_items_all(None, 200_001)?;
    let mut summary = UpdateSummary { partial: items.len() > 200_000, ..Default::default() };
    items.truncate(200_000);
    let mut cache = repo.update_cache()?.into_iter().map(|r| (r.id(), r)).collect::<BTreeMap<_, _>>();
    // Oldest checked first: every run advances even if the first packages remain offline.
    let mut pending = Vec::new();
    for (id, _) in candidates(&items) {
        let previous = cache.remove(&id);
        pending.push((previous.as_ref().map_or(0, |p| p.checked_at), id, previous));
    }
    pending.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    for (_, id, previous) in pending {
        if started.elapsed() >= UPDATE_RUN_BUDGET {
            summary.partial = true;
            break;
        }
        let mut record = UpdateRecord {
            ecosystem: id.ecosystem.clone(),
            name: id.name.clone(),
            source: id.source,
            status: UpdateStatus::Unknown,
            installed_version: Some(id.version.clone()),
            latest_version: None,
            source_host: Some(id.source_host.clone()),
            checked_at: now,
            failures: 0,
            next_allowed_at: 0,
            lifecycle: Lifecycle::default(),
        };
        let host = match registry_host(&id) {
            Ok(host) => host,
            Err(status) => {
                if status == UpdateStatus::PrivateSkipped {
                    summary.skipped_private += 1;
                }
                record.status = status;
                if let Some(old) = previous {
                    record.lifecycle = old.lifecycle;
                    record.latest_version = old.latest_version;
                    repo.store_update(&record)?;
                }
                continue;
            }
        };
        if !settings.source_approved(host) {
            summary.not_approved += 1;
            record.status = UpdateStatus::NotApproved;
            if let Some(old) = previous {
                record.lifecycle = old.lifecycle;
                record.latest_version = old.latest_version;
                repo.store_update(&record)?;
            }
            continue;
        }
        if let Some(previous) = &previous {
            let fresh = now - previous.checked_at < CACHE_TTL
                && previous.lifecycle.checked_at.is_some()
                && matches!(previous.status, UpdateStatus::Current | UpdateStatus::UpdateAvailable);
            if fresh || previous.next_allowed_at > now {
                summary.cached += 1;
                continue;
            }
        }
        if summary.checked >= UPDATE_CHECKS_PER_RUN || started.elapsed() >= UPDATE_RUN_BUDGET {
            summary.partial = true;
            break;
        }
        summary.checked += 1;
        match registry.metadata(host, &id.name, &id.version) {
            Ok(mut metadata) => {
                record.status = metadata
                    .latest
                    .as_deref()
                    .map_or(UpdateStatus::Unknown, |latest| compare(&id.ecosystem, &id.version, latest));
                if record.status == UpdateStatus::UpdateAvailable {
                    summary.available += 1;
                }
                metadata.lifecycle.checked_at = Some(now);
                if metadata.lifecycle.needs_review() {
                    summary.lifecycle_warnings += 1;
                }
                record.latest_version = metadata.latest;
                record.lifecycle = metadata.lifecycle;
            }
            Err(error) => {
                summary.failed += 1;
                record.failures = previous.as_ref().map_or(0, |p| p.failures) + 1;
                record.next_allowed_at = now + backoff(record.failures);
                record.status = match error {
                    FetchError::Offline(_) => UpdateStatus::Offline,
                    FetchError::NotFound | FetchError::Rejected(_) => UpdateStatus::Unknown,
                };
                if let Some(previous) = previous {
                    record.latest_version = previous.latest_version;
                    record.lifecycle = previous.lifecycle;
                }
            }
        }
        repo.store_update(&record)?;
    }
    Ok(summary)
}

pub fn backoff(failures: i64) -> i64 {
    (15 * 60 * (1_i64 << failures.clamp(1, 8).saturating_sub(1))).min(DAY)
}
pub fn effective_status(record: &UpdateRecord, now: Timestamp) -> UpdateStatus {
    if matches!(record.status, UpdateStatus::Current | UpdateStatus::UpdateAvailable)
        && now >= stale_at(record.checked_at)
    {
        UpdateStatus::Stale
    } else {
        record.status
    }
}
pub fn lifecycle_stale(lifecycle: &Lifecycle, now: Timestamp) -> bool {
    lifecycle.checked_at.is_none_or(|at| now >= stale_at(at))
}
/// First instant at which a check made at `checked_at` counts as stale.
pub fn stale_at(checked_at: Timestamp) -> Timestamp {
    checked_at.saturating_add(STALE_AFTER + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_approval_is_source_bound() {
        let item = InventoryItem::new(ItemCategory::Package, "npm", "x", "lock").version("1.0.0");
        assert_eq!(registry_host(&item.package_id()), Err(UpdateStatus::Unsupported));
        let private = item.clone().source(SourceClass::PrivateRegistry, Some("corp".into()));
        assert_eq!(registry_host(&private.package_id()), Err(UpdateStatus::PrivateSkipped));
        let public = item.source(SourceClass::PublicRegistry, Some("registry.npmjs.org".into()));
        assert_eq!(registry_host(&public.package_id()), Ok("registry.npmjs.org"));
        assert_eq!(backoff(1), 900);
        assert_eq!(backoff(99), DAY);
    }
}
