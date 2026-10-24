//! Pure package-report calculation over bounded, published metadata.
//! Receives data only; opens no files, database connections or network clients.
use super::references::{ReferenceIndex, push};
use crate::domain::*;
use crate::limits::{PACKAGE_ANALYSIS_ITEMS as ANALYSIS_LIMIT, PACKAGE_EVIDENCE_ROWS as EVIDENCE_ROWS, PACKAGE_PAGE};
use crate::storage::PublishedItem;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct PackageInput<'a> {
    pub items: &'a [PublishedItem],
    pub scopes: &'a [Scope],
    pub projects: &'a [Project],
    pub retained: &'a [String],
}

pub(super) fn analyze(input: PackageInput<'_>, query: &PackageQuery, offset: usize, limit: usize) -> PackageReport {
    let mut coverage = coverage(input.scopes, input.projects);
    coverage.truncated = input.items.len() > ANALYSIS_LIMIT as usize;
    let items = &input.items[..input.items.len().min(ANALYSIS_LIMIT as usize)];
    let references = ReferenceIndex::build(items);
    let retained = input.retained.iter().collect::<BTreeSet<_>>();
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
        let finding = groups.entry(id.clone()).or_insert_with(|| PackageFinding {
            id,
            key: key.clone(),
            occurrences: Vec::new(),
            references: Vec::new(),
            possible_references: Vec::new(),
            local_deprecation: None,
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
        deprecated(b).cmp(&deprecated(a)).then_with(|| b.review.cmp(&a.review)).then_with(|| a.id.cmp(&b.id))
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
    finding.local_deprecation.is_some()
}

fn advise(finding: &mut PackageFinding) {
    let warning = deprecated(finding);
    finding.review = warning || !finding.retained && finding.references.is_empty();
    let next_action = if warning && !finding.references.is_empty() {
        concat!(
            "Projektbelege und Abhängigkeitskette prüfen; bei transitiver Abhängigkeit ",
            "das übergeordnete Paket aktualisieren. Änderungen und Tests separat ausführen."
        )
    } else if !finding.references.is_empty() {
        "In Projektmetadaten referenziert. Deklaration und aufgelöste Version vor Änderungen prüfen."
    } else if finding.retained {
        "Lokal zum Behalten vorgemerkt. Lockfile-Hinweise bleiben sichtbar."
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
