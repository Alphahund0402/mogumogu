//! Tidy-up report over approved projects and approved global stores. It is
//! measured on request only and kept in memory until the next report; it
//! changes nothing on disk.
use super::Core;
use crate::domain::*;
use crate::inventory;
use crate::{Error, Result};
use std::path::Path;

impl Core {
    /// Measures every approved project, or only `project_id`, and with all
    /// projects also the approved global stores. Measured project sizes are
    /// recorded on the way.
    pub fn tidy_report(&mut self, project_id: Option<i64>) -> Result<TidyReport> {
        self.repo.require_local()?;
        let scopes = self.repo.scopes()?;
        let projects = self.repo.projects()?;
        let now = self.repo.now();
        let approved = |project: &Project| {
            scopes.iter().find(|s| {
                s.project_id == Some(project.id) && s.kind == ScopeKind::Project && s.status == ScopeStatus::Approved
            })
        };
        let targets: Vec<(Project, Scope)> = projects
            .iter()
            .filter(|p| project_id.is_none_or(|id| id == p.id))
            .filter_map(|p| approved(p).map(|s| (p.clone(), s.clone())))
            .collect();
        if project_id.is_some() && targets.is_empty() {
            return Err(Error::blocked("Für dieses Projekt fehlt eine Lesefreigabe; ohne sie misst mogumogu nichts."));
        }
        let stores: Vec<Scope> = if project_id.is_none() {
            scopes
                .iter()
                .filter(|s| {
                    s.status == ScopeStatus::Approved
                        && (s.kind.is_cache()
                            || matches!(
                                s.kind,
                                ScopeKind::NpmGlobal
                                    | ScopeKind::UvTools
                                    | ScopeKind::Python
                                    | ScopeKind::CargoTools
                                    | ScopeKind::NugetCache
                            ))
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let mut report =
            if project_id.is_some() { self.tidy.clone().unwrap_or_default() } else { TidyReport::default() };
        self.scan_control.begin(targets.len() + stores.len());
        let control = Some(self.scan_control());
        for (project, scope) in &targets {
            if self.scan_control.cancelled() {
                break;
            }
            let identity = self.repo.scope_identity(scope.id)?;
            let walk = inventory::tidy::tidy(Path::new(&scope.path), identity, control.clone());
            if let Some(total) = walk.total {
                self.repo.record_project_size(project.id, i64::try_from(total).ok())?;
            }
            report.projects.retain(|p| p.project_id != project.id);
            report.projects.push(ProjectTidy {
                project_id: project.id,
                name: project.name.clone(),
                path: project.path.clone(),
                complete: walk.total.is_some(),
                total: walk.total,
                regenerable: walk.regenerable,
                review: walk.review,
                vcs: walk.vcs,
                test_data: walk.test_data,
                items: walk.items,
                last_change: walk.last_change,
                note: walk.reason.unwrap_or_default(),
                checked_at: now,
            });
            self.scan_control.scope_done();
        }
        if project_id.is_none() {
            report.stores.clear();
        }
        for scope in &stores {
            if self.scan_control.cancelled() {
                break;
            }
            let measurement = inventory::measure(Path::new(&scope.path), self.repo.scope_identity(scope.id)?);
            let (class, evidence) = match scope.kind.cache_cleanup() {
                Some((true, evidence)) => (TidyClass::Regenerable, evidence),
                Some((false, evidence)) => (TidyClass::Review, evidence),
                None if scope.kind == ScopeKind::NugetCache => {
                    (TidyClass::Regenerable, "Paket-Cache; Pakete werden bei Bedarf neu geladen")
                }
                None => (TidyClass::Review, "Installierte Werkzeuge oder Pakete; Nutzung unbekannt"),
            };
            report.stores.push(StoreTidy {
                scope_id: scope.id,
                label: scope.kind.label().into(),
                path: scope.path.clone(),
                bytes: measurement.bytes,
                class,
                evidence: evidence.into(),
            });
            self.scan_control.scope_done();
        }
        self.scan_control.finish();
        report.projects.sort_by(|a, b| b.regenerable.cmp(&a.regenerable).then_with(|| a.name.cmp(&b.name)));
        report.unapproved = projects.iter().filter(|p| approved(p).is_none()).count();
        report.checked_at = Some(now);
        self.tidy = Some(report.clone());
        Ok(report)
    }

    /// The last report, without projects whose approval has gone since.
    pub(super) fn current_tidy(&self, scopes: &[Scope]) -> TidyReport {
        let mut report = self.tidy.clone().unwrap_or_default();
        report
            .projects
            .retain(|p| scopes.iter().any(|s| s.project_id == Some(p.project_id) && s.status == ScopeStatus::Approved));
        report.stores.retain(|t| scopes.iter().any(|s| s.id == t.scope_id && s.status == ScopeStatus::Approved));
        report
    }
}
