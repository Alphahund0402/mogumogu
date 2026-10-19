//! Assembles the bounded read model for dashboard and `list --json`.
use super::Core;
use crate::Result;
use crate::cleanup;
use crate::clock::{display_relative, parse_iso};
use crate::domain::*;
use crate::limits::{PACKAGE_PAGE, UI_PAGE};
use crate::profiles;
use crate::review::{self, ReviewInput};
use crate::storage::PublishedItem;
use std::collections::BTreeMap;

/// Storage overview groups; a group with any unmeasured member is unknown.
const STORAGE_GROUPS: &[(&str, &[ResourceKind])] = &[
    ("Projektquellen", &[ResourceKind::Source]),
    ("Paket-Caches", &[ResourceKind::Cache]),
    ("Temporär", &[ResourceKind::Temporary]),
    ("Umgebungen", &[ResourceKind::Environment]),
    ("Builds / Ergebnisse", &[ResourceKind::Output, ResourceKind::Results]),
];

impl Core {
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        self.reconcile_sessions()?;
        let demo = self.is_demo();
        let now = self.repo.now();
        let mut scopes = self.repo.scopes()?;
        for scope in &mut scopes {
            scope.watched = self.watched.contains(&scope.id);
        }
        let mut projects = self.repo.projects()?;
        for project in &mut projects {
            project.ecosystem = project
                .ecosystem
                .split(" · ")
                .map(|id| crate::adapters::by_id(id).map_or(id, |a| a.descriptor().name))
                .collect::<Vec<_>>()
                .join(" · ");
            project.coverage = if demo {
                "Beispieldaten".into()
            } else {
                scopes
                    .iter()
                    .find(|s| s.project_id == Some(project.id))
                    .map_or_else(|| "Keine Lesefreigabe – nicht erfasst".into(), Scope::coverage_label)
            };
        }
        let resources = self.repo.resources()?;
        let ai = if demo {
            self.repo.demo_ai()?
        } else {
            // Only AI rows; the package inventory is not loaded for this view.
            let mut items = self.repo.published_items_all(Some(ItemCategory::AiArtifact), UI_PAGE)?;
            items.extend(self.repo.published_items_all(Some(ItemCategory::AiReference), UI_PAGE * 10)?);
            ai_entries(&items, &scopes)
        };
        let activity = self
            .repo
            .activity(20)?
            .into_iter()
            .map(|mut a| {
                if let Some(ts) = parse_iso(&a.created_at) {
                    a.created_at = display_relative(ts, now);
                }
                a
            })
            .collect();
        let mut plans = Vec::new();
        for id in self.repo.plan_ids(20)? {
            plans.push(cleanup::summary(&mut self.repo, id)?);
        }
        let storage = storage_slices(&resources);
        // Review and dashboard share the same bounded read model. Loading these
        // rows again via Core::review would duplicate queries and allocations.
        let sessions = self.repo.sessions()?;
        let owners = self.repo.owners()?;
        let settings = self.settings()?;
        let hints = review::evaluate(&ReviewInput {
            resources: &resources,
            sessions: &sessions,
            owners: &owners,
            settings: &settings,
            now,
            demo,
        });
        let packages = self.packages_with_basis(&PackageQuery::default(), 0, PACKAGE_PAGE, &scopes, &projects)?;
        let discoveries = if demo { Vec::new() } else { self.candidates_with_basis(&scopes, &projects)? };
        let artifacts = if demo { Vec::new() } else { self.artifacts_with_basis(&scopes, &projects)? };
        let system = self.system_with_basis(&scopes);
        let last_scan_at = if demo { None } else { self.repo.last_complete_scan()? };
        let autostart = if demo { None } else { self.autostart() };
        let disks = self.storage_with_basis(&scopes, &projects);
        Ok(Snapshot {
            demo,
            sqlite_version: self.repo.sqlite_version().into(),
            hints,
            sessions,
            owners,
            ecosystems: if demo { Vec::new() } else { self.repo.ecosystem_counts()? },
            settings: Some(settings),
            projects,
            resources,
            ai,
            activity,
            scopes,
            plans,
            storage,
            packages,
            discoveries,
            artifacts,
            system,
            last_scan_at,
            autostart,
            disks,
        })
    }

    /// Tray state without the dashboard read model; the review hints are
    /// evaluated exactly as in [`Core::snapshot`].
    pub fn tray_summary(&mut self) -> Result<TraySummary> {
        self.reconcile_sessions()?;
        let resources = self.repo.resources()?;
        let sessions = self.repo.sessions()?;
        let owners = self.repo.owners()?;
        let settings = self.settings()?;
        let review_count = review::evaluate(&ReviewInput {
            resources: &resources,
            sessions: &sessions,
            owners: &owners,
            settings: &settings,
            now: self.repo.now(),
            demo: self.is_demo(),
        })
        .len();
        let last_scan_at = if self.is_demo() { None } else { self.repo.last_complete_scan()? };
        Ok(TraySummary { demo: self.is_demo(), review_count, settings, last_scan_at })
    }
}

fn storage_slices(resources: &[Resource]) -> Vec<StorageSlice> {
    STORAGE_GROUPS
        .iter()
        .map(|(label, kinds)| {
            let mut members = resources.iter().filter(|r| kinds.contains(&r.kind)).peekable();
            let bytes = if members.peek().is_none() {
                None
            } else {
                members.try_fold(0_i64, |sum, resource| sum.checked_add(resource.bytes?))
            };
            StorageSlice { label: (*label).into(), bytes }
        })
        .collect()
}

/// AI artifacts of the published inventory with their references.
pub(super) fn ai_entries(items: &[PublishedItem], scopes: &[Scope]) -> Vec<AiEntry> {
    let mut references: BTreeMap<(i64, &str, &str), Vec<String>> = BTreeMap::new();
    for p in items.iter().filter(|p| p.item.category == ItemCategory::AiReference) {
        references
            .entry((p.scope_id, p.item.ecosystem.as_str(), p.item.rel_path.as_str()))
            .or_default()
            .push(format!("{} – {}", p.item.name, p.item.detail));
    }
    let catalog = profiles::catalog();
    let roots: BTreeMap<_, _> = scopes.iter().map(|scope| (scope.id, (scope.path.as_str(), scope.kind))).collect();
    items
        .iter()
        .filter(|p| p.item.category == ItemCategory::AiArtifact)
        .take(UI_PAGE as usize)
        .map(|p| {
            let (root, scope_kind) = roots.get(&p.scope_id).copied().unwrap_or(("", ScopeKind::Project));
            let (kind, label) = p.item.name.split_once(": ").unwrap_or((p.item.name.as_str(), ""));
            // Catalog kinds speak of projects; in a user folder the same file is user-wide.
            let kind = match (scope_kind, kind) {
                (ScopeKind::UserAi, "Projekteinstellungen") => "Benutzereinstellungen",
                (_, kind) => kind,
            };
            AiEntry {
                client: catalog.profile(&p.item.ecosystem).map_or_else(|| p.item.ecosystem.clone(), |c| c.name.clone()),
                label: if label.is_empty() { p.item.detail.clone() } else { format!("{label} · {}", p.item.detail) },
                path: format!("{root}\\{}", p.item.rel_path.replace('/', "\\")),
                state: "protected".into(),
                kind: kind.to_string(),
                scope: scope_kind.audience().into(),
                references: references
                    .get(&(p.scope_id, p.item.ecosystem.as_str(), p.item.rel_path.as_str()))
                    .cloned()
                    .unwrap_or_default(),
                scope_id: Some(p.scope_id),
                rel_path: p.item.rel_path.clone(),
                viewable: super::ai_content::viewable_limit(scope_kind, root, &p.item.rel_path, &p.item.ecosystem)
                    .is_some(),
            }
        })
        .collect()
}
