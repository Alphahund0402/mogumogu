//! Tidy-up report over approved projects and approved global stores. It is
//! measured on request only and kept in memory until the next report; its
//! sizes and largest entries are saved as history per project, to show what
//! grows. It changes nothing on disk.
use super::Core;
use crate::domain::*;
use crate::{Error, Result, validation};
use crate::{cleanup, inventory};
use std::path::Path;

/// Runs of a project named in its report.
const RUNS_IN_REPORT: i64 = 5;

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
        let held = cleanup::holds(&mut self.repo, &*self.env, &mut cleanup::Places::default())?;
        self.scan_control.begin(targets.len() + stores.len());
        let control = Some(self.scan_control());
        for (project, scope) in &targets {
            if self.scan_control.cancelled() {
                break;
            }
            let identity = self.repo.scope_identity(scope.id)?;
            let kept = self.repo.kept_folders(project.id)?;
            // What could not be determined holds the whole project, as in a plan.
            let mut holds = held.holds.clone();
            holds.extend(held.unknown.iter().map(|reason| Hold {
                kind: HoldKind::Docker,
                location: scope.path.clone(),
                reason: reason.clone(),
            }));
            let walk = inventory::tidy::tidy(Path::new(&scope.path), identity, control.clone(), &holds);
            if let Some(total) = walk.total {
                self.repo.record_project_size(project.id, i64::try_from(total).ok())?;
            }
            let notes: Vec<String> = walk.reason.into_iter().chain(held.unknown.iter().cloned()).collect();
            let runs = self
                .repo
                .runs(Some(project.id), RUNS_IN_REPORT, 0)?
                .into_iter()
                .map(|run| RunBrief {
                    id: run.id,
                    title: run.title(),
                    started_at: run.started_at,
                    artifacts: run.artifact_count,
                })
                .collect();
            let mut measured = ProjectTidy {
                project_id: project.id,
                name: project.name.clone(),
                path: project.path.clone(),
                complete: walk.total.is_some(),
                total: walk.total,
                regenerable: walk.regenerable,
                regenerable_allocated: walk.regenerable_allocated,
                review: walk.review,
                vcs: walk.vcs,
                test_data: walk.test_data,
                agent: walk.agent,
                kept,
                runs,
                growth: None,
                items: walk.items,
                last_change: walk.last_change,
                note: notes.join(" · "),
                checked_at: now,
            };
            // Saved for the history; growth comes from the saved reports.
            self.repo.record_tidy(project.id, &TidyPoint::of(&measured))?;
            measured.growth = growth(&self.repo.tidy_history(project.id)?, now);
            report.projects.retain(|p| p.project_id != project.id);
            report.projects.push(measured);
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
        if project_id.is_none() && !self.scan_control.cancelled() {
            self.measure_path_databases()?;
        }
        self.scan_control.finish();
        report.projects.sort_by(|a, b| b.regenerable.cmp(&a.regenerable).then_with(|| a.name.cmp(&b.name)));
        report.unapproved = projects.iter().filter(|p| approved(p).is_none()).count();
        report.checked_at = Some(now);
        self.tidy = Some(report.clone());
        Ok(report)
    }

    /// Keeps a folder of a project out of every cleanup plan, or suggests it
    /// again, and measures the project anew so the report shows the change.
    /// A keep only ever takes folders out; it never makes anything recoverable.
    /// The saved reports of a project, oldest first, and what grew.
    pub fn tidy_history(&mut self, project_id: i64) -> Result<(Vec<TidyPoint>, Option<Growth>)> {
        self.repo.project(project_id)?;
        let points = self.repo.tidy_history(project_id)?;
        let grown = growth(&points, self.repo.now());
        Ok((points, grown))
    }

    /// A keep is stored with the folder's real place, read through the
    /// project's approval, so it holds for every project reaching the folder.
    pub fn keep_folder(&mut self, project_id: i64, rel_path: &str, keep: bool) -> Result<TidyReport> {
        self.repo.require_local()?;
        let rel_path = validation::project_rel_path(rel_path)?;
        let scope = self.repo.scopes()?.into_iter().find(|s| {
            s.project_id == Some(project_id) && s.kind == ScopeKind::Project && s.status == ScopeStatus::Approved
        });
        if keep {
            let scope = scope.as_ref().ok_or_else(|| {
                Error::blocked(
                    "Behalten braucht die Lesefreigabe des Projekts: nur so lässt sich der Ordner eindeutig benennen.",
                )
            })?;
            let identity = self.repo.scope_identity(scope.id)?;
            let location = cleanup::keep_location(&scope.path, identity, &rel_path).map_err(Error::blocked)?;
            self.repo.keep_folder(project_id, &rel_path, &location)?;
        } else {
            self.repo.unkeep_folder(project_id, &rel_path)?;
        }
        if scope.is_some() { self.tidy_report(Some(project_id)) } else { Ok(self.tidy.clone().unwrap_or_default()) }
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
