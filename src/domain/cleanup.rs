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
    /// Per-entry action journal state. `Started` is written durably before
    /// the filesystem is touched.
    pub enum JournalState {
        Planned => "planned",
        Started => "started",
        Done => "done",
        Failed => "failed",
        /// After a crash: the verified object no longer exists.
        ReconciledMissing => "reconciled_missing",
        /// After a crash: the object still exists; never retried blindly.
        Pending => "pending",
        /// After a crash: a different object now occupies the location.
        Blocked => "blocked",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlanTarget {
    pub id: i64,
    pub resource_id: i64,
    pub resource_name: String,
    pub path: String,
    pub entries: i64,
    pub bytes: i64,
    pub blockers: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlanSummary {
    pub id: i64,
    pub state: PlanState,
    pub created_at: Timestamp,
    pub fingerprint: String,
    pub targets: Vec<PlanTarget>,
    /// What stays: sources, results, environment and protected content.
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
