//! Expendability contract without filesystem access (PROJEKTPLAN §10.2).
//! Every rule adds a blocker; a plan is executable only without blockers.
use crate::domain::{Acquisition, Resource, ResourceKind, Session};

/// Blockers derivable from stored facts. Filesystem checks follow in the
/// planner (identity, links, protected children, disposable root).
pub fn static_blockers(resource: &Resource, sessions: &[Session]) -> Vec<String> {
    let mut blockers = Vec::new();
    if resource.kind != ResourceKind::Temporary {
        blockers.push(format!(
            "{} wird nicht bereinigt: nur verwaltete Temp-Ordner sind in diesem Stand zulässig.",
            resource.kind.label()
        ));
    }
    if resource.acquisition != Acquisition::Managed {
        blockers.push("Nicht von mogumogu angelegt; Inhalt und Besitz sind nicht belegt.".into());
    }
    if resource.protected {
        blockers.push("Geschützt – Schutz gewinnt gegenüber jeder Regel.".into());
    }
    if !resource.expendable {
        blockers.push("Nicht ausdrücklich lokal als entbehrlich markiert.".into());
    }
    if resource.parent_id.is_none() {
        blockers.push("Kein zugehöriger verwalteter Scratchpad.".into());
    }
    if sessions.is_empty() {
        blockers.push("Kein verwalteter Lauf belegt den Zweck dieser Ausgabe.".into());
    }
    for session in sessions.iter().filter(|s| s.state.locks_resources()) {
        blockers.push(format!("Session {} ist „{}“ – Ressourcen bleiben gesperrt.", session.id, session.state.label()));
    }
    if sessions.iter().any(|s| s.exit_code.is_some_and(|c| c != 0)) {
        blockers.push("Ein Lauf endete mit Fehler – Diagnose bleibt standardmäßig erhalten.".into());
    }
    blockers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Observation, Origin, SessionOrigin, SessionState};

    fn temp(protected: bool, expendable: bool) -> Resource {
        Resource {
            id: 2,
            project_id: Some(1),
            name: "t".into(),
            path: r"C:\x\temporary".into(),
            kind: ResourceKind::Temporary,
            bytes: None,
            state: Observation::Unknown,
            evidence: String::new(),
            protected,
            origin: Origin::Registered,
            acquisition: Acquisition::Managed,
            parent_id: Some(1),
            purpose: None,
            review_at: None,
            expendable,
            last_activity_at: None,
            created_at: None,
        }
    }

    fn session(state: SessionState, exit_code: Option<i64>) -> Session {
        Session {
            id: 7,
            scratch_id: 1,
            purpose: String::new(),
            state,
            origin: SessionOrigin::Managed,
            process: None,
            job_name: None,
            started_at: 0,
            heartbeat_at: None,
            ended_at: None,
            exit_code,
            remaining_processes: None,
            note: None,
        }
    }

    #[test]
    fn only_explicit_completed_managed_temp_output_qualifies() {
        let done = [session(SessionState::CompletedVerified, Some(0))];
        assert!(static_blockers(&temp(false, true), &done).is_empty());
        assert!(!static_blockers(&temp(true, true), &done).is_empty(), "protection wins");
        assert!(!static_blockers(&temp(false, false), &done).is_empty(), "age or name is not enough");
        assert!(!static_blockers(&temp(false, true), &[]).is_empty());
        assert!(!static_blockers(&temp(false, true), &[session(SessionState::InterruptedUnknown, None)]).is_empty());
        assert!(!static_blockers(&temp(false, true), &[session(SessionState::CompletedVerified, Some(1))]).is_empty());
        let mut results = temp(false, true);
        results.kind = ResourceKind::Results;
        assert!(!static_blockers(&results, &done).is_empty());
    }
}
