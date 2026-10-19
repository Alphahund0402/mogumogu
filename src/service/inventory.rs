//! Project registration, identity-bound read approvals, scans and measurements.
use super::Core;
use crate::domain::*;
use crate::inventory::{self, ScanFailure, ScanRequest};
use crate::limits::{ScanLimits, UI_PAGE};
use crate::platform;
use crate::profiles;
use crate::storage::PublishReport;
use crate::{Error, Result, validation};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct ScanReport {
    pub scope_id: i64,
    pub state: Option<GenerationState>,
    pub baseline: bool,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    /// Items of this pass; for a search these are its findings.
    pub items: usize,
    pub problem: Option<String>,
}

impl Core {
    pub fn register_project(&mut self, name: &str, path: &str) -> Result<i64> {
        self.repo.register_project(name, path)
    }

    /// Read approval for a registered project folder, bound to its identity.
    pub fn approve_project(&mut self, project_id: i64) -> Result<i64> {
        let project = self.repo.project(project_id)?;
        let identity = platform::path_identity(Path::new(&project.path))
            .map_err(|e| Error::blocked(format!("Ordner nicht freigebbar: {e}")))?;
        self.repo.approve_scope(ScopeKind::Project, Some(project_id), &project.path, Some(identity))
    }

    /// Read approval for a system-wide manager root or the software list.
    pub fn approve_system(&mut self, kind: ScopeKind, path: Option<&str>) -> Result<i64> {
        match kind {
            ScopeKind::Project => Err(Error::invalid("Projektordner werden über ihr Projekt freigegeben.")),
            ScopeKind::Winget => self.repo.approve_scope(kind, None, "Registry: installierte Programme", None),
            ScopeKind::UserAi => {
                // Only a known client folder: approving the whole profile or an
                // arbitrary folder as "user configuration" is not possible.
                let path = validation::windows_path(
                    path.ok_or_else(|| Error::invalid("Für Benutzer-KI-Konfiguration einen Ordner angeben."))?,
                )?;
                let known = inventory::system::USER_AI_ROOTS
                    .iter()
                    .filter_map(|root| root.path(&inventory::system::ProcessEnv))
                    .any(|candidate| candidate.to_string_lossy().eq_ignore_ascii_case(&path));
                if !known {
                    return Err(Error::blocked(
                        "Nur bekannte Konfigurationsordner von KI-Clients (z. B. %USERPROFILE%\\.claude) sind möglich.",
                    ));
                }
                let identity = platform::path_identity(Path::new(&path))
                    .map_err(|e| Error::blocked(format!("Ordner nicht freigebbar: {e}")))?;
                self.repo.approve_scope(kind, None, &path, Some(identity))
            }
            // A whole drive only for the names-only search, and only a local
            // disk: network, optical and removable media stay folder-only.
            ScopeKind::ProjectSearch if path.is_some_and(|p| validation::drive_root(p).is_ok()) => {
                let path = validation::drive_root(path.unwrap_or_default())?;
                if !platform::local_fixed_path(Path::new(&path)) {
                    return Err(Error::blocked("Nur lokale Festplatten lassen sich als Ganzes durchsuchen."));
                }
                let identity = platform::path_identity(Path::new(&path))
                    .map_err(|e| Error::blocked(format!("Laufwerk nicht freigebbar: {e}")))?;
                self.repo.approve_scope(kind, None, &path, Some(identity))
            }
            _ => {
                let path = match path {
                    Some(path) => validation::windows_path(path)?,
                    None => default_system_root(kind)?,
                };
                let identity = platform::path_identity(Path::new(&path))
                    .map_err(|e| Error::blocked(format!("Ordner nicht freigebbar: {e}")))?;
                self.repo.approve_scope(kind, None, &path, Some(identity))
            }
        }
    }

    pub fn revoke_scope(&mut self, scope_id: i64) -> Result<()> {
        self.repo.revoke_scope(scope_id)
    }

    /// One static pass over an approved scope, stored as a generation.
    pub fn scan(&mut self, scope_id: i64) -> Result<ScanReport> {
        self.scan_control.begin(1);
        let report = self.scan_one(scope_id);
        self.scan_control.finish();
        report
    }

    /// All approved scopes in turn. A cancel request ends the current pass
    /// at its next entry and skips the remaining scopes.
    pub fn scan_all(&mut self) -> Result<Vec<ScanReport>> {
        let ids: Vec<i64> =
            self.repo.scopes()?.iter().filter(|s| s.status == ScopeStatus::Approved).map(|s| s.id).collect();
        self.scan_control.begin(ids.len());
        let mut reports = Vec::with_capacity(ids.len());
        for id in ids {
            if self.scan_control.cancelled() {
                break;
            }
            let report = self.scan_one(id);
            self.scan_control.scope_done();
            match report {
                Ok(report) => reports.push(report),
                Err(e) => {
                    self.scan_control.finish();
                    return Err(e);
                }
            }
        }
        self.scan_control.finish();
        Ok(reports)
    }

    fn scan_one(&mut self, scope_id: i64) -> Result<ScanReport> {
        self.repo.require_local()?;
        let scope = self.repo.scope(scope_id)?;
        match scope.status {
            ScopeStatus::Approved | ScopeStatus::Unavailable => {}
            ScopeStatus::IdentityChanged => {
                return Err(Error::blocked("Der freigegebene Ordner wurde ersetzt. Bitte bewusst neu freigeben."));
            }
            ScopeStatus::Revoked => return Err(Error::blocked("Lesefreigabe wurde widerrufen.")),
        }
        let request = ScanRequest {
            kind: scope.kind,
            path: PathBuf::from(&scope.path),
            identity: self.repo.scope_identity(scope_id)?,
            limits: if validation::drive_root(&scope.path).is_ok() {
                ScanLimits::drive_search()
            } else {
                ScanLimits::default()
            },
            control: Some(self.scan_control()),
        };
        let generation = self.repo.begin_generation(scope_id, &profiles::catalog().basis())?;
        let outcome = inventory::scan(&request);
        let items = outcome.result.items.len();
        let cancelled = outcome.result.limit_reason.as_deref() == Some(crate::fsread::CANCELLED);
        let problem = match &outcome.failure {
            Some(ScanFailure::IdentityChanged) => {
                self.repo.set_scope_status(scope_id, ScopeStatus::IdentityChanged, Some("Ordner ersetzt"))?;
                Some("Ordner wurde ersetzt; Freigabe blockiert.".to_string())
            }
            Some(ScanFailure::Unavailable(reason)) => {
                self.repo.set_scope_status(scope_id, ScopeStatus::Unavailable, Some(reason))?;
                Some(reason.clone())
            }
            None => {
                if scope.status != ScopeStatus::Approved {
                    self.repo.set_scope_status(scope_id, ScopeStatus::Approved, None)?;
                }
                cancelled.then(|| "Abgebrochen; der letzte vollständige Stand bleibt sichtbar.".to_string())
            }
        };
        let PublishReport { state, baseline, added, removed, changed, .. } =
            self.repo.finish_generation(generation, &outcome.result)?;
        Ok(ScanReport { scope_id, state, baseline, added, removed, changed, items, problem })
    }

    /// Measures a managed folder or an approved scope; unknown stays unknown.
    pub fn measure_resource(&mut self, resource_id: i64) -> Result<inventory::Measurement> {
        let resource = self.repo.resource(resource_id)?;
        let identity = self.repo.resource_identity(resource_id)?;
        if resource.acquisition != Acquisition::Managed || identity.is_none() {
            return Err(Error::blocked(
                "Größen werden nur für von mogumogu angelegte Ordner oder freigegebene Suchbereiche gemessen.",
            ));
        }
        let measurement = inventory::measure(Path::new(&resource.path), identity);
        self.repo.record_size(resource_id, measurement.bytes.map(|b| b as i64))?;
        Ok(measurement)
    }

    pub fn measure_scope(&mut self, scope_id: i64) -> Result<inventory::Measurement> {
        let scope = self.repo.scope(scope_id)?;
        if scope.status != ScopeStatus::Approved || scope.kind == ScopeKind::Winget {
            return Err(Error::blocked("Nur freigegebene Ordner können gemessen werden."));
        }
        if validation::drive_root(&scope.path).is_ok() {
            return Err(Error::blocked(
                "Ein ganzes Laufwerk wird nicht ausgemessen; seine Belegung steht unter Speicher.",
            ));
        }
        let measurement = inventory::measure(Path::new(&scope.path), self.repo.scope_identity(scope_id)?);
        if let Some(project) = scope.project_id {
            self.repo.record_project_size(project, measurement.bytes.map(|b| b as i64))?;
        }
        Ok(measurement)
    }

    pub fn inventory_artifacts(&mut self) -> Result<Vec<ArtifactFinding>> {
        let scopes = self.repo.scopes()?;
        let projects = self.repo.projects()?;
        self.artifacts_with_basis(&scopes, &projects)
    }

    pub(super) fn artifacts_with_basis(
        &mut self,
        scopes: &[Scope],
        projects: &[Project],
    ) -> Result<Vec<ArtifactFinding>> {
        let mut artifacts = Vec::new();
        for category in [ItemCategory::Environment, ItemCategory::Output, ItemCategory::Cache] {
            for p in self.repo.published_items_all(Some(category), UI_PAGE)? {
                artifacts.push(ArtifactFinding {
                    scope_id: p.scope_id,
                    name: p.item.name,
                    rel_path: p.item.rel_path,
                    category,
                    project: projects
                        .iter()
                        .find(|project| Some(project.id) == p.project_id)
                        .map_or("Globaler Bestand", |p| p.name.as_str())
                        .into(),
                    detail: p.item.detail,
                    current: scopes.iter().find(|s| s.id == p.scope_id).is_some_and(|s| s.current()),
                });
                if artifacts.len() >= UI_PAGE as usize {
                    return Ok(artifacts);
                }
            }
        }
        Ok(artifacts)
    }
}

fn default_system_root(kind: ScopeKind) -> Result<String> {
    let from_env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let path = match kind {
        ScopeKind::Scoop => from_env("SCOOP").or_else(|| from_env("USERPROFILE").map(|home| format!(r"{home}\scoop"))),
        ScopeKind::Chocolatey => from_env("ChocolateyInstall").or_else(|| Some(r"C:\ProgramData\chocolatey".into())),
        ScopeKind::NpmGlobal => from_env("npm_config_prefix")
            .map(|p| format!(r"{p}\node_modules"))
            .or_else(|| from_env("APPDATA").map(|p| format!(r"{p}\npm\node_modules"))),
        // Older uv versions keep tools directly below `uv` (SYSTEMERKENNUNG);
        // an existing folder wins over the current default name.
        ScopeKind::UvTools => from_env("UV_TOOL_DIR").or_else(|| {
            from_env("APPDATA").map(|p| {
                let current = format!(r"{p}\uv\data\tools");
                let older = format!(r"{p}\uv\tools");
                if !Path::new(&current).is_dir() && Path::new(&older).is_dir() { older } else { current }
            })
        }),
        ScopeKind::CargoTools => from_env("CARGO_INSTALL_ROOT")
            .or_else(|| from_env("CARGO_HOME"))
            .or_else(|| from_env("USERPROFILE").map(|p| format!(r"{p}\.cargo"))),
        ScopeKind::NugetCache => {
            from_env("NUGET_PACKAGES").or_else(|| from_env("USERPROFILE").map(|p| format!(r"{p}\.nuget\packages")))
        }
        _ => None,
    };
    let path = path.ok_or_else(|| Error::invalid("Standardpfad nicht ermittelbar; bitte --path angeben."))?;
    validation::windows_path(&path)
}
