//! The project page: one project with its storage, the entries of its last
//! tidy-up report, what holds its folders, how far AI tools reach and how it
//! grew. Pure presentation from the snapshot. A selection only narrows a
//! recycle-bin plan; the owner measures again and checks every folder.
use super::disk::DORMANT_DAYS;
use super::ui::*;
use super::view::{coverage_cell, model};
use mogumogu::clock::{Clock, DAY, SystemClock, display_date, display_local, display_relative, display_time};
use mogumogu::domain::*;
use slint::{ComponentHandle, SharedString};
use std::collections::BTreeSet;

/// A complete report younger than this counts as current.
const CURRENT: i64 = 7 * DAY;

fn bytes(value: u64) -> String {
    format_bytes(i64::try_from(value).ok())
}

fn signed(value: i64) -> String {
    format!("{}{}", if value < 0 { "−" } else { "+" }, format_bytes(Some(value.saturating_abs())))
}

fn shown(rel: &str) -> String {
    rel.replace('/', "\\")
}

fn chip(label: impl Into<SharedString>, kind: &str) -> PageChip {
    PageChip { label: label.into(), kind: kind.into() }
}

fn action(id: impl Into<SharedString>, label: &str, primary: bool) -> DetailAction {
    DetailAction { id: id.into(), label: label.into(), primary }
}

fn fact(label: &str, value: impl Into<SharedString>) -> DetailRow {
    DetailRow { label: label.into(), value: value.into() }
}

/// `heute 17:39` within a day, the date and time otherwise.
fn when(at: i64, now: i64) -> String {
    if now - at < DAY { format!("heute {}", display_time(at)) } else { display_local(at) }
}

/// Whether the project page lists a plan: open ones and those that ran.
fn shows_plan(state: PlanState) -> bool {
    !matches!(state, PlanState::Cancelled | PlanState::Invalidated)
}

/// Recoverable entries of a report that the next plan would take.
pub fn selected<'a>(report: &'a ProjectTidy, skipped: &'a BTreeSet<String>) -> impl Iterator<Item = &'a TidyItem> {
    report.items.iter().filter(move |i| i.class == TidyClass::Regenerable && !skipped.contains(&i.rel_path))
}

/// Fills the project page for `project_id`; a project that is gone leaves an
/// empty name, and the page says so.
pub fn render(ui: &Dashboard, snapshot: &Snapshot, project_id: i64, skipped: &BTreeSet<String>) {
    let store = ui.global::<Store>();
    store.set_project_id(i32::try_from(project_id).unwrap_or(-1));
    let Some(project) = snapshot.projects.iter().find(|p| p.id == project_id) else {
        store.set_project_name(SharedString::new());
        store.set_project_path(SharedString::new());
        store.set_project_chips(model(Vec::new()));
        store.set_project_actions(model(Vec::new()));
        return;
    };
    let now = SystemClock.now();
    let demo = snapshot.demo;
    let scope = snapshot.scopes.iter().find(|s| s.project_id == Some(project.id) && s.kind == ScopeKind::Project);
    let approved = scope.filter(|s| s.status == ScopeStatus::Approved);
    let report = snapshot.tidy.projects.iter().find(|p| p.project_id == project.id);
    let access = approved.and_then(|s| snapshot.ai_access.iter().position(|a| a.scope_id == s.id));

    store.set_project_name(project.name.clone().into());
    store.set_project_path(project.path.clone().into());

    let mut chips = vec![match (demo, scope, approved) {
        (true, ..) => chip("Beispieldaten", "neutral"),
        (_, _, Some(_)) => chip("Lesefreigabe", "ok"),
        (_, Some(scope), None) => chip(scope.status.label(), "review"),
        (_, None, None) => chip("Keine Lesefreigabe", "neutral"),
    }];
    if approved.is_some() {
        let (coverage, kind) = coverage_cell(snapshot, project);
        chips.push(chip(format!("Erfassung: {coverage}"), kind));
    }
    match report {
        Some(r) if !r.complete => chips.push(chip("Bericht teilweise", "review")),
        Some(r) if now - r.checked_at < CURRENT => {
            chips.push(chip(format!("Bericht aktuell · {}", when(r.checked_at, now)), "ok"));
        }
        Some(r) => chips.push(chip(format!("Bericht vom {}", display_date(r.checked_at)), "neutral")),
        None => {}
    }
    if let Some(r) = report {
        if r.heavy_test_data() {
            chips.push(chip("Viele Testdaten", "review"));
        }
        if r.last_change.is_some_and(|t| now - t > DORMANT_DAYS * DAY) && r.regenerable > 0 {
            chips.push(chip("Ruht", "review"));
        }
    }
    if let Some(a) = access.map(|i| &snapshot.ai_access[i])
        && a.level != AccessLevel::Limited
    {
        chips.push(chip(
            format!("KI: {}", a.level.label()),
            if a.level == AccessLevel::Wide { "alert" } else { "review" },
        ));
    }
    store.set_project_chips(model(chips));

    // Header: what the page does to the whole project. Reading starts with
    // the approval, offered below where its effect is explained.
    let mut actions = Vec::new();
    if !demo && let Some(scope) = approved {
        actions.push(action(format!("scan-scope:{}", scope.id), "Jetzt erfassen", false));
        if report.is_some() {
            actions.push(action(format!("tidy:{}", project.id), "Neu messen", false));
        }
        actions.push(action(format!("revoke-scope:{}", scope.id), "Freigabe widerrufen", false));
    }
    store.set_project_actions(model(actions));

    let (note, report_action) = match (report, demo, approved) {
        (Some(_), ..) => (String::new(), action("", "", false)),
        (None, true, _) => ("Beispieldaten: Der Demomodus ist schreibgeschützt und misst nichts.".into(), action("", "", false)),
        (None, false, None) => (
            "Ohne Lesefreigabe misst mogumogu in diesem Projekt nichts und schlägt nichts vor. Die Freigabe ist an die \
             Identität dieses Ordners gebunden und lässt sich hier wieder widerrufen."
                .into(),
            action(format!("approve-project:{}", project.id), "Lesefreigabe erteilen", true),
        ),
        (None, false, Some(_)) => (
            "Für dieses Projekt gibt es noch keinen Aufräumbericht. Er misst den Ordner einmal und teilt ihn in \
             Wiederherstellbares und zu Prüfendes; entfernt wird dabei nichts."
                .into(),
            action(format!("tidy:{}", project.id), "Bericht erstellen", true),
        ),
    };
    store.set_project_report_note(note.into());
    store.set_project_report_action(report_action);

    let mut facts = Vec::new();
    if !project.ecosystem.is_empty() {
        facts.push(fact("Technologie", project.ecosystem.clone()));
    }
    facts.push(fact("Beobachtung", format!("{} – {}", project.state.label(), project.evidence)));
    facts.push(fact(
        "Pakete",
        project.packages.map_or_else(|| "Unbekannt (keine vollständige Erfassung)".to_string(), |n| n.to_string()),
    ));
    facts.push(fact(
        "Lesefreigabe",
        match (approved, scope) {
            (Some(_), _) => "Erteilt, an die Ordneridentität gebunden",
            (None, Some(scope)) => scope.status.label(),
            (None, None) => "Nicht erteilt",
        },
    ));
    if !project.coverage.is_empty() {
        facts.push(fact("Abdeckung", project.coverage.clone()));
    }

    match report {
        Some(r) => {
            let parts = r.regenerable + r.review + r.vcs;
            let known = r.total.unwrap_or(parts).max(parts);
            let rest = known - parts;
            let share = |part: u64| if known == 0 { 0.0 } else { (part as f64 / known as f64) as f32 };
            store.set_project_total(
                r.total.map_or_else(|| format!("mindestens {} · unvollständig gemessen", bytes(parts)), bytes).into(),
            );
            store.set_project_shares(model(vec![share(r.regenerable), share(r.review), share(r.vcs), share(rest)]));
            store.set_project_legend(model(vec![
                format!("Wiederherstellbar {}", bytes(r.regenerable)).into(),
                if r.test_data > 0 {
                    format!("Prüfen {} · davon Testdaten {}", bytes(r.review), bytes(r.test_data))
                } else {
                    format!("Prüfen {}", bytes(r.review))
                }
                .into(),
                format!("Versionsverwaltung {}", bytes(r.vcs)).into(),
                if r.total.is_some() {
                    format!("Quellen und Rest {} · bleibt", bytes(rest))
                } else {
                    "Rest unbekannt".into()
                }
                .into(),
            ]));
            store.set_project_checked(format!("Gemessen {}", when(r.checked_at, now)).into());
            if let Some(t) = r.last_change {
                facts.push(fact("Zuletzt geändert", display_relative(t, now)));
            }
            if r.agent > 0 {
                facts.push(fact("Reste von KI-Agenten", format!("{} · nie wiederherstellbar", bytes(r.agent))));
            }
            if !r.note.is_empty() {
                facts.push(fact("Hinweis", r.note.clone()));
            }
        }
        None => {
            facts.push(fact("Größe", format_bytes(project.bytes)));
            store.set_project_total(SharedString::new());
            store.set_project_shares(model(vec![0.0; 4]));
            store.set_project_legend(model(vec![SharedString::new(); 4]));
            store.set_project_checked(SharedString::new());
        }
    }
    store.set_project_facts(model(facts));

    render_cleanup(&store, project, report, approved.is_some() && !demo, skipped);
    render_holds(&store, project, report);

    // Open plans and results; a discarded or invalidated plan moved nothing.
    store.set_project_plans(model(
        snapshot
            .plans
            .iter()
            .filter(|p| p.project_id == Some(project.id) && shows_plan(p.state))
            .map(|plan| PlanRow {
                id: plan.id as i32,
                state: plan.state.label().into(),
                created: display_relative(plan.created_at, now).into(),
                summary: format!("{} Ordner · {}", plan.targets.len(), format_bytes(Some(plan.bytes()))).into(),
                fingerprint: plan.short_fingerprint().into(),
            })
            .collect(),
    ));

    match access.map(|i| (i, &snapshot.ai_access[i])) {
        Some((index, a)) => {
            store.set_project_ai_level(a.level.label().into());
            store.set_project_ai_kind(
                match a.level {
                    AccessLevel::Wide => "alert",
                    AccessLevel::Open => "review",
                    AccessLevel::Limited => "ok",
                }
                .into(),
            );
            let tools = a.tools.join(", ");
            store.set_project_ai_summary(
                match a.findings.first() {
                    Some(finding) => format!("{tools} · {finding}"),
                    None => format!("{tools} · keine Befunde"),
                }
                .into(),
            );
            store.set_project_ai_action(if a.level != AccessLevel::Limited && !demo {
                action(format!("filter-preview:{}", a.scope_id), "Kontextfilter vorbereiten …", true)
            } else {
                action("", "", false)
            });
            store.set_project_ai_index(i32::try_from(index).unwrap_or(-1));
        }
        None => {
            store.set_project_ai_level(SharedString::new());
            store.set_project_ai_kind(SharedString::new());
            store.set_project_ai_summary(
                if approved.is_some() {
                    "In diesem Projekt sind keine Einstellungen oder Instruktionen von KI-Werkzeugen erfasst."
                } else {
                    "Ohne Lesefreigabe sind die KI-Dateien dieses Projekts nicht erfasst."
                }
                .into(),
            );
            store.set_project_ai_action(action("", "", false));
            store.set_project_ai_index(-1);
        }
    }

    // Every change names the two reports it compares.
    let (growth, items) = match report.map(|r| &r.growth) {
        Some(Some(g)) => (
            format!(
                "Vom {} bis {} · {} vollständige Berichte\n{} wiederherstellbar · {} zu prüfen{}",
                display_date(g.since),
                display_date(g.until),
                g.reports,
                signed(g.regenerable),
                signed(g.review),
                g.total.map(|t| format!(" · gesamt {}", signed(t))).unwrap_or_default()
            ),
            g.items.iter().map(|(rel, grown)| fact(&shown(rel), signed(*grown))).collect(),
        ),
        Some(None) => ("Ab dem zweiten vollständigen Bericht zeigt sich hier, was wächst.".to_string(), Vec::new()),
        None => (String::new(), Vec::new()),
    };
    store.set_project_growth(growth.into());
    store.set_project_growth_items(model(items));
}

/// Recoverable entries first, each with its evidence; then what needs a look.
fn render_cleanup(
    store: &Store<'_>,
    project: &Project,
    report: Option<&ProjectTidy>,
    plannable: bool,
    skipped: &BTreeSet<String>,
) {
    let Some(report) = report else {
        store.set_project_cleanup(model(Vec::new()));
        store.set_project_selection(SharedString::new());
        store.set_project_can_plan(false);
        return;
    };
    let (recoverable, review): (Vec<&TidyItem>, Vec<&TidyItem>) =
        report.items.iter().partition(|i| i.class == TidyClass::Regenerable);
    let rows = recoverable
        .iter()
        .map(|item| CleanupRow {
            rel_path: item.rel_path.clone().into(),
            title: shown(&item.rel_path).into(),
            detail: format!("{} · {}", item.label, item.evidence).into(),
            size: bytes(item.bytes).into(),
            recoverable: true,
            selected: !skipped.contains(&item.rel_path),
            action_id: format!("keep-folder:{}:1:{}", project.id, item.rel_path).into(),
            action_label: "Immer behalten".into(),
        })
        .chain(review.iter().map(|item| {
            let (action_id, action_label) = if item.label == "Datenbank" {
                (
                    format!("db-register-path:{}\\{}", project.path.trim_end_matches('\\'), shown(&item.rel_path)),
                    "Überwachen",
                )
            } else {
                (String::new(), "")
            };
            CleanupRow {
                rel_path: item.rel_path.clone().into(),
                title: shown(&item.rel_path).into(),
                detail: format!("Prüfen · {} · {}", item.label, item.evidence).into(),
                size: bytes(item.bytes).into(),
                recoverable: false,
                selected: false,
                action_id: action_id.into(),
                action_label: action_label.into(),
            }
        }))
        .collect();
    store.set_project_cleanup(model(rows));
    let chosen: Vec<&TidyItem> = selected(report, skipped).collect();
    let logical: u64 = chosen.iter().map(|i| i.bytes).sum();
    let allocated: u64 = chosen.iter().map(|i| i.allocated).sum();
    store.set_project_selection(
        if recoverable.is_empty() {
            "Nichts in diesem Projekt ist als wiederherstellbar belegt.".to_string()
        } else {
            format!(
                "{} von {} ausgewählt · {}{}",
                chosen.len(),
                recoverable.len(),
                bytes(logical),
                if allocated > 0 { format!(" · belegt {}", bytes(allocated)) } else { String::new() }
            )
        }
        .into(),
    );
    store.set_project_can_plan(plannable && !chosen.is_empty());
}

/// Holds named in the report. Monitored databases and Docker mounts are
/// checked by every plan; their lists open from the section.
fn render_holds(store: &Store<'_>, project: &Project, report: Option<&ProjectTidy>) {
    let Some(report) = report else {
        store.set_project_holds(model(Vec::new()));
        store.set_project_holds_note(
            "Was Ordner dieses Projekts festhält, nennt der Aufräumbericht. Überwachte Datenbanken und Ordner, die ein \
             Container einbindet, prüft außerdem jeder Plan."
                .into(),
        );
        return;
    };
    let holds: Vec<HoldRow> = report
        .kept
        .iter()
        .map(|rel| HoldRow {
            title: shown(rel).into(),
            detail: "Von dir behalten · erscheint in keinem Papierkorb-Plan, ebenso nichts darin oder darüber".into(),
            action_id: format!("keep-folder:{}:0:{rel}", project.id).into(),
            action_label: "Wieder vorschlagen".into(),
        })
        .chain(report.runs.iter().map(|run| {
            HoldRow {
                title: run.title.clone().into(),
                detail: format!(
                    "Registrierter KI-Lauf · {} Artefakt{} geschützt · gestartet {}",
                    run.artifacts,
                    if run.artifacts == 1 { "" } else { "e" },
                    display_local(run.started_at)
                )
                .into(),
                action_id: "agents".into(),
                action_label: "Ansehen …".into(),
            }
        }))
        .collect();
    store.set_project_holds_note(
        if holds.is_empty() {
            "Weder behaltene Ordner noch registrierte KI-Läufe. Überwachte Datenbanken und Ordner, die ein Container \
             einbindet, prüft jeder Plan zusätzlich."
        } else {
            "Überwachte Datenbanken und Ordner, die ein Container einbindet, prüft jeder Plan zusätzlich. Aufheben geht \
             nur hier im Dashboard."
        }
        .into(),
    );
    store.set_project_holds(model(holds));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(rel: &str, class: TidyClass) -> TidyItem {
        TidyItem {
            rel_path: rel.into(),
            class,
            label: String::new(),
            bytes: 1,
            allocated: 1,
            evidence: String::new(),
            modified_at: None,
        }
    }

    #[test]
    fn a_plan_takes_recoverable_entries_only_and_leaves_skipped_ones_out() {
        let report = ProjectTidy {
            project_id: 1,
            name: "web".into(),
            path: r"C:\dev\web".into(),
            total: Some(3),
            regenerable: 2,
            regenerable_allocated: 2,
            review: 1,
            vcs: 0,
            test_data: 1,
            agent: 0,
            kept: Vec::new(),
            runs: Vec::new(),
            growth: None,
            items: vec![
                item("node_modules", TidyClass::Regenerable),
                item("tests/fixtures", TidyClass::Review),
                item(".next", TidyClass::Regenerable),
            ],
            last_change: None,
            complete: true,
            note: String::new(),
            checked_at: 0,
        };
        let paths =
            |skipped: &BTreeSet<String>| selected(&report, skipped).map(|i| i.rel_path.clone()).collect::<Vec<_>>();
        assert_eq!(paths(&BTreeSet::new()), ["node_modules", ".next"], "review entries are never offered");
        assert_eq!(paths(&BTreeSet::from(["node_modules".to_string()])), [".next"]);
        assert!(paths(&BTreeSet::from(["node_modules".to_string(), ".next".to_string()])).is_empty());
    }

    #[test]
    fn the_page_lists_open_plans_and_results_but_not_discarded_ones() {
        for state in [
            PlanState::Draft,
            PlanState::Approved,
            PlanState::RecoveryRequired,
            PlanState::Completed,
            PlanState::Failed,
        ] {
            assert!(shows_plan(state), "{state:?}");
        }
        assert!(!shows_plan(PlanState::Cancelled) && !shows_plan(PlanState::Invalidated));
    }
}
