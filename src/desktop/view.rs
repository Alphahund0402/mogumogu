//! Maps the core snapshot to Slint models. Pure presentation: no SQL, no
//! filesystem access, no decisions about safety.
use super::donut;
use super::ui::*;
use mogumogu::adapters;
use mogumogu::clock::{Clock, SystemClock, display_relative};
use mogumogu::domain::*;
use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;

const SLICE_COLORS: [u32; 5] = [0x2d7cf5, 0x2bb3d7, 0x25bc9d, 0x78899e, 0xb7c2d0];

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(items)).into()
}

fn s(text: impl Into<SharedString>) -> SharedString {
    text.into()
}

fn rgb(value: u32) -> Color {
    Color::from_rgb_u8((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// UI-side state that is not part of the snapshot.
pub struct ViewState<'a> {
    pub snapshot: &'a Snapshot,
    pub busy: bool,
    pub status: &'a str,
    pub data_path: &'a str,
}

pub fn observation_kind(state: Observation) -> &'static str {
    match state {
        Observation::Observed => "observed",
        Observation::Review => "review",
        Observation::Protected => "protected",
        Observation::Unknown => "",
    }
}

pub fn owner_text(snapshot: &Snapshot, resource_id: i64) -> String {
    let owners: Vec<&OwnerLink> = snapshot.owners_of(resource_id).collect();
    match owners.iter().find(|o| o.kind == OwnerKind::Explicit).or(owners.first()) {
        Some(o) if o.kind == OwnerKind::Explicit => o.project_name.clone(),
        Some(o) => format!("{} (vermutet)", o.project_name),
        None => "Besitzer unbekannt".into(),
    }
}

fn matches(project: &Project, query: &str, filter: i32) -> bool {
    let text = format!("{} {} {}", project.name, project.path, project.ecosystem).to_lowercase();
    let state_ok = match filter {
        1 => project.state == Observation::Observed,
        2 => project.state == Observation::Review,
        _ => true,
    };
    state_ok && text.contains(&query.to_lowercase())
}

pub fn render(ui: &Dashboard, view: &ViewState<'_>) {
    let store = ui.global::<Store>();
    let snapshot = view.snapshot;
    let now = SystemClock.now();
    store.set_demo(snapshot.demo);
    store.set_busy(view.busy);
    store.set_status(s(view.status));
    store.set_sqlite_version(s(&snapshot.sqlite_version));
    store.set_data_path(s(view.data_path));
    store.set_version(s(env!("CARGO_PKG_VERSION")));

    // Metrics
    let observed = snapshot.projects.iter().filter(|p| p.state == Observation::Observed).count();
    let review = snapshot.projects.iter().filter(|p| p.state == Observation::Review).count();
    let approved = snapshot.projects.iter().filter(|p| p.read_approved).count();
    store.set_project_count(s(snapshot.projects.len().to_string()));
    store.set_project_note(s(if snapshot.demo {
        format!("{observed} mit Beispielaktivität")
    } else {
        format!("{approved} mit Lesefreigabe")
    }));
    store.set_package_count(s(snapshot.package_count().map_or("—".into(), |n| n.to_string())));
    store.set_package_note(s(match (snapshot.demo, snapshot.package_count()) {
        (true, _) => "Beispielbestand · keine Prüfung",
        (false, Some(_)) => "Aus vollständigen Erfassungen",
        (false, None) => "Ohne vollständige Erfassung unbekannt",
    }));
    store.set_footprint(s(snapshot.total_bytes().map_or("—".into(), |b| format_bytes(Some(b)))));
    store.set_footprint_note(s("Keine zugesagte Ersparnis"));
    store.set_ai_count(s(snapshot.ai.len().to_string()));
    store.set_ai_note(s("Quellen bleiben geschützt"));
    store.set_review_count(snapshot.review_count() as i32);
    store.set_count_all(s(snapshot.projects.len().to_string()));
    store.set_count_observed(s(observed.to_string()));
    store.set_count_review(s(review.to_string()));

    // Projects (filtered in Rust; the UI holds one bounded page)
    let query = store.get_query().to_string();
    let filter = store.get_filter();
    store.set_projects(model(
        snapshot
            .projects
            .iter()
            .filter(|p| matches(p, &query, filter))
            .map(|p| ProjectRow {
                id: p.id as i32,
                name: s(&p.name),
                path: s(&p.path),
                ecosystem: s(&p.ecosystem),
                state: s(p.state.label()),
                state_kind: s(observation_kind(p.state)),
                size: s(format_bytes(p.bytes)),
                detail: s(&p.evidence),
                approved: p.read_approved,
                coverage: s(&p.coverage),
            })
            .collect(),
    ));

    let tab = store.get_resource_tab();
    store.set_resources(model(
        snapshot
            .resources
            .iter()
            .filter(|r| tab != 1 || r.kind == ResourceKind::Scratchpad || r.parent_id.is_some())
            .map(|r| {
                let (state, kind) = if r.expendable {
                    ("Entbehrlich markiert".to_string(), "warn")
                } else if r.protected {
                    ("Geschützt".to_string(), "protected")
                } else {
                    (r.state.label().to_string(), observation_kind(r.state))
                };
                ResourceRow {
                    id: r.id as i32,
                    name: s(&r.name),
                    kind: s(r.kind.label()),
                    owner: s(owner_text(snapshot, r.id)),
                    size: s(format_bytes(r.bytes)),
                    state: s(state),
                    state_kind: s(kind),
                    detail: s(&r.evidence),
                }
            })
            .collect(),
    ));

    store.set_ai(model(
        snapshot
            .ai
            .iter()
            .enumerate()
            .map(|(index, a)| AiRow {
                index: index as i32,
                client: s(&a.client),
                kind: s(&a.kind),
                detail: s(&a.label),
                path: s(&a.path),
                letter: s(a.client.chars().next().unwrap_or('?').to_string()),
                references: a.references.len() as i32,
            })
            .collect(),
    ));

    store.set_events(model(
        snapshot
            .activity
            .iter()
            .map(|e| EventRow { title: s(&e.title), detail: s(&e.detail), time: s(&e.created_at), kind: s(&e.kind) })
            .collect(),
    ));

    store.set_adapters(model(
        adapters::descriptors()
            .into_iter()
            .map(|d| {
                let counts = snapshot.ecosystems.iter().find(|e| e.ecosystem == d.id);
                let counts = match (snapshot.demo, counts) {
                    (true, _) => "Beispieldaten · keine Erfassung".to_string(),
                    (false, Some(c)) => format!(
                        "{} deklariert · {} aufgelöst · {} installiert{}",
                        c.declared,
                        c.resolved,
                        c.installed,
                        if c.updates_available > 0 {
                            format!(" · {} Updatehinweise", c.updates_available)
                        } else {
                            String::new()
                        }
                    ),
                    (false, None) => "Noch nicht erfasst".to_string(),
                };
                AdapterRow {
                    id: s(d.id),
                    name: s(d.name),
                    inventory: s(d.inventory.label()),
                    updates: s(d.updates.label()),
                    cleanup: s(d.cleanup.label()),
                    counts: s(counts),
                    formats: s(d.formats.join(" · ")),
                    limits: s(d.limits),
                }
            })
            .collect(),
    ));

    store.set_hints(model(
        snapshot
            .hints
            .iter()
            .map(|h| HintRow {
                id: h.resource_id as i32,
                name: s(&h.resource_name),
                kind: s(h.kind.label()),
                trigger: s(&h.trigger),
                evidence: s(&h.evidence),
                size: s(h.bytes.map(|b| format_bytes(Some(b))).unwrap_or_default()),
                protected: h.protected,
                plannable: h.plannable,
            })
            .collect(),
    ));

    store.set_scopes(model(
        snapshot
            .scopes
            .iter()
            .map(|scope| ScopeRow {
                id: scope.id as i32,
                kind: s(scope.kind.label()),
                path: s(&scope.path),
                status: s(scope.status.label()),
                coverage: s(scope.coverage_label()),
                last: s(scope
                    .latest
                    .as_ref()
                    .and_then(|g| g.finished_at)
                    .map(|t| format!("zuletzt {}", display_relative(t, now)))
                    .unwrap_or_default()),
                watched: scope.watched,
                healthy: scope.status == ScopeStatus::Approved,
            })
            .collect(),
    ));

    store.set_sessions(model(
        snapshot
            .sessions
            .iter()
            .map(|session| SessionRow {
                id: session.id as i32,
                scratch: s(snapshot
                    .resources
                    .iter()
                    .find(|r| r.id == session.scratch_id)
                    .map_or("?", |r| r.name.as_str())),
                purpose: s(&session.purpose),
                state: s(session.state.label()),
                state_kind: s(match session.state {
                    SessionState::Starting | SessionState::Running | SessionState::CompletionRequested => "running",
                    SessionState::CompletedVerified | SessionState::ReleasedAfterReview => "ok",
                    SessionState::InterruptedUnknown => "warn",
                }),
                started: s(display_relative(session.started_at, now)),
            })
            .collect(),
    ));

    store.set_plans(model(
        snapshot
            .plans
            .iter()
            .map(|plan| PlanRow {
                id: plan.id as i32,
                state: s(plan.state.label()),
                created: s(display_relative(plan.created_at, now)),
                summary: s(format!(
                    "{} Ziel(e) · {} Einträge",
                    plan.targets.len(),
                    plan.targets.iter().map(|t| t.entries).sum::<i64>()
                )),
                fingerprint: s(plan.short_fingerprint()),
            })
            .collect(),
    ));

    store.set_profiles(model(
        mogumogu::profiles::catalog()
            .profiles
            .iter()
            .map(|p| ProfileChip { name: s(&p.name), status: s(p.status.label()) })
            .collect(),
    ));

    render_storage(&store, snapshot);
    render_settings(&store, snapshot);
}

fn render_storage(store: &Store<'_>, snapshot: &Snapshot) {
    let known: Option<Vec<u64>> = snapshot.storage.iter().map(|slice| slice.bytes.map(|b| b.max(0) as u64)).collect();
    let paths = known.as_deref().map(donut::segments).unwrap_or_default();
    store.set_donut(model(
        paths
            .into_iter()
            .zip(SLICE_COLORS)
            .filter(|(commands, _)| !commands.is_empty())
            .map(|(commands, color)| DonutSegment { commands: s(commands), color: rgb(color) })
            .collect(),
    ));
    store.set_legend(model(
        snapshot
            .storage
            .iter()
            .zip(SLICE_COLORS)
            .map(|(slice, color)| LegendRow {
                label: s(&slice.label),
                value: s(slice.bytes.map_or("—".into(), |b| format_bytes(Some(b)).replace(",0 GB", " GB"))),
                color: rgb(color),
            })
            .collect(),
    ));
    let total = known.map(|v| v.iter().sum::<u64>()).filter(|t| *t > 0);
    store.set_donut_value(s(total.map_or("—".into(), |t| format_bytes(Some(t as i64)).replace(",0 GB", " GB"))));
    store.set_donut_caption(s(match (total, snapshot.demo) {
        (Some(_), true) => "Beispielumfang",
        (Some(_), false) => "Gemessen",
        (None, _) => "Unbekannt",
    }));
    store.set_storage_subtitle(s(if snapshot.demo {
        "Illustrativer, überschneidungsfreier Beispielbestand"
    } else {
        "Nur gemessene, von mogumogu verwaltete Ordner – Unbekanntes bleibt unbekannt"
    }));
}

fn render_settings(store: &Store<'_>, snapshot: &Snapshot) {
    let settings = snapshot.settings.clone().unwrap_or_default();
    store.set_monitoring(settings.monitoring_enabled);
    store.set_notifications(settings.notifications_enabled);
    store.set_updates(settings.network_updates_enabled);
    store.set_sources(model(
        PUBLIC_UPDATE_SOURCES
            .iter()
            .map(|host| SourceToggle { host: s(*host), enabled: settings.update_sources.iter().any(|h| h == host) })
            .collect(),
    ));
    store.set_scratch_root(s(settings.scratch_root.unwrap_or_default()));
    store.set_cleanup_status(s(format!(
        "Produktive Bereinigung ist bis zum unabhängigen Review (G5) gesperrt. Der Testexecutor entfernt nur \
         verwaltete Temp-Ausgabe in registrierten Wegwerf-Testwurzeln, nach bestätigtem Plan. {} Pläne gespeichert.",
        snapshot.plans.len()
    )));
}
