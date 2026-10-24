//! Development candidates from published filename-only discovery. Registration
//! and explicit ownership store metadata without granting read or delete rights.
use super::Core;
use crate::domain::*;
use crate::limits::UI_PAGE;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Outer projects of complete searches that nothing tracks yet, for one
/// confirmed step that registers them and approves them for reading.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectBatch {
    pub projects: Vec<FoundProject>,
    /// Binds the confirmation to exactly this list.
    pub fingerprint: String,
    /// Read scopes approved by the apply step.
    pub approved: Vec<i64>,
    pub skipped: Vec<FoundSkip>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FoundProject {
    pub scope_id: i64,
    pub rel_path: String,
    pub name: String,
    pub path: String,
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FoundSkip {
    pub path: String,
    pub reason: String,
}

impl Core {
    pub fn project_candidates(&mut self) -> Result<Vec<ProjectCandidate>> {
        let scopes = self.repo.scopes()?;
        let projects = self.repo.projects()?;
        self.candidates_with_basis(&scopes, &projects)
    }

    /// One stored finding beyond the dashboard page cap. This only reads
    /// inventory metadata; approval and currentness are still separate facts.
    pub fn discovery_candidate(&mut self, scope_id: i64, rel_path: &str) -> Result<ProjectCandidate> {
        let scope = self.repo.scope(scope_id)?;
        if !matches!(scope.kind, ScopeKind::ProjectSearch | ScopeKind::Project) || scope.status != ScopeStatus::Approved
        {
            return Err(Error::blocked("Der Suchbereich ist nicht freigegeben."));
        }
        let item = self
            .repo
            .published_discovery(scope_id, rel_path)?
            .ok_or_else(|| Error::not_found("Entwicklungsfund nicht gefunden."))?;
        let projects = self.repo.projects()?;
        let resources = self.repo.resources()?;
        map_candidate(&scope, item, &projects, &resources)
    }

    pub(super) fn candidates_with_basis(
        &mut self,
        scopes: &[Scope],
        projects: &[Project],
    ) -> Result<Vec<ProjectCandidate>> {
        let resources = self.repo.resources()?;
        let mut candidates = Vec::new();
        for scope in scopes.iter().filter(|s| {
            matches!(s.kind, ScopeKind::ProjectSearch | ScopeKind::Project) && s.status == ScopeStatus::Approved
        }) {
            for item in self.repo.published_discoveries(scope.id, UI_PAGE as usize - candidates.len())? {
                candidates.push(map_candidate(scope, item, projects, &resources)?);
                if candidates.len() >= UI_PAGE as usize {
                    return Ok(candidates);
                }
            }
        }
        Ok(candidates)
    }

    /// Takes over every current scratchpad finding that nothing tracks yet:
    /// protected, with a review date in 30 days and the owner inferred from
    /// its path. This stores metadata only and grants no read approval.
    pub fn adopt_found_scratchpads(&mut self) -> Result<usize> {
        const REVIEW_DAYS: i64 = 30;
        let candidates = self.project_candidates()?;
        let mut found = Vec::new();
        for candidate in candidates.iter().filter(|c| {
            c.kind == DiscoveryKind::Scratchpad && c.is_dir && c.current && !c.registered && c.resource_id.is_none()
        }) {
            // Redacted or changed paths are never turned into real ones.
            let Ok((scope, item)) = self.current_candidate(candidate.scope_id, &candidate.rel_path) else { continue };
            let path = candidate_path(&scope.path, &candidate.rel_path)?;
            found.push((candidate_name(&path), path, item.detail));
        }
        self.repo.adopt_found_scratchpads(&found, REVIEW_DAYS)
    }

    /// Certain outer projects of complete project searches (one, or all),
    /// leaving out what lies at or below a registered project. Nothing is
    /// registered or approved here.
    pub fn found_projects(&mut self, scope_id: Option<i64>) -> Result<ProjectBatch> {
        let scopes = self.repo.scopes()?;
        let projects = self.repo.projects()?;
        let mut batch = ProjectBatch::default();
        let mut hash = Sha256::new();
        for scope in scopes.iter().filter(|s| {
            s.kind == ScopeKind::ProjectSearch
                && s.status == ScopeStatus::Approved
                && s.current()
                && scope_id.is_none_or(|id| id == s.id)
        }) {
            for item in self.repo.published_root_projects(scope.id, UI_PAGE as usize)? {
                let path = candidate_path(&scope.path, &item.rel_path)?;
                if batch.projects.len() >= UI_PAGE as usize || projects.iter().any(|p| path_at_or_below(&path, &p.path))
                {
                    continue;
                }
                hash.update(scope.id.to_le_bytes());
                hash.update(path.as_bytes());
                hash.update(
                    b"
",
                );
                batch.projects.push(FoundProject {
                    scope_id: scope.id,
                    rel_path: item.rel_path,
                    name: candidate_name(&path),
                    path,
                    evidence: item.detail,
                });
            }
        }
        batch.fingerprint = format!("{:x}", hash.finalize())[..16].to_string();
        Ok(batch)
    }

    /// Registers every previewed project and approves it for reading, one
    /// by one: a folder that cannot be approved is skipped with its reason.
    /// The fingerprint binds this to the preview the user confirmed.
    pub fn adopt_found_projects(&mut self, scope_id: Option<i64>, fingerprint: &str) -> Result<ProjectBatch> {
        let mut batch = self.found_projects(scope_id)?;
        if batch.projects.is_empty() || batch.fingerprint != fingerprint {
            return Err(Error::conflict(
                "Die gefundenen Projekte haben sich seit der Vorschau geändert. Bitte neu öffnen.",
            ));
        }
        for found in &batch.projects {
            // A folder that cannot be approved is not registered either.
            let approved = crate::platform::path_identity(std::path::Path::new(&found.path))
                .map_err(|e| Error::blocked(format!("Ordner nicht freigebbar: {e}")))
                .and_then(|_| self.repo.register_project(&found.name, &found.path))
                .and_then(|id| self.approve_project(id));
            match approved {
                Ok(scope) => batch.approved.push(scope),
                Err(e) => batch.skipped.push(FoundSkip { path: found.path.clone(), reason: e.to_string() }),
            }
        }
        Ok(batch)
    }

    /// Registration is metadata-only and does not approve the child for reading.
    pub fn register_candidate(&mut self, scope_id: i64, rel: &str) -> Result<i64> {
        let (scope, item) = self.current_candidate(scope_id, rel)?;
        if item.evidence.discovery.as_ref().is_some_and(|d| !d.is_dir) {
            return Err(Error::blocked("Einzelne Dateien einem Projekt als Ressource zuordnen."));
        }
        let path = candidate_path(&scope.path, rel)?;
        let name = candidate_name(&path);
        self.repo.register_project(&name, &path)
    }

    /// The user supplies the owner. Filename evidence stays evidence, while
    /// the resource is protected and retains its actual discovered category.
    pub fn adopt_candidate(&mut self, scope_id: i64, rel: &str, project_id: i64, purpose: Option<&str>) -> Result<i64> {
        let (scope, item) = self.current_candidate(scope_id, rel)?;
        let path = candidate_path(&scope.path, rel)?;
        let name = candidate_name(&path);
        self.repo.adopt_discovered_resource(project_id, &name, &path, purpose, &item)
    }

    fn current_candidate(&mut self, scope_id: i64, rel: &str) -> Result<(Scope, InventoryItem)> {
        let scope = self.repo.scope(scope_id)?;
        if !matches!(scope.kind, ScopeKind::ProjectSearch | ScopeKind::Project) || !scope.current() {
            return Err(Error::blocked("Suchwurzel zuerst vollständig neu erfassen."));
        }
        let item = self
            .repo
            .published_discovery(scope_id, rel)?
            .ok_or_else(|| Error::not_found("Entwicklungskandidat nicht gefunden."))?;
        if item.evidence.discovery.as_ref().is_some_and(|d| !d.path_exact) {
            return Err(Error::blocked(
                "Der Fundpfad wurde redigiert oder gekürzt. Bitte den Pfad manuell registrieren.",
            ));
        }
        Ok((scope, item))
    }
}

fn map_candidate(
    scope: &Scope,
    item: InventoryItem,
    projects: &[Project],
    resources: &[Resource],
) -> Result<ProjectCandidate> {
    let path = candidate_path(&scope.path, &item.rel_path)?;
    let discovered = item.evidence.discovery.as_ref();
    let kind = discovered.map_or(DiscoveryKind::Project, |d| d.kind);
    let exact_path = discovered.is_none_or(|d| d.path_exact);
    let resource = resources.iter().find(|r| exact_path && r.path.eq_ignore_ascii_case(&path));
    let exact_project = projects.iter().find(|p| exact_path && p.path.eq_ignore_ascii_case(&path));
    let project = resource.and_then(|r| projects.iter().find(|p| Some(p.id) == r.project_id)).or_else(|| {
        projects.iter().filter(|p| exact_path && path_at_or_below(&path, &p.path)).max_by_key(|p| p.path.len())
    });
    let mut evidence = item.detail;
    if !exact_path {
        evidence.push_str(" · Pfad redigiert/gekürzt; bitte manuell registrieren.");
    } else if resource.is_some() {
        evidence.push_str(" · Besitzer ausdrücklich zugewiesen; Schutz aktiv.");
    } else if project.is_some() && exact_project.is_none() {
        evidence.push_str(" · Projektbezug durch Pfadnähe, noch nicht bestätigt.");
    }
    Ok(ProjectCandidate {
        scope_id: scope.id,
        rel_path: item.rel_path,
        evidence,
        current: scope.current(),
        registered: exact_project.is_some() || resource.is_some(),
        name: item.name,
        kind,
        is_dir: discovered.is_none_or(|d| d.is_dir),
        bytes: item.bytes,
        modified_at: discovered.and_then(|d| d.modified_at),
        project_id: project.map(|p| p.id),
        project: project.map_or_else(String::new, |p| p.name.clone()),
        project_rel_path: discovered.filter(|d| d.has_project_context).map(|_| item.evidence.context),
        resource_id: resource.map(|r| r.id),
        confidence: discovered.map_or(0, |d| d.confidence),
        path_exact: exact_path,
    })
}

fn candidate_name(path: &str) -> String {
    let name = path.trim_end_matches('\\').rsplit('\\').next().unwrap_or("Projekt");
    if name.ends_with(':') { format!("Laufwerk {}", name.trim_end_matches(':')) } else { name.to_string() }
}

fn path_at_or_below(path: &str, root: &str) -> bool {
    path.eq_ignore_ascii_case(root)
        || path
            .get(..root.trim_end_matches('\\').len() + 1)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&format!("{}\\", root.trim_end_matches('\\'))))
}

fn candidate_path(root: &str, rel: &str) -> Result<String> {
    if rel.split('/').any(|p| p == ".." || p == "." || p.contains(['\\', ':'])) {
        return Err(Error::invalid("Ungültiger relativer Kandidatenpfad."));
    }
    crate::validation::windows_path(&if rel.is_empty() {
        root.to_string()
    } else {
        format!("{}\\{}", root.trim_end_matches('\\'), rel.replace('/', "\\"))
    })
}
