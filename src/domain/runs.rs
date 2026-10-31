//! Runs of AI agents and test harnesses that a cooperating harness
//! registers on request, with what they made. A run is provenance, never an
//! authorization: nothing about a run, its state or its age makes anything
//! removable. Every registered artifact is protected instead.
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    /// What the harness reports; no state is inferred from silence.
    pub enum RunState {
        Running => "running",
        Paused => "paused",
        Succeeded => "succeeded",
        Failed => "failed",
        Cancelled => "cancelled",
    }
}

impl RunState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "läuft",
            Self::Paused => "pausiert",
            Self::Succeeded => "erfolgreich",
            Self::Failed => "fehlgeschlagen",
            Self::Cancelled => "abgebrochen",
        }
    }

    pub fn finished(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    /// A paused run may go on; a finished one stays finished.
    pub fn can_become(self, next: Self) -> bool {
        !self.finished() && self != next
    }
}

string_enum! {
    /// The artifact classes of the cleanup specification (§18).
    pub enum ArtifactKind {
        Workspace => "workspace",
        Scratch => "scratch",
        Diagnostic => "diagnostic",
        Test => "test",
        Code => "code",
        State => "state",
        Output => "output",
        Docker => "docker",
    }
}

impl ArtifactKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Workspace => "Arbeitsordner",
            Self::Scratch => "Scratchpad",
            Self::Diagnostic => "Diagnose",
            Self::Test => "Erzeugter Test",
            Self::Code => "Erzeugter Code",
            Self::State => "Persistenter Zustand",
            Self::Output => "Ausgabe",
            Self::Docker => "Docker-Objekt",
        }
    }
}

string_enum! {
    pub enum DockerObject {
        Container => "container",
        Image => "image",
        Volume => "volume",
        Network => "network",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunArtifact {
    pub id: i64,
    pub kind: ArtifactKind,
    /// Real place of a file or folder; empty for Docker objects.
    pub location: String,
    pub docker: Option<(String, DockerObject, String)>,
    pub purpose: Option<String>,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRun {
    pub id: i64,
    pub project_id: i64,
    pub project: String,
    pub harness: String,
    pub purpose: Option<String>,
    pub state: RunState,
    pub started_at: Timestamp,
    pub updated_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub note: Option<String>,
    /// When the person released the run's protection in the dashboard; its
    /// artifacts are not held any more (nothing becomes recoverable by it).
    #[serde(default)]
    pub released_at: Option<Timestamp>,
    /// All registered artifacts; `artifacts` may list fewer in an overview.
    #[serde(default)]
    pub artifact_count: usize,
    pub artifacts: Vec<RunArtifact>,
}

impl AgentRun {
    /// `#3 claude-code (fehlgeschlagen)`
    pub fn title(&self) -> String {
        format!("#{} {} ({})", self.id, self.harness, self.state.label())
    }
}

/// Whether harnesses may register runs, and the newest runs.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunOverview {
    pub allowed: bool,
    pub runs: Vec<AgentRun>,
}

#[cfg(test)]
mod tests {
    use super::RunState::*;

    #[test]
    fn finished_runs_stay_finished_and_paused_ones_go_on() {
        assert!(Running.can_become(Paused) && Paused.can_become(Running) && Running.can_become(Failed));
        assert!(!Failed.can_become(Running) && !Succeeded.can_become(Failed) && !Running.can_become(Running));
    }
}
