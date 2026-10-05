use serde::{Deserialize, Serialize};

string_enum! {
    /// Review hints are priorities, not verdicts (PROJEKTPLAN §10.1).
    pub enum HintKind {
        Inactive => "inactive",
        ReviewDue => "review_due",
        PossiblyOrphaned => "possibly_orphaned",
        Unclear => "unclear",
    }
}

impl HintKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inactive => "Inaktiv im Beobachtungszeitraum",
            Self::ReviewDue => "Prüftermin erreicht",
            Self::PossiblyOrphaned => "Möglicherweise verwaist",
            Self::Unclear => "Unklar",
        }
    }
}

/// One explained review candidate. `plannable` only says a cleanup *plan*
/// may be drafted; blockers are listed and protection always wins.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReviewHint {
    pub resource_id: i64,
    pub resource_name: String,
    pub kind: HintKind,
    pub trigger: String,
    pub evidence: String,
    pub observed_period: String,
    pub owner: String,
    pub uncertainty: String,
    pub next_action: String,
    pub protected: bool,
    pub plannable: bool,
    pub blockers: Vec<String>,
    pub bytes: Option<i64>,
}
