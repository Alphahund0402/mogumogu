//! Pure package-report calculation over bounded, published metadata.
//! Receives data and time; opens no files, database connections or network clients.
use super::references::{ReferenceIndex, push};
use crate::clock::Timestamp;
use crate::domain::*;
use crate::limits::{PACKAGE_ANALYSIS_ITEMS as ANALYSIS_LIMIT, PACKAGE_EVIDENCE_ROWS as EVIDENCE_ROWS, PACKAGE_PAGE};
use crate::storage::{PublishedItem, UpdateRecord};
use crate::updates;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct PackageInput<'a> {
    pub items: &'a [PublishedItem],
    pub scopes: &'a [Scope],
    pub projects: &'a [Project],
    pub records: &'a [UpdateRecord],
    pub retained: &'a [String],
    pub now: Timestamp,
}

pub(super) fn analyze(input: PackageInput<'_>, query: &PackageQuery, offset: usize, limit: usize) -> PackageReport {
    let mut coverage = coverage(input.scopes, input.projects);
    coverage.truncated = input.items.len() > ANALYSIS_LIMIT as usize;
    let items = &input.items[..input.items.len().min(ANALYSIS_LIMIT as usize)];
    let references = ReferenceIndex::build(items);
    let retained = input.retained.iter().collect::<BTreeSet<_>>();
    let records = input.records.iter().map(|r| (r.id(), r)).collect::<BTreeMap<_, _>>();
    let mut groups = BTreeMap::<PackageId, PackageFinding>::new();
    let scope_by_id = input.scopes.iter().map(|s| (s.id, s)).collect::<BTreeMap<_, _>>();
    let project_by_id = input.projects.iter().map(|p| (p.id, p.name.as_str())).collect::<BTreeMap<_, _>>();
    for p in items {
        let item = &p.item;
        let is_package =
            item.category == ItemCategory::Package || item.category == ItemCategory::Cache && item.ecosystem == "nuget";
        if !is_package || item.install_state == Some(InstallState::Declared) {
            continue;
        }
        let id = item.package_id();
        let key = id.key();
        let current = scope_by_id.get(&p.scope_id).is_some_and(|s| s.current());
        let record = records.get(&id);
        let finding = groups.entry(id.clone()).or_insert_with(|| PackageFinding {
            id,
            key: key.clone(),
            occurrences: Vec::new(),
            references: Vec::new(),
            possible_references: Vec::new(),
            lifecycle: record.map(|r| r.lifecycle.clone()).unwrap_or_default(),
            lifecycle_stale: record.is_none_or(|r| {
                updates::lifecycle_stale(&r.lifecycle, input.now)
                    || matches!(r.status, UpdateStatus::Offline | UpdateStatus::Unknown | UpdateStatus::NotApproved)
            }),
            local_deprecation: None,
            update: record.map(|r| updates::effective_status(r, input.now)),
            latest_version: record.and_then(|r| r.latest_version.clone()),
            retained: retained.contains(&key),
            coverage: coverage.label(),
            next_action: String::new(),
            review: false,
        });
        if finding.occurrences.len() < EVIDENCE_ROWS {
            finding.occurrences.push(PackageOccurrence {
                scope_id: p.scope_id,
                project_id: p.project_id,
                project: p
                    .project_id
                    .and_then(|id| project_by_id.get(&id).copied())
                    .unwrap_or("Globaler Bestand")
                    .into(),
                rel_path: item.rel_path.clone(),
                state: item.install_state,
                detail: item.detail.clone(),
                current,
                owned_files: item.evidence.owned_files,
            });
        }
        if item.install_state == Some(InstallState::Resolved)
            && let Some(project) = p.project_id.and_then(|id| project_by_id.get(&id))
        {
            push(
                &mut finding.references,
                format!(
                    "{project} · {} · aufgelöst{}",
                    item.rel_path,
                    if current { "" } else { " (älterer vollständiger Stand)" }
                ),
            );
        }
        if finding.local_deprecation.is_none() {
            finding.local_deprecation = item.evidence.deprecated.clone();
        }
    }
    let text = query.text.trim().to_lowercase();
    let mut packages = groups
        .into_values()
        .filter(|p| {
            (text.is_empty() || p.id.name.to_lowercase().contains(&text) || p.id.version.to_lowercase().contains(&text))
                && (query.ecosystem.is_empty() || p.id.ecosystem.eq_ignore_ascii_case(&query.ecosystem))
        })
        .map(|mut finding| {
            advise(&mut finding);
            finding
        })
        .collect::<Vec<_>>();
    let counts = PackageCounts {
        all: packages.len(),
        review: packages.iter().filter(|p| p.review).count(),
        deprecated: packages.iter().filter(|p| deprecated(p)).count(),
        unreferenced: packages.iter().filter(|p| p.references.is_empty()).count(),
        kept: packages.iter().filter(|p| p.retained).count(),
    };
    packages.retain(|p| match query.filter {
        PackageFilter::All => true,
        PackageFilter::Review => p.review,
        PackageFilter::Deprecated => deprecated(p),
        PackageFilter::Unreferenced => p.references.is_empty(),
        PackageFilter::Kept => p.retained,
    });
    packages.sort_by(|a, b| {
        b.lifecycle
            .needs_review()
            .cmp(&a.lifecycle.needs_review())
            .then_with(|| b.review.cmp(&a.review))
            .then_with(|| a.id.cmp(&b.id))
    });
    let total = packages.len();
    let limit = limit.clamp(1, PACKAGE_PAGE);
    let mut packages = packages.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
    // Possible references only explain the shown page; they never change
    // order, filters or the review flag, so the rest skips this matching.
    for finding in &mut packages {
        finding.possible_references = references.possible(&finding.id, &project_by_id);
    }
    PackageReport {
        coverage,
        truncated: offset > 0 || total > packages.len(),
        packages,
        total,
        offset,
        query: query.clone(),
        counts,
    }
}

fn deprecated(finding: &PackageFinding) -> bool {
    finding.lifecycle.needs_review() || finding.local_deprecation.is_some()
}

/// The report depends on the clock only through check staleness, so it stays
/// exact until the first check that is still fresh at `now` turns stale.
pub(super) fn valid_until(records: &[UpdateRecord], now: Timestamp) -> Timestamp {
    records
        .iter()
        .flat_map(|r| [Some(r.checked_at), r.lifecycle.checked_at])
        .flatten()
        .map(updates::stale_at)
        .filter(|&at| at > now)
        .min()
        .unwrap_or(Timestamp::MAX)
}

fn advise(finding: &mut PackageFinding) {
    let warning = finding.lifecycle.needs_review() || finding.local_deprecation.is_some();
    finding.review = warning || !finding.retained && finding.references.is_empty();
    let next_action = if warning && !finding.references.is_empty() {
        concat!(
            "Projektbelege und Abhängigkeitskette prüfen; bei transitiver Abhängigkeit ",
            "das übergeordnete Paket aktualisieren. Änderungen und Tests separat ausführen."
        )
    } else if !finding.references.is_empty() {
        "In Projektmetadaten referenziert. Deklaration und aufgelöste Version vor Änderungen prüfen."
    } else if finding.retained {
        "Lokal zum Behalten vorgemerkt. Upstream-Warnungen bleiben sichtbar."
    } else {
        concat!(
            "Keine passenden Projekt-Lockbelege gefunden. Mögliche Deklarationen, ",
            "globale CLI-Nutzung und Abdeckung prüfen; bei Bedarf behalten."
        )
    };
    finding.next_action = next_action.into();
}

fn coverage(scopes: &[Scope], projects: &[Project]) -> PackageCoverage {
    let complete_projects =
        projects.iter().filter(|p| scopes.iter().any(|s| s.project_id == Some(p.id) && s.current())).count();
    let roots = scopes.iter().filter(|s| s.kind == ScopeKind::ProjectSearch).collect::<Vec<_>>();
    PackageCoverage {
        complete_projects,
        incomplete_projects: projects.len() - complete_projects,
        discovery_roots: roots.len(),
        incomplete_roots: roots.iter().filter(|s| !s.current()).count(),
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::DAY;

    fn record(checked_at: Timestamp, lifecycle_at: Option<Timestamp>) -> UpdateRecord {
        UpdateRecord {
            ecosystem: "npm".into(),
            name: "x".into(),
            status: UpdateStatus::Current,
            installed_version: Some("1.0.0".into()),
            latest_version: None,
            source: SourceClass::PublicRegistry,
            source_host: Some("registry.npmjs.org".into()),
            checked_at,
            failures: 0,
            next_allowed_at: 0,
            lifecycle: Lifecycle { checked_at: lifecycle_at, ..Default::default() },
        }
    }

    #[test]
    fn report_stays_valid_until_the_next_check_turns_stale() {
        assert_eq!(valid_until(&[], 100 * DAY), Timestamp::MAX);
        let records = [record(10 * DAY, Some(8 * DAY)), record(12 * DAY, None)];
        let first = valid_until(&records, 10 * DAY);
        assert_eq!(first, updates::stale_at(8 * DAY));
        assert!(!updates::lifecycle_stale(&records[0].lifecycle, first - 1));
        assert!(updates::lifecycle_stale(&records[0].lifecycle, first));
        // Already stale checks never end validity; the next fresh one does.
        assert_eq!(valid_until(&records, first), updates::stale_at(10 * DAY));
        assert_eq!(updates::effective_status(&records[0], updates::stale_at(10 * DAY) - 1), UpdateStatus::Current);
        assert_eq!(updates::effective_status(&records[0], updates::stale_at(10 * DAY)), UpdateStatus::Stale);
        assert_eq!(valid_until(&records, updates::stale_at(12 * DAY)), Timestamp::MAX);
    }
}
