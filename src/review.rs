//! Review rules with evidence (F-21; PROJEKTPLAN §10.1).
//!
//! Hints are priorities, never verdicts. They come from explicit dates and
//! missing owners only; nothing observes use, so neither inactivity nor
//! file age, names or missing references ever become a hint.
use crate::clock::{DAY, Timestamp, display_date};
use crate::domain::{HintKind, Observation, OwnerKind, OwnerLink, Resource, ResourceKind, ReviewHint, Settings};

#[derive(Debug)]
pub struct ReviewInput<'a> {
    pub resources: &'a [Resource],
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

fn hint_for(input: &ReviewInput<'_>, resource: &Resource) -> Option<ReviewHint> {
    let owner = owner_text(input.owners, resource.id);
    // Observation starts when mogumogu learned about the resource; work in it
    // is not observed.
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
        blockers: Vec::new(),
        bytes: resource.bytes,
    };

    if resource.kind == ResourceKind::Scratchpad && resource.review_at.is_some_and(|at| at <= input.now) {
        base.kind = HintKind::ReviewDue;
        base.trigger = format!("Prüftermin am {} erreicht.", display_date(resource.review_at.unwrap_or_default()));
        base.evidence = format!("Scratchpad-Regel ({} Tage ab Aufnahme)", input.settings.scratch_review_days);
        base.next_action = "Verlängern, schützen oder zum Projekt machen.".into();
    } else if owner == "unbekannt" && resource.origin == crate::domain::Origin::Registered {
        base.kind = HintKind::PossiblyOrphaned;
        base.trigger = "Kein ausdrücklicher oder vermuteter Besitzer bekannt.".into();
        base.evidence = "Registriert ohne Projektzuordnung.".into();
        base.next_action = "Besitzer zuordnen oder schützen.".into();
    } else {
        return None;
    }
    if resource.protected {
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
            blockers: vec!["Demodaten sind schreibgeschützt.".into()],
            bytes: r.bytes,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Acquisition, Origin};

    fn scratch(review_at: Option<i64>, created: i64) -> Resource {
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
            created_at: Some(created),
        }
    }

    fn input<'a>(
        resources: &'a [Resource],
        owners: &'a [OwnerLink],
        settings: &'a Settings,
        now: i64,
    ) -> ReviewInput<'a> {
        ReviewInput { resources, owners, settings, now, demo: false }
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
        let resources = [scratch(Some(10 * DAY), 0)];
        let hints = evaluate(&input(&resources, &owners, &settings, 11 * DAY));
        assert_eq!(hints[0].kind, HintKind::ReviewDue);
        assert!(!hints[0].blockers.is_empty(), "protected");
        assert_eq!(hints[0].owner, "app (ausdrücklich)");
    }

    #[test]
    fn age_alone_never_becomes_a_hint() {
        let settings = Settings::default();
        let owners = [OwnerLink {
            resource_id: 1,
            project_id: 1,
            project_name: "app".into(),
            kind: OwnerKind::Explicit,
            evidence: String::new(),
        }];
        let resources = [scratch(None, 0)];
        assert!(evaluate(&input(&resources, &owners, &settings, 1_000 * DAY)).is_empty());
    }
}
