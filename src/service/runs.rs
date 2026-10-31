//! Runs that cooperating harnesses register through the CLI – only after the
//! person switched registration on. A run and its artifacts are provenance
//! and protection: there is no command that removes a run, an artifact or
//! anything they name, and no run state makes anything recoverable.
use super::Core;
use crate::domain::*;
use crate::storage::NewArtifact;
use crate::{Error, Result, cleanup, privacy, validation};
use serde::{Deserialize, Serialize};

/// Newest runs shown in an overview, with their first artifacts: small
/// enough for one IPC answer.
const RUNS_SHOWN: i64 = 20;
const ARTIFACTS_SHOWN: usize = 10;

/// The project of a run: by id, or the innermost registered project holding
/// a folder, such as the harness's working folder.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRef {
    Id(i64),
    Path(String),
}

/// What a run reports it made: a file or folder in its project, or a Docker
/// object (`kind` docker, with context, object kind and id or name).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRequest {
    pub kind: ArtifactKind,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub docker: Option<(String, DockerObject, String)>,
    #[serde(default)]
    pub purpose: Option<String>,
}

fn starts_within(path: &str, root: &str) -> Option<String> {
    let (folded, root_folded) = (fold_case(path), fold_case(root.trim_end_matches('\\')));
    if folded == root_folded {
        return Some(String::new());
    }
    let skip = root.trim_end_matches('\\').chars().count() + 1;
    folded.starts_with(&format!("{root_folded}\\")).then(|| path.chars().skip(skip).collect())
}

impl Core {
    /// Switches registration by harnesses on or off. The owner accepts
    /// switching on only from the dashboard.
    pub fn allow_agent_runs(&mut self, allowed: bool) -> Result<RunOverview> {
        self.repo.require_local()?;
        let mut settings = self.repo.settings()?;
        settings.agent_runs = allowed;
        self.repo.save_settings(&settings)?;
        self.run_overview(None)
    }

    pub fn run_overview(&mut self, project_id: Option<i64>) -> Result<RunOverview> {
        let runs = self.repo.runs(project_id, RUNS_SHOWN, ARTIFACTS_SHOWN)?;
        Ok(RunOverview { allowed: self.settings()?.agent_runs, runs })
    }

    fn require_runs(&mut self) -> Result<()> {
        self.repo.require_local()?;
        if !self.settings()?.agent_runs {
            return Err(Error::blocked("KI-Läufe werden nicht angenommen; das entscheidet die Person im Dashboard."));
        }
        Ok(())
    }

    fn project_of(&mut self, project: &ProjectRef) -> Result<i64> {
        match project {
            ProjectRef::Id(id) => Ok(self.repo.project(*id)?.id),
            ProjectRef::Path(path) => {
                let path = validation::windows_path(path)?;
                self.repo
                    .projects()?
                    .into_iter()
                    .filter(|p| starts_within(&path, &p.path).is_some())
                    .max_by_key(|p| p.path.len())
                    .map(|p| p.id)
                    .ok_or_else(|| Error::not_found("Kein registriertes Projekt enthält diesen Ordner."))
            }
        }
    }

    pub fn start_run(&mut self, project: &ProjectRef, harness: &str, purpose: Option<&str>) -> Result<AgentRun> {
        self.require_runs()?;
        let harness = validation::name(harness)?;
        let project_id = self.project_of(project)?;
        let id = self.repo.start_run(project_id, &harness, purpose)?;
        self.repo.run(id)
    }

    /// Registers what a run made. A path must lie in the run's project; with
    /// the project's read approval it is stored as its real place, like a
    /// kept folder. Registration protects; it never permits anything.
    pub fn add_run_artifact(&mut self, run_id: i64, artifact: &ArtifactRequest) -> Result<AgentRun> {
        self.require_runs()?;
        let run = self.repo.run(run_id)?;
        let purpose = artifact.purpose.as_deref().map(privacy::label);
        let new = match (artifact.kind, &artifact.path, &artifact.docker) {
            (ArtifactKind::Docker, None, Some((context, object, id))) => {
                let context = validation::name(context)?;
                let plain = |c: char| c.is_ascii_alphanumeric() || "_.:/@-".contains(c);
                if id.is_empty() || id.len() > 200 || !id.chars().all(plain) {
                    return Err(Error::invalid("Docker-Objekt: Id oder Name aus Buchstaben, Ziffern und _.:/@-."));
                }
                NewArtifact {
                    kind: ArtifactKind::Docker,
                    location: String::new(),
                    docker: Some((context, *object, id.clone())),
                    purpose,
                }
            }
            (kind, Some(path), None) if kind != ArtifactKind::Docker => {
                let path = validation::windows_path(path)?;
                let project = self.repo.project(run.project_id)?;
                let rel = starts_within(&path, &project.path)
                    .ok_or_else(|| Error::invalid("Artefakte eines Laufs liegen in seinem Projekt."))?;
                if rel.is_empty() {
                    return Err(Error::invalid(
                        "Der Projektordner selbst würde alles schützen; gib einen Ordner oder eine Datei darin an.",
                    ));
                }
                let rel = validation::project_rel_path(&rel)?;
                let scope = self.repo.scopes()?.into_iter().find(|s| {
                    s.project_id == Some(run.project_id)
                        && s.kind == ScopeKind::Project
                        && s.status == ScopeStatus::Approved
                });
                let location = match scope {
                    Some(scope) => {
                        let identity = self.repo.scope_identity(scope.id)?;
                        cleanup::keep_location(&scope.path, identity, &rel).map_err(Error::blocked)?
                    }
                    // Without an approval nothing is opened; the path stays as written.
                    None => path,
                };
                NewArtifact { kind, location, docker: None, purpose }
            }
            _ => return Err(Error::invalid("Ein Artefakt ist ein Pfad oder, mit der Art docker, ein Docker-Objekt.")),
        };
        self.repo.add_artifact(run_id, &new)?;
        self.repo.run(run_id)
    }

    /// The person releases a finished run: its artifacts are not held any
    /// more. The owner accepts this only from the dashboard. Nothing becomes
    /// recoverable by it; the tidy rules alone decide that.
    pub fn release_run(&mut self, run_id: i64) -> Result<RunOverview> {
        self.repo.release_run(run_id)?;
        self.run_overview(None)
    }

    pub fn set_run_state(&mut self, run_id: i64, state: RunState, note: Option<&str>) -> Result<AgentRun> {
        self.require_runs()?;
        let run = self.repo.run(run_id)?;
        self.repo.set_run_state(run_id, run.state, state, note)?;
        self.repo.run(run_id)
    }
}
