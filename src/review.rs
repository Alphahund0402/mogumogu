//! Review rules with evidence (F-21; PROJEKTPLAN §10.1).
//!
//! Hints are priorities, never verdicts. "Inactive" is only claimed when
//! the observation window actually covers the threshold; file age, names
//! or missing references are never used.
use crate::cleanup::static_blockers;
use crate::clock::{DAY, Timestamp, display_date};
use crate::domain::{
    Acquisition, HintKind, Observation, OwnerKind, OwnerLink, Resource, ResourceKind, ReviewHint, Session,
    SessionState, Settings,
};

#[derive(Debug)]
pub struct ReviewInput<'a> {
    pub resources: &'a [Resource],
    pub sessions: &'a [Session],
    pub owners: &'a [OwnerLink],
    pub settings: &'a Settings,
    pub now: Timestamp,
    pub demo: bool,
}

pub fn evaluate(input: &ReviewInput<'_>) -> Vec<ReviewHint> {
    if input.demo {
        return demo_hints(input.resources);
    }
    let mut hints = Vec::new();
    for resource in input.resources {
        if let Some(hint) = hint_for(input, resource) {
            hints.push(hint);
        }
    }
    hints
}

fn owner_text(owners: &[OwnerLink], id: i64) -> String {
    let mine: Vec<&OwnerLink> = owners.iter().filter(|o| o.resource_id == id).collect();
    if let Some(explicit) = mine.iter().find(|o| o.kind == OwnerKind::Explicit) {
        return format!("{} (ausdrücklich)", explicit.project_name);
    }
    match mine.first() {
        Some(inferred) => format!("{} (vermutet)", inferred.project_name),
        None => "unbekannt".into(),
    }
}

fn sessions_of(input: &ReviewInput<'_>, scratch_id: i64) -> Vec<Session> {
    input.sessions.iter().filter(|s| s.scratch_id == scratch_id).cloned().collect()
}

fn hint_for(input: &ReviewInput<'_>, resource: &Resource) -> Option<ReviewHint> {
    let owner = owner_text(input.owners, resource.id);
    let scratch_id = if resource.kind == ResourceKind::Scratchpad { Some(resource.id) } else { resource.parent_id };
    let sessions = scratch_id.map(|id| sessions_of(input, id)).unwrap_or_default();
    let last = resource.last_activity_at.or(resource.created_at);
    let threshold_days = match resource.kind {
        ResourceKind::Scratchpad | ResourceKind::Temporary => input.settings.scratch_review_days,
        _ => input.settings.environment_review_days,
    };
    // Observation starts when mogumogu learned about the resource; only
    // registered sessions are observed, manual work is not.
    let observed_days = resource.created_at.map(|since| (input.now - since) / DAY);
    let mut base = ReviewHint {
        resource_id: resource.id,
        resource_name: resource.name.clone(),
        kind: HintKind::Unclear,
        trigger: String::new(),
        evidence: String::new(),
        observed_period: match observed_days {
            Some(days) => format!("{days} Tage Beobachtung"),
            None => "keine Beobachtung".into(),
        },
        owner: owner.clone(),
        uncertainty: "Keine beobachtete Nutzung ist kein Entbehrlichkeitsnachweis.".into(),
        next_action: String::new(),
        protected: resource.protected,
        plannable: false,
        blockers: Vec::new(),
        bytes: resource.bytes,
    };

    if sessions.iter().any(|s| s.state == SessionState::InterruptedUnknown) {
        base.kind = HintKind::Unclear;
        base.trigger = "Eine Session wurde unklar unterbrochen.".into();
        base.evidence = "Starterende oder Absturz ohne Abschlussmeldung.".into();
        base.next_action =
            "Diagnose in results/ prüfen, dann Session nach Prüfung freigeben oder erneut ausführen.".into();
    } else if resource.kind == ResourceKind::Temporary && resource.acquisition == Acquisition::Managed {
        let done = sessions.iter().any(|s| s.state == SessionState::CompletedVerified);
        if !done && !resource.expendable {
            return None;
        }
        base.kind = HintKind::ReviewDue;
        base.trigger = if resource.expendable {
            "Lokal als entbehrlich markierte Temp-Ausgabe.".into()
        } else {
            "Verwalteter Lauf abgeschlossen; Temp-Ausgabe kann geprüft werden.".into()
        };
        base.evidence = format!(
            "{} abgeschlossene Session(s) belegen den Zweck.",
            sessions.iter().filter(|s| !s.state.locks_resources()).count()
        );
        base.uncertainty =
            "Programme können außerhalb von temporary/ geschrieben haben; dort bleibt alles erhalten.".into();
        base.next_action = if resource.expendable {
            "Plan-Vorschau erstellen; Ausführung nur nach Bestätigung des Fingerabdrucks.".into()
        } else {
            "Als entbehrlich markieren oder schützen.".into()
        };
        base.blockers = static_blockers(resource, &sessions);
        base.plannable = base.blockers.is_empty();
    } else if resource.kind == ResourceKind::Scratchpad && resource.review_at.is_some_and(|at| at <= input.now) {
        base.kind = HintKind::ReviewDue;
        base.trigger = format!("Prüftermin am {} erreicht.", display_date(resource.review_at.unwrap_or_default()));
        base.evidence = format!(
            "Scratchpad-Regel ({} Tage) · letzte erfasste Aktivität: {}",
            input.settings.scratch_review_days,
            last.map_or("keine".into(), display_date)
        );
        base.next_action = "Verlängern, schützen, zum Projekt machen oder Temp-Ausgabe prüfen.".into();
    } else if owner == "unbekannt" && resource.origin == crate::domain::Origin::Registered {
        base.kind = HintKind::PossiblyOrphaned;
        base.trigger = "Kein ausdrücklicher oder vermuteter Besitzer bekannt.".into();
        base.evidence = "Registriert ohne Projektzuordnung.".into();
        base.next_action = "Besitzer zuordnen oder schützen.".into();
    } else if let (Some(last), Some(observed)) = (last, observed_days) {
        let idle_days = (input.now - last) / DAY;
        if idle_days < threshold_days || observed < threshold_days {
            return None;
        }
        base.kind = HintKind::Inactive;
        base.trigger = format!("Keine registrierte Aktivität während {idle_days} Tagen Beobachtung.");
        base.evidence = format!("Letzte erfasste Aktivität am {}.", display_date(last));
        base.uncertainty =
            "Nur verwaltete Läufe und Sessions werden beobachtet; manuelle Arbeit bleibt unsichtbar.".into();
        base.next_action = "Prüfen, schützen oder im Rahmen eines Projektabschlusses berücksichtigen.".into();
    } else {
        return None;
    }
    if resource.protected && base.blockers.is_empty() {
        base.blockers.push("Geschützt – keine Bereinigung möglich.".into());
    }
    Some(base)
}

fn demo_hints(resources: &[Resource]) -> Vec<ReviewHint> {
    resources
        .iter()
        .filter(|r| r.state == Observation::Review)
        .map(|r| ReviewHint {
            resource_id: r.id,
            resource_name: r.name.clone(),
            kind: HintKind::ReviewDue,
            trigger: String::new(),
            evidence: r.evidence.clone(),
            observed_period: "Beispieldaten".into(),
            owner: "Beispiel".into(),
            uncertainty: "Keine Nutzung beobachtet ≠ sicher entbehrlich.".into(),
            next_action: "Nur Vorschau – im Demomodus wird nichts geplant.".into(),
            protected: r.protected,
            plannable: false,
            blockers: vec!["Demodaten sind schreibgeschützt.".into()],
            bytes: r.bytes,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Origin;

    fn scratch(review_at: Option<i64>, last: Option<i64>, created: i64) -> Resource {
        Resource {
            id: 1,
            project_id: Some(1),
            name: "spike".into(),
            path: r"C:\s".into(),
            kind: ResourceKind::Scratchpad,
            bytes: None,
            state: Observation::Unknown,
            evidence: String::new(),
            protected: true,
            origin: Origin::Registered,
            acquisition: Acquisition::Managed,
            parent_id: None,
            purpose: None,
            review_at,
            expendable: false,
            last_activity_at: last,
            created_at: Some(created),
        }
    }

    fn input<'a>(
        resources: &'a [Resource],
        owners: &'a [OwnerLink],
        settings: &'a Settings,
        now: i64,
    ) -> ReviewInput<'a> {
        ReviewInput { resources, sessions: &[], owners, settings, now, demo: false }
    }

    #[test]
    fn review_date_is_a_priority_and_protection_blocks() {
        let settings = Settings::default();
        let owners = [OwnerLink {
            resource_id: 1,
            project_id: 1,
            project_name: "app".into(),
            kind: OwnerKind::Explicit,
            evidence: String::new(),
        }];
        let resources = [scratch(Some(10 * DAY), None, 0)];
        let hints = evaluate(&input(&resources, &owners, &settings, 11 * DAY));
        assert_eq!(hints[0].kind, HintKind::ReviewDue);
        assert!(!hints[0].plannable);
        assert_eq!(hints[0].owner, "app (ausdrücklich)");
    }

    #[test]
    fn inactivity_needs_an_observation_window() {
        let settings = Settings::default();
        let owners = [OwnerLink {
            resource_id: 1,
            project_id: 1,
            project_name: "app".into(),
            kind: OwnerKind::Explicit,
            evidence: String::new(),
        }];
        // Registered two days ago: mogumogu cannot claim inactivity yet.
        let recent = [scratch(None, Some(0), 98 * DAY)];
        assert!(evaluate(&input(&recent, &owners, &settings, 100 * DAY)).is_empty());
        let resources = [scratch(None, Some(0), 0)];
        let hints = evaluate(&input(&resources, &owners, &settings, 100 * DAY));
        assert_eq!(hints[0].kind, HintKind::Inactive);
    }
}
