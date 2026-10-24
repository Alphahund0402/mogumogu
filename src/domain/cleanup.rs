use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    /// Plan lifecycle (PROJEKTPLAN §10.3).
    pub enum PlanState {
        Draft => "draft",
        Approved => "approved",
        Revalidating => "revalidating",
        Executing => "executing",
        Completed => "completed",
        Partial => "partial",
        Failed => "failed",
        Cancelled => "cancelled",
        Invalidated => "invalidated",
        RecoveryRequired => "recovery_required",
    }
}

impl PlanState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Entwurf",
            Self::Approved => "Bestätigt",
            Self::Revalidating => "Wird erneut geprüft",
            Self::Executing => "Wird ausgeführt",
            Self::Completed => "Abgeschlossen",
            Self::Partial => "Teilweise ausgeführt",
            Self::Failed => "Fehlgeschlagen",
            Self::Cancelled => "Abgebrochen",
            Self::Invalidated => "Ungültig geworden",
            Self::RecoveryRequired => "Abgleich erforderlich",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Partial | Self::Failed | Self::Cancelled | Self::Invalidated)
    }

    pub fn can_become(self, next: Self) -> bool {
        use PlanState::*;
        matches!(
            (self, next),
            (Draft, Approved | Cancelled)
                | (Approved, Revalidating | Cancelled)
                | (Revalidating, Invalidated | Cancelled | Executing)
                | (Executing, Completed | Partial | Failed | Cancelled | RecoveryRequired)
                | (RecoveryRequired, Partial | Completed | Failed)
        )
    }
}

string_enum! {
    /// Per-folder action journal state. `Started` is written durably before
    /// the folder moves.
    pub enum JournalState {
        Planned => "planned",
        Started => "started",
        Done => "done",
        Failed => "failed",
        /// After a crash: the verified folder is no longer at its place.
        ReconciledMissing => "reconciled_missing",
        /// After a crash: the folder is still there; never retried blindly.
        Pending => "pending",
        /// After a crash: a different folder now occupies the location.
        Blocked => "blocked",
    }
}

impl JournalState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Planned => "Geplant",
            Self::Started => "Gestartet",
            Self::Done => "Im Papierkorb",
            Self::Failed => "Nicht verschoben",
            Self::ReconciledMissing => "Nicht mehr am Ort",
            Self::Pending => "Noch vorhanden",
            Self::Blocked => "Unklar",
        }
    }
}

/// One folder of a plan, below the root of an approved project.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlanTarget {
    pub id: i64,
    pub rel_path: String,
    pub label: String,
    /// The file that rebuilds it, e.g. "package-lock.json daneben".
    pub evidence: String,
    pub bytes: i64,
    pub blockers: Vec<String>,
    pub state: JournalState,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlanSummary {
    pub id: i64,
    pub state: PlanState,
    pub created_at: Timestamp,
    pub fingerprint: String,
    #[serde(default)]
    pub project_id: Option<i64>,
    pub project: String,
    /// Root of the approved project the targets lie in.
    pub root: String,
    pub targets: Vec<PlanTarget>,
    /// What stays: sources, lockfiles, version control and everything to review.
    pub kept: Vec<String>,
    pub recovery: String,
    pub done: i64,
    pub failed: i64,
    pub note: Option<String>,
}

impl PlanSummary {
    pub fn executable(&self) -> bool {
        self.targets.iter().all(|t| t.blockers.is_empty()) && !self.targets.is_empty()
    }
    /// First 12 hex digits: what a person confirms when approving.
    pub fn short_fingerprint(&self) -> &str {
        &self.fingerprint[..self.fingerprint.len().min(12)]
    }
    pub fn bytes(&self) -> i64 {
        self.targets.iter().map(|t| t.bytes).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::PlanState::*;

    #[test]
    fn plans_cannot_skip_revalidation() {
        assert!(!Approved.can_become(Executing));
        assert!(Revalidating.can_become(Executing));
        assert!(!Invalidated.can_become(Approved));
        assert!(!Completed.can_become(Executing));
    }
}
