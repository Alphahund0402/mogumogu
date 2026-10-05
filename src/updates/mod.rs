//! Source-bound update hints (F-20; PROJEKTPLAN §11.3).
//!
//! Off by default. A request is only sent when the global switch is on, the
//! package's source is a *known public* registry recorded in a lockfile or
//! environment, and that registry host is approved locally. Private or
//! unknown sources are never sent to a public service. Results are hints:
//! nothing is installed and no lockfile is changed. Failures become
//! `offline`/`unknown`, never "current".
#[cfg(feature = "network")]
mod http;

#[cfg(feature = "network")]
pub use http::HttpRegistry;

use crate::Result;
use crate::clock::{DAY, Timestamp};
use crate::domain::{InstallState, InventoryItem, ItemCategory, Settings, SourceClass, UpdateStatus};
use crate::limits::{UPDATE_CHECKS_PER_RUN, UPDATE_RUN_BUDGET};
use crate::storage::{PublishedItem, Repository, UpdateRecord};
use std::collections::BTreeMap;
use std::time::Instant;

/// Cached results are reused for a day; older ones are shown as stale.
const CACHE_TTL: i64 = DAY;
const STALE_AFTER: i64 = 7 * DAY;

#[derive(Debug)]
pub enum FetchError {
    Offline(String),
    NotFound,
    Rejected(String),
}

/// One public registry lookup. Implementations must not send credentials.
pub trait Registry {
    fn latest(&self, host: &str, name: &str) -> std::result::Result<String, FetchError>;
}

/// Registry used when the `network` feature is disabled.
#[derive(Debug, Default)]
pub struct NoNetwork;

impl Registry for NoNetwork {
    fn latest(&self, _: &str, _: &str) -> std::result::Result<String, FetchError> {
        Err(FetchError::Rejected("Netzwerkfunktion nicht einkompiliert".into()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct UpdateSummary {
    pub checked: usize,
    pub available: usize,
    pub skipped_private: usize,
    pub not_approved: usize,
    pub failed: usize,
    pub cached: usize,
}

/// Registry host for an inventory item, if the item may ever be checked.
fn registry_host(item: &InventoryItem) -> std::result::Result<&'static str, UpdateStatus> {
    match item.source {
        SourceClass::PrivateRegistry | SourceClass::Path | SourceClass::Git => {
            return Err(UpdateStatus::PrivateSkipped);
        }
        SourceClass::Unknown => return Err(UpdateStatus::Unsupported),
        SourceClass::PublicRegistry => {}
    }
    match (item.ecosystem.as_str(), item.source_host.as_deref()) {
        ("npm", Some("registry.npmjs.org")) => Ok("registry.npmjs.org"),
        ("pip" | "uv", Some("pypi.org" | "files.pythonhosted.org")) => Ok("pypi.org"),
        ("cargo", Some("crates.io")) => Ok("crates.io"),
        _ => Err(UpdateStatus::Unsupported),
    }
}

/// Distinct checkable packages (resolved or installed with a version).
fn candidates(items: &[PublishedItem]) -> BTreeMap<(String, String), InventoryItem> {
    let mut out = BTreeMap::new();
    for published in items {
        let item = &published.item;
        let concrete = matches!(item.install_state, Some(InstallState::Resolved | InstallState::Installed));
        if item.category == ItemCategory::Package && concrete && item.version.is_some() {
            out.entry((item.ecosystem.clone(), item.name.clone())).or_insert_with(|| item.clone());
        }
    }
    out
}

pub fn run(repo: &mut Repository, registry: &dyn Registry, settings: &Settings) -> Result<UpdateSummary> {
    let now = repo.now();
    let started = Instant::now();
    let items = repo.published_items_all(Some(ItemCategory::Package), 200_000)?;
    let mut summary = UpdateSummary::default();
    for ((ecosystem, name), item) in candidates(&items) {
        let installed = item.version.clone();
        let mut record = UpdateRecord {
            ecosystem: ecosystem.clone(),
            name: name.clone(),
            status: UpdateStatus::Unknown,
            installed_version: installed.clone(),
            latest_version: None,
            source_host: item.source_host.clone(),
            checked_at: now,
            failures: 0,
            next_allowed_at: 0,
        };
        let host = match registry_host(&item) {
            Ok(host) => host,
            Err(status) => {
                if status == UpdateStatus::PrivateSkipped {
                    summary.skipped_private += 1;
                }
                record.status = status;
                repo.store_update(&record)?;
                continue;
            }
        };
        if !settings.source_approved(host) {
            summary.not_approved += 1;
            record.status = UpdateStatus::NotApproved;
            repo.store_update(&record)?;
            continue;
        }
        let previous = repo.update_record(&ecosystem, &name)?;
        if let Some(previous) = &previous {
            let fresh = previous.installed_version == installed
                && now - previous.checked_at < CACHE_TTL
                && matches!(previous.status, UpdateStatus::Current | UpdateStatus::UpdateAvailable);
            if fresh || previous.next_allowed_at > now {
                summary.cached += 1;
                continue;
            }
        }
        if summary.checked >= UPDATE_CHECKS_PER_RUN || started.elapsed() > UPDATE_RUN_BUDGET {
            break;
        }
        summary.checked += 1;
        match registry.latest(host, &name) {
            Ok(latest) => {
                record.status = compare(installed.as_deref().unwrap_or(""), &latest);
                if record.status == UpdateStatus::UpdateAvailable {
                    summary.available += 1;
                }
                record.latest_version = Some(crate::privacy::label(&latest));
            }
            Err(error) => {
                summary.failed += 1;
                let failures = previous.as_ref().map_or(0, |p| p.failures) + 1;
                record.failures = failures;
                record.next_allowed_at = now + backoff(failures);
                record.status = match error {
                    FetchError::Offline(_) => UpdateStatus::Offline,
                    FetchError::NotFound | FetchError::Rejected(_) => UpdateStatus::Unknown,
                };
                record.latest_version = previous.and_then(|p| p.latest_version);
            }
        }
        repo.store_update(&record)?;
    }
    Ok(summary)
}

/// Exponential backoff: 15 minutes doubling up to one day.
pub fn backoff(failures: i64) -> i64 {
    let minutes = 15_i64.saturating_mul(1_i64 << failures.clamp(1, 7).saturating_sub(1));
    (minutes * 60).min(DAY)
}

/// Displayed status: old results become stale instead of staying "current".
pub fn effective_status(record: &UpdateRecord, now: Timestamp) -> UpdateStatus {
    if matches!(record.status, UpdateStatus::Current | UpdateStatus::UpdateAvailable)
        && now - record.checked_at > STALE_AFTER
    {
        UpdateStatus::Stale
    } else {
        record.status
    }
}

/// Numeric comparison of dotted versions; pre-release tails are ignored.
/// Unparseable versions are reported as unknown, not as equal.
pub fn compare(installed: &str, latest: &str) -> UpdateStatus {
    fn parts(version: &str) -> Option<Vec<u64>> {
        let core = version.trim().trim_start_matches(['v', '=']).split(['-', '+']).next()?;
        let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
        parts.filter(|p| !p.is_empty())
    }
    match (parts(installed), parts(latest)) {
        (Some(a), Some(b)) => {
            let len = a.len().max(b.len());
            let pad = |v: Vec<u64>| v.into_iter().chain(std::iter::repeat(0)).take(len).collect::<Vec<_>>();
            if pad(b) > pad(a) { UpdateStatus::UpdateAvailable } else { UpdateStatus::Current }
        }
        _ => UpdateStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert_eq!(compare("1.2.3", "1.10.0"), UpdateStatus::UpdateAvailable);
        assert_eq!(compare("2.0.0", "2.0"), UpdateStatus::Current);
        assert_eq!(compare("1.0.0-beta.1", "1.0.0"), UpdateStatus::Current);
        assert_eq!(compare("abc", "1.0"), UpdateStatus::Unknown);
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff(1), 15 * 60);
        assert_eq!(backoff(2), 30 * 60);
        assert_eq!(backoff(50), DAY.min(15 * 60 * 64));
    }

    #[test]
    fn private_and_unknown_sources_are_never_hosts() {
        let base = InventoryItem::new(ItemCategory::Package, "npm", "x", "package-lock.json").version("1.0.0");
        let private = base.clone().source(SourceClass::PrivateRegistry, Some("npm.corp".into()));
        assert_eq!(registry_host(&private), Err(UpdateStatus::PrivateSkipped));
        assert_eq!(registry_host(&base), Err(UpdateStatus::Unsupported));
        let public = base.source(SourceClass::PublicRegistry, Some("registry.npmjs.org".into()));
        assert_eq!(registry_host(&public), Ok("registry.npmjs.org"));
    }
}
