//! Read-only Docker inventory for approved local contexts. Discovering the
//! contexts reads only their `meta.json`; an approval binds a context to the
//! named pipe it named then. A context that names another pipe later, is no
//! longer local or is gone is "changed": it is not asked until approved
//! again, and cleanup treats its bind mounts as unknown. Nothing is kept.
use super::Core;
use crate::docker::{self, Links};
use crate::domain::*;
use crate::fsread::ScopeRoot;
use crate::{Error, Result};
use std::path::Path;

impl Core {
    /// Contexts the Docker client knows and which of them are approved.
    pub fn docker_overview(&mut self) -> Result<DockerOverview> {
        let approvals = self.repo.docker_approvals()?;
        let found = docker::contexts(&*self.env);
        let mut contexts: Vec<DockerContext> = found
            .iter()
            .map(|(name, host)| {
                let pipe = docker::local_pipe(host);
                let approval = approvals.iter().find(|(approved, ..)| approved == name);
                let check = approval.map(|(_, approved, _)| docker::still_approved(&found, name, approved));
                let note = match (&pipe, &check) {
                    (_, Some(Err(reason))) => reason.clone(),
                    (None, _) => "Kein lokaler Endpunkt; mogumogu fragt nur Engines dieses Rechners".into(),
                    _ => String::new(),
                };
                DockerContext {
                    approved: approval.is_some(),
                    changed: matches!(check, Some(Err(_))),
                    name: name.clone(),
                    host: host.clone(),
                    pipe,
                    note,
                }
            })
            .collect();
        for (name, pipe, _) in &approvals {
            if !contexts.iter().any(|c| &c.name == name) {
                contexts.push(DockerContext {
                    name: name.clone(),
                    host: format!("npipe:////./pipe/{pipe}"),
                    pipe: Some(pipe.clone()),
                    approved: true,
                    changed: true,
                    note: docker::still_approved(&found, name, pipe).err().unwrap_or_default(),
                });
            }
        }
        Ok(DockerOverview { contexts })
    }

    /// Approves a context with a named pipe of this machine for read-only
    /// questions. Remote contexts cannot be approved.
    pub fn approve_docker(&mut self, name: &str) -> Result<DockerOverview> {
        self.repo.require_local()?;
        let (_, host) = docker::contexts(&*self.env)
            .into_iter()
            .find(|(n, _)| n == name)
            .ok_or_else(|| Error::not_found("Diesen Docker-Kontext kennt die Docker-Konfiguration nicht."))?;
        let pipe = docker::local_pipe(&host).ok_or_else(|| {
            Error::blocked("Nur Kontexte mit einer Named Pipe dieses Rechners lassen sich freigeben, keine entfernten.")
        })?;
        self.repo.approve_docker(name, &pipe)?;
        self.docker_overview()
    }

    pub fn revoke_docker(&mut self, name: &str) -> Result<DockerOverview> {
        self.repo.revoke_docker(name)?;
        self.docker_overview()
    }

    /// Asks an approved engine what it runs and stores; with `usage` also
    /// Docker's own disk accounting. Nothing is changed.
    pub fn docker_inventory(&mut self, name: Option<&str>, usage: bool) -> Result<DockerInventory> {
        self.repo.require_local()?;
        let approvals = self.repo.docker_approvals()?;
        let (context, pipe, _) = match name {
            Some(name) => approvals.into_iter().find(|(n, ..)| n == name),
            None if approvals.len() == 1 => approvals.into_iter().next(),
            None => None,
        }
        .ok_or_else(|| Error::blocked("Zuerst einen lokalen Docker-Kontext freigeben (oder den Namen angeben)."))?;
        docker::still_approved(&docker::contexts(&*self.env), &context, &pipe).map_err(Error::blocked)?;
        // Docker reports folders in the spelling it was given; compare an
        // approved project by its real place, an unapproved one as registered.
        let scopes = self.repo.scopes()?;
        let mut projects = Vec::new();
        for project in self.repo.projects()? {
            let scope = scopes.iter().find(|s| {
                s.project_id == Some(project.id) && s.kind == ScopeKind::Project && s.status == ScopeStatus::Approved
            });
            let real = match scope {
                Some(scope) => ScopeRoot::open(Path::new(&scope.path), self.repo.scope_identity(scope.id)?)
                    .ok()
                    .and_then(|root| root.handle().final_path().ok()),
                None => None,
            };
            if let Some(real) = real {
                projects.push((project.name.clone(), real));
            }
            projects.push((project.name, project.path));
        }
        let links = Links { projects, artifacts: self.repo.docker_artifacts()? };
        docker::inventory(&context, &pipe, usage, &links, self.repo.now()).map_err(Error::blocked)
    }
}
