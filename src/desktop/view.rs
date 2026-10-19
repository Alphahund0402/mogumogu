//! Maps the core snapshot to Slint models. Pure presentation: no SQL, no
//! filesystem access, no decisions about safety.
use super::ui::*;
use mogumogu::clock::{Clock, DAY, SystemClock, display_relative, display_time};
use mogumogu::domain::*;
use mogumogu::limits::PACKAGE_PAGE;
use mogumogu::service::ScanProgress;
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;

/// Desktop state the snapshot alone does not carry.
#[derive(Debug)]
pub struct ViewState<'a> {
    pub package_key: Option<&'a str>,
    pub experiment: Option<i64>,
    pub scanning: bool,
}

pub(super) fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(items)).into()
}

fn s(text: impl Into<SharedString>) -> SharedString {
    text.into()
}

pub fn observation_kind(state: Observation) -> &'static str {
    match state {
        Observation::Observed => "ok",
        Observation::Review => "review",
        Observation::Protected => "protected",
        Observation::Unknown => "neutral",
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

/// Short coverage of one project for its table row.
fn coverage_cell(snapshot: &Snapshot, project: &Project) -> (String, &'static str) {
    if snapshot.demo {
        return ("Beispieldaten".into(), "neutral");
    }
    match snapshot.scopes.iter().find(|s| s.project_id == Some(project.id)) {
        None => ("Keine Lesefreigabe".into(), "neutral"),
        Some(scope) if scope.status != ScopeStatus::Approved => (scope.status.label().into(), "review"),
        Some(scope) if scope.current() => ("Vollständig".into(), "ok"),
        Some(scope) => match &scope.latest {
            None => ("Noch nicht erfasst".into(), "neutral"),
            Some(latest) if latest.limit_reason.is_some() => ("Teilweise · Limit".into(), "review"),
            Some(latest) => (format!("Teilweise · {}", latest.state.label()), "review"),
        },
    }
}

/// Scratchpads whose sessions got stuck, with the most pressing stall.
fn stalled(snapshot: &Snapshot, now: i64) -> Vec<(i64, &Session, Stall)> {
    snapshot
        .sessions
        .iter()
        .filter_map(|session| session.stall(now).map(|stall| (session.scratch_id, session, stall)))
        .collect()
}

pub fn default_experiment(snapshot: &Snapshot) -> Option<i64> {
    let now = SystemClock.now();
    stalled(snapshot, now)
        .first()
        .map(|(id, ..)| *id)
        .or_else(|| snapshot.resources.iter().find(|r| r.kind == ResourceKind::Scratchpad).map(|r| r.id))
}

pub fn candidate_owner(candidate: &ProjectCandidate) -> String {
    if !candidate.project.is_empty() {
        if candidate.resource_id.is_some() {
            candidate.project.clone()
        } else {
            format!("{} (vermutet)", candidate.project)
        }
    } else if let Some(rel) = &candidate.project_rel_path {
        format!("Projektordner {} (nicht registriert)", if rel.is_empty() { "." } else { rel })
    } else {
        "Besitzer unbekannt".into()
    }
}

/// Name-based discoveries and scanned environments/build outputs share one
/// list; `artifact` tells the row which detail it opens.
pub fn render_candidates(ui: &Dashboard, snapshot: &Snapshot) {
    let store = ui.global::<Store>();
    let filter = store.get_candidate_filter();
    let query = store.get_candidate_search().to_string().to_lowercase();
    let found = |text: String| query.is_empty() || text.to_lowercase().contains(&query);
    let mut rows: Vec<CandidateRow> = snapshot
        .discoveries
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            let kind_matches = match filter {
                1 => !c.registered,
                2 => matches!(c.kind, DiscoveryKind::Scratchpad | DiscoveryKind::Notebook | DiscoveryKind::Script),
                3 => matches!(
                    c.kind,
                    DiscoveryKind::Environment
                        | DiscoveryKind::Dependencies
                        | DiscoveryKind::BuildOutput
                        | DiscoveryKind::TestOutput
                        | DiscoveryKind::Cache
                ),
                4 => matches!(c.kind, DiscoveryKind::TestData | DiscoveryKind::Database),
                _ => true,
            };
            kind_matches && found(format!("{} {} {} {}", c.name, c.rel_path, c.evidence, c.project))
        })
        .map(|(index, c)| CandidateRow {
            index: index as i32,
            artifact: false,
            path: s(format!("{} · {}", c.kind.label(), candidate_name(snapshot, c))),
            evidence: s(format!(
                "{} · {}",
                if c.rel_path.is_empty() { "Ordnerwurzel" } else { &c.rel_path },
                candidate_owner(c)
            )),
            status: s(if c.resource_id.is_some() {
                "Zugeordnet"
            } else if c.registered {
                "Registriert"
            } else if c.current {
                "Prüfen"
            } else {
                "Älterer Stand"
            }),
        })
        .collect();
    // The same folder found by name is listed once, as the finding.
    let unique = |a: &&ArtifactFinding| {
        !snapshot.discoveries.iter().any(|c| c.scope_id == a.scope_id && c.rel_path == a.rel_path)
    };
    if matches!(filter, 0 | 3) {
        rows.extend(
            snapshot
                .artifacts
                .iter()
                .enumerate()
                .filter(|(_, a)| unique(a))
                .filter(|(_, a)| found(format!("{} {} {} {}", a.name, a.rel_path, a.project, a.detail)))
                .map(|(index, a)| CandidateRow {
                    index: index as i32,
                    artifact: true,
                    path: s(format!("{} · {}", artifact_label(a.category), a.name)),
                    evidence: s(format!("{} · {}", a.rel_path, a.project)),
                    status: s(if a.current { "Erfasst" } else { "Älterer Stand" }),
                }),
        );
    }
    store.set_candidates(model(rows));
    let total = snapshot.discoveries.len() + snapshot.artifacts.iter().filter(|a| unique(a)).count();
    store.set_finding_count(s(total.to_string()));
}

/// A finding without its own name is the root of its scope.
fn candidate_name(snapshot: &Snapshot, c: &ProjectCandidate) -> String {
    if !c.name.is_empty() && c.name != "." {
        return c.name.clone();
    }
    if !c.rel_path.is_empty() {
        return c.rel_path.clone();
    }
    if !c.project.is_empty() {
        return c.project.clone();
    }
    snapshot.scopes.iter().find(|s| s.id == c.scope_id).map_or_else(|| "Ordnerwurzel".into(), |s| folder_label(&s.path))
}

/// Last component of a Windows path, for titles that must not lose it.
pub fn folder_label(path: &str) -> String {
    path.trim_end_matches('\\').rsplit('\\').next().filter(|n| !n.is_empty()).unwrap_or(path).to_string()
}

pub fn artifact_label(category: ItemCategory) -> &'static str {
    match category {
        ItemCategory::Environment => "Umgebung",
        ItemCategory::Output => "Build-Ausgabe",
        ItemCategory::Cache => "Cache",
        _ => "Fund",
    }
}

pub fn render_snapshot(ui: &Dashboard, snapshot: &Snapshot, data_path: &str, state: &ViewState<'_>) {
    let store = ui.global::<Store>();
    let now = SystemClock.now();
    let settings = snapshot.settings.clone().unwrap_or_default();
    store.set_demo(snapshot.demo);
    store.set_sqlite_version(s(&snapshot.sqlite_version));
    store.set_data_path(s(data_path));
    store.set_version(s(env!("CARGO_PKG_VERSION")));
    store.set_last_scan(s(match snapshot.last_scan_at {
        _ if snapshot.demo => "Beispieldaten · keine Erfassung".to_string(),
        Some(t) if now - t < DAY => format!("Zuletzt erfasst heute, {}", display_time(t)),
        Some(t) => format!("Zuletzt erfasst {}", display_relative(t, now)),
        None => "Noch nicht erfasst".to_string(),
    }));
    let watched = snapshot.scopes.iter().filter(|s| s.watched).count();
    store.set_monitoring_text(s(match settings.pause_pending(now) {
        _ if snapshot.demo => "Überwachung aus (Demo)".to_string(),
        Some(until) => format!("Überwachung pausiert bis {}", display_time(until)),
        None if settings.monitoring_enabled => format!("Überwachung an · {watched} Ordner beobachtet"),
        None => "Überwachung aus".to_string(),
    }));

    // Key figures of the overview
    store.set_project_count(s(snapshot.projects.len().to_string()));
    // A searched package page counts only its matches; keep the total.
    let query = &snapshot.packages.query;
    if query.text.trim().is_empty() && query.ecosystem.is_empty() {
        store.set_package_count(s(snapshot.packages.counts.all.to_string()));
    }
    store.set_ai_count(s(snapshot.ai.len().to_string()));
    let measured: Vec<i64> = snapshot.projects.iter().filter_map(|p| p.bytes).collect();
    let unmeasured = snapshot.projects.len() - measured.len();
    store.set_storage_total(s(if measured.is_empty() {
        "—".to_string()
    } else {
        format_bytes(Some(measured.iter().sum()))
    }));
    store.set_storage_caption(s(match unmeasured {
        0 => "Projektgröße".to_string(),
        n => format!("Projektgröße · {n} ohne Messung"),
    }));
    store.set_review_count(snapshot.review_count() as i32);
    let stalls = stalled(snapshot, now);
    let stalled_ids: Vec<i64> =
        stalls.iter().filter(|(.., st)| *st != Stall::LongRunning).map(|(id, ..)| *id).collect();
    store.set_stalled_count(stalls.iter().filter(|(.., st)| *st != Stall::LongRunning).count() as i32);

    render_mascot(&store, snapshot, &settings, state, stalled_ids.len(), now);
    render_attention(&store, snapshot, &stalls);
    render_projects(ui, snapshot);
    render_resources(ui, snapshot);
    render_packages(ui, &snapshot.packages, state.package_key);
    render_experiments(ui, snapshot, state.experiment);
    render_system(&store, snapshot, now);
    super::disk::render(ui, &snapshot.disks);

    render_candidates(ui, snapshot);

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
                references: a.references.len() as i32,
                scope: s(if a.scope.is_empty() { "Projekt" } else { &a.scope }),
            })
            .collect(),
    ));
    let users = snapshot.ai.iter().filter(|a| a.scope == "Benutzer").count();
    store.set_ai_user_count(s(users.to_string()));
    store.set_ai_project_count(s((snapshot.ai.len() - users).to_string()));

    store.set_events(model(
        snapshot
            .activity
            .iter()
            .map(|e| EventRow { title: s(&e.title), detail: s(&e.detail), time: s(&e.created_at), kind: s(&e.kind) })
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
                stalled: stalled_ids.contains(&h.resource_id),
            })
            .collect(),
    ));

    store.set_scopes(model(
        snapshot
            .scopes
            .iter()
            .map(|scope| ScopeRow {
                id: scope.id as i32,
                name: s(folder_label(&scope.path)),
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

    render_settings(&store, snapshot, &settings, now);
}

/// Mochi shows the state; it only speaks when the state needs a word.
/// The state is always also given as text elsewhere.
fn render_mascot(
    store: &Store<'_>,
    snapshot: &Snapshot,
    settings: &Settings,
    state: &ViewState<'_>,
    stalled: usize,
    now: i64,
) {
    let partial = !snapshot.demo
        && snapshot.scopes.iter().any(|s| s.status != ScopeStatus::Approved || !s.current() && s.latest.is_some());
    let (pose, line) = if state.scanning {
        (4, "Ich erfasse gerade. Du kannst weiterarbeiten.".to_string())
    } else if let Some(until) = settings.pause_pending(now).filter(|_| !snapshot.demo) {
        (5, format!("Überwachung pausiert bis {}.", display_time(until)))
    } else if stalled > 0 && !snapshot.demo {
        (6, format!("{stalled} Experiment{} wartet auf deine Entscheidung.", if stalled == 1 { "" } else { "e" }))
    } else if partial {
        (6, "Ein Ordner ist nur teilweise erfasst.".to_string())
    } else if !snapshot.demo && snapshot.projects.is_empty() {
        (1, "Hallo! Füge deinen ersten Projektordner hinzu.".to_string())
    } else {
        (1, String::new())
    };
    store.set_mascot_pose(pose);
    store.set_mascot_line(s(line));
}

/// Decisions that live on other pages. Review hints are listed on the
/// overview itself and therefore not repeated here.
fn render_attention(store: &Store<'_>, snapshot: &Snapshot, stalls: &[(i64, &Session, Stall)]) {
    let name_of = |id: i64| snapshot.resources.iter().find(|r| r.id == id).map_or("?".to_string(), |r| r.name.clone());
    let names = |ids: Vec<String>| {
        let mut ids = ids;
        ids.dedup();
        let shown = ids.iter().take(2).cloned().collect::<Vec<_>>().join(" · ");
        if ids.len() > 2 { format!("{shown} · +{}", ids.len() - 2) } else { shown }
    };
    #[allow(clippy::too_many_arguments)]
    fn card(
        kind: &str,
        label: &str,
        count: usize,
        title: String,
        detail: String,
        action: &str,
        page: i32,
        tab: i32,
    ) -> AttentionCard {
        AttentionCard {
            kind: s(kind),
            label: s(label),
            count: s(count.to_string()),
            title: s(title),
            detail: s(detail),
            action: s(action),
            page,
            tab,
        }
    }
    let mut cards = Vec::new();
    // A stuck scratchpad with a hint is already in the hint list.
    let stuck: Vec<_> = stalls
        .iter()
        .filter(|(id, _, st)| *st != Stall::LongRunning && !snapshot.hints.iter().any(|h| h.resource_id == *id))
        .collect();
    if !stuck.is_empty() {
        cards.push(card(
            "alert",
            "Experiment hängt",
            stuck.len(),
            names(stuck.iter().map(|(id, ..)| name_of(*id)).collect()),
            format!("{} · prüfe die Ergebnisse und gib die Session frei", stuck[0].2.label()),
            "Öffnen",
            4,
            0,
        ));
    }
    if snapshot.packages.counts.deprecated > 0 {
        cards.push(card(
            "alert",
            "Abgekündigt",
            snapshot.packages.counts.deprecated,
            "Paketversionen vom Anbieter abgekündigt oder zurückgezogen".into(),
            "Welche Projekte sie nutzen, steht in den Paketdetails".into(),
            "Pakete ansehen",
            2,
            0,
        ));
    }
    let partial: Vec<&Scope> = snapshot
        .scopes
        .iter()
        .filter(|s| s.status != ScopeStatus::Approved || !s.current() && s.latest.is_some())
        .collect();
    if !partial.is_empty() {
        cards.push(card(
            "review",
            "Teilweise erfasst",
            partial.len(),
            names(partial.iter().map(|s| s.path.rsplit('\\').next().unwrap_or(&s.path).to_string()).collect()),
            "Limit oder Lesefehler; der letzte vollständige Stand bleibt sichtbar".into(),
            "Freigaben ansehen",
            6,
            1,
        ));
    }
    let open =
        |group: FindingGroup| snapshot.system.of(group).filter(|f| f.scope.is_some() && f.scope_id.is_none()).count();
    let stores = open(FindingGroup::Store);
    if stores > 0 && cards.len() < 3 {
        cards.push(card(
            "info",
            "Neu gefunden",
            stores,
            "Globale Paketbestände auf diesem Rechner".into(),
            "Erlaube das Lesen, um ihre Pakete zu erfassen".into(),
            "Ansehen",
            2,
            1,
        ));
    }
    let personal = open(FindingGroup::AiConfig);
    if personal > 0 && cards.len() < 3 {
        cards.push(card(
            "info",
            "Neu gefunden",
            personal,
            "Persönliche KI-Ordner, zum Beispiel .claude".into(),
            "Erlaube das Lesen, um Skills und MCP-Server zu erfassen".into(),
            "Ansehen",
            3,
            1,
        ));
    }
    cards.truncate(3);
    store.set_attention(model(cards));
}

pub fn render_projects(ui: &Dashboard, snapshot: &Snapshot) {
    let store = ui.global::<Store>();
    store.set_projects(model(
        snapshot
            .projects
            .iter()
            .map(|p| {
                let (coverage, coverage_kind) = coverage_cell(snapshot, p);
                ProjectRow {
                    id: p.id as i32,
                    name: s(&p.name),
                    path: s(&p.path),
                    ecosystem: s(&p.ecosystem),
                    state: s(p.state.label()),
                    state_kind: s(observation_kind(p.state)),
                    size: s(format_bytes(p.bytes)),
                    detail: s(&p.evidence),
                    approved: p.read_approved,
                    coverage: s(coverage),
                    coverage_kind: s(coverage_kind),
                }
            })
            .collect(),
    ));
}

/// Registered resources: scratchpads, their folders and adopted findings.
pub fn render_resources(ui: &Dashboard, snapshot: &Snapshot) {
    let store = ui.global::<Store>();
    store.set_resources(model(
        snapshot
            .resources
            .iter()
            .map(|r| {
                let (state, kind) = if r.expendable {
                    ("Entbehrlich markiert".to_string(), "review")
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
}

fn upstream(pkg: &PackageFinding) -> (String, &'static str) {
    if pkg.lifecycle.checked_at.is_none() && pkg.local_deprecation.is_some() {
        return ("Lockfile: abgekündigt".into(), "alert");
    }
    let stale = if pkg.lifecycle_stale && pkg.lifecycle.checked_at.is_some() { " · älter" } else { "" };
    let kind = if pkg.lifecycle.needs_review() {
        "alert"
    } else if pkg.update == Some(UpdateStatus::UpdateAvailable) {
        "next"
    } else if pkg.lifecycle.checked_at.is_some() {
        "ok"
    } else {
        "neutral"
    };
    let label = if kind == "next" { "Neuere Version".to_string() } else { pkg.lifecycle.label().to_string() };
    (format!("{label}{stale}"), kind)
}

/// Where the package list comes from, in plain words.
fn package_coverage(c: &PackageCoverage) -> String {
    let mut parts = vec![format!(
        "Aus {} vollständig erfassten Projekt{}",
        c.complete_projects,
        if c.complete_projects == 1 { "" } else { "en" }
    )];
    if c.incomplete_projects > 0 {
        parts.push(format!("{} noch nicht vollständig erfasst", c.incomplete_projects));
    }
    if c.incomplete_roots > 0 {
        parts.push(format!("{} Suchordner unvollständig", c.incomplete_roots));
    }
    if c.truncated {
        parts.push("Auswertung begrenzt".into());
    }
    parts.join(" · ")
}

pub fn render_packages(ui: &Dashboard, report: &PackageReport, selected: Option<&str>) {
    let store = ui.global::<Store>();
    store.set_package_page_size(PACKAGE_PAGE as i32);
    store.set_package_coverage(s(package_coverage(&report.coverage)));
    store.set_package_offset(report.offset as i32);
    store.set_package_previous(report.offset > 0);
    store.set_package_next(report.offset + report.packages.len() < report.total);
    store.set_package_page_label(s(format!(
        "{}–{} von {} Versionen",
        if report.packages.is_empty() { 0 } else { report.offset + 1 },
        report.offset + report.packages.len(),
        report.total
    )));
    let c = report.counts;
    store.set_package_counts(model(
        [c.all, c.review, c.deprecated, c.unreferenced, c.kept].iter().map(|n| s(n.to_string())).collect(),
    ));
    store.set_packages(model(
        report
            .packages
            .iter()
            .map(|pkg| {
                let (upstream, upstream_kind) = upstream(pkg);
                PackageRow {
                    key: s(&pkg.key),
                    name: s(&pkg.id.name),
                    version: s(if pkg.id.version.is_empty() { "?" } else { &pkg.id.version }),
                    source: s(format!(
                        "{} · {}",
                        pkg.id.ecosystem,
                        if pkg.id.source_host.is_empty() { pkg.id.source.as_str() } else { &pkg.id.source_host }
                    )),
                    upstream: s(upstream),
                    upstream_kind: s(upstream_kind),
                    references: s(pkg.reference_label()),
                    update: s(match pkg.update {
                        Some(UpdateStatus::UpdateAvailable) => "Neuere Version",
                        Some(UpdateStatus::Current) => "Aktuell",
                        Some(status) => status.label(),
                        None => "Nicht geprüft",
                    }),
                    review: pkg.review,
                    kept: pkg.retained,
                }
            })
            .collect(),
    ));
    let finding = selected.and_then(|key| report.packages.iter().find(|p| p.key == key));
    store.set_package_panel(finding.is_some());
    if let Some(pkg) = finding {
        let (status, kind) = upstream(pkg);
        store.set_panel_title(s(format!(
            "{} · {}",
            pkg.id.name,
            if pkg.id.version.is_empty() { "?" } else { &pkg.id.version }
        )));
        store.set_panel_status(s(status));
        store.set_panel_status_kind(s(kind));
        store.set_panel_source(s(format!(
            "{} · {} · {}",
            pkg.id.ecosystem,
            if pkg.id.source_host.is_empty() { pkg.id.source.as_str() } else { &pkg.id.source_host },
            pkg.lifecycle.checked_at.map_or("Quelle nicht geprüft".to_string(), |t| format!(
                "Quelle geprüft {}",
                display_relative(t, SystemClock.now())
            ))
        )));
        let mut chain: Vec<ChainStep> = pkg
            .occurrences
            .iter()
            .take(3)
            .map(|o| ChainStep {
                label: s(&o.project),
                detail: s(format!(
                    "{} · {}{}",
                    o.rel_path,
                    o.state.map_or("Cache/Metadaten", InstallState::label),
                    if o.current { "" } else { " · älterer Stand" }
                )),
                warn: false,
            })
            .collect();
        chain.push(ChainStep {
            label: s("Upstream"),
            detail: s(pkg
                .lifecycle
                .reason
                .clone()
                .or_else(|| pkg.local_deprecation.clone())
                .unwrap_or_else(|| pkg.lifecycle.label().to_string())),
            warn: pkg.lifecycle.needs_review() || pkg.local_deprecation.is_some(),
        });
        store.set_panel_chain(model(chain));
        store.set_panel_advice(s(&pkg.next_action));
        store.set_panel_note(s(if pkg.possible_references.is_empty() {
            format!("Keine Befehls- oder MCP-Verweise erkannt. {}", pkg.coverage)
        } else {
            format!("Mögliche Verweise: {}", pkg.possible_references.join(" · "))
        }));
        store.set_panel_kept(pkg.retained);
        store.set_panel_key(s(&pkg.key));
    }
}

fn experiment_state(snapshot: &Snapshot, scratch: &Resource, now: i64) -> (&'static str, &'static str) {
    let sessions: Vec<&Session> = snapshot.sessions.iter().filter(|s| s.scratch_id == scratch.id).collect();
    if sessions.iter().any(|s| s.stall(now).is_some_and(|st| st != Stall::LongRunning)) {
        ("Hängt", "alert")
    } else if sessions.iter().any(|s| matches!(s.state, SessionState::Starting | SessionState::Running)) {
        ("Läuft", "running")
    } else if scratch.review_at.is_some_and(|at| at <= now) {
        ("Prüftermin", "review")
    } else if sessions.iter().any(|s| s.state == SessionState::CompletedVerified) {
        ("Geprüft", "ok")
    } else {
        ("Ruhig", "neutral")
    }
}

pub fn render_experiments(ui: &Dashboard, snapshot: &Snapshot, selected: Option<i64>) {
    let store = ui.global::<Store>();
    let now = SystemClock.now();
    let project_name = |id: Option<i64>| {
        id.and_then(|id| snapshot.projects.iter().find(|p| p.id == id))
            .map_or("ohne Projekt".to_string(), |p| p.name.clone())
    };
    let scratches: Vec<&Resource> = snapshot.resources.iter().filter(|r| r.kind == ResourceKind::Scratchpad).collect();
    store.set_experiments(model(
        scratches
            .iter()
            .map(|r| {
                let (state, kind) = experiment_state(snapshot, r, now);
                ExperimentRow {
                    id: r.id as i32,
                    name: s(&r.name),
                    project: s(project_name(r.project_id)),
                    purpose: s(r.purpose.clone().unwrap_or_else(|| "ohne Zweck".into())),
                    state: s(state),
                    state_kind: s(kind),
                }
            })
            .collect(),
    ));
    let Some(scratch) = selected.and_then(|id| scratches.iter().find(|r| r.id == id)) else {
        store.set_experiment_id(-1);
        return;
    };
    store.set_experiment_id(scratch.id as i32);
    store.set_experiment_name(s(&scratch.name));
    store.set_experiment_path(s(&scratch.path));
    let (review, review_kind) = match scratch.review_at {
        Some(at) if at <= now => {
            (format!("Prüftermin {}", display_relative(at, now).replace("vor ", "seit ")), "review")
        }
        Some(at) => (format!("Prüftermin in {} Tagen", ((at - now) / DAY).max(0)), "neutral"),
        None => (String::new(), "neutral"),
    };
    store.set_experiment_review(s(review));
    store.set_experiment_review_kind(s(review_kind));
    let sessions: Vec<&Session> = snapshot.sessions.iter().filter(|s| s.scratch_id == scratch.id).collect();
    let mut stall_text = String::new();
    store.set_experiment_sessions(model(
        sessions
            .iter()
            .map(|session| {
                let stall = session.stall(now);
                let releasable = stall.is_some_and(|st| st != Stall::LongRunning);
                if releasable && stall_text.is_empty() {
                    stall_text = format!(
                        "Session {}: {} ({}). Die Sperre bleibt; prüfe results\\ und gib sie begründet frei oder führe erneut aus.",
                        session.id,
                        stall.map_or("", Stall::label),
                        display_relative(now - session.quiet_for(now), now)
                    );
                }
                SessionRow {
                    id: session.id as i32,
                    scratch: s(&scratch.name),
                    scratch_id: scratch.id as i32,
                    purpose: s(&session.purpose),
                    state: s(session.state.label()),
                    state_kind: s(match session.state {
                        _ if releasable => "alert",
                        SessionState::Starting | SessionState::Running | SessionState::CompletionRequested => "running",
                        SessionState::CompletedVerified | SessionState::ReleasedAfterReview => "ok",
                        SessionState::InterruptedUnknown => "alert",
                    }),
                    started: s(display_relative(session.started_at, now)),
                    stall: s(stall.map_or("", Stall::label)),
                    releasable,
                    note: s(session.note.clone().unwrap_or_default()),
                }
            })
            .collect(),
    ));
    store.set_experiment_stall(s(stall_text));
    let children: Vec<&Resource> = snapshot.resources.iter().filter(|r| r.parent_id == Some(scratch.id)).collect();
    let locked = sessions.iter().any(|s| s.state.locks_resources());
    store.set_experiment_plannable(
        !locked && children.iter().any(|c| c.kind == ResourceKind::Temporary && c.expendable && !c.protected),
    );
    store.set_experiment_folders(model(
        children
            .iter()
            .map(|c| {
                let (fate, kind) = match c.kind {
                    ResourceKind::Source | ResourceKind::Results => ("Bleibt", "ok"),
                    ResourceKind::Temporary if c.expendable => ("Entbehrlich markiert", "review"),
                    ResourceKind::Temporary => ("Bleibt, bis markiert", "neutral"),
                    _ => ("Separat prüfen", "neutral"),
                };
                FolderRow {
                    name: s(format!("{}\\", c.path.rsplit('\\').next().unwrap_or(&c.name))),
                    size: s(format_bytes(c.bytes)),
                    fate: s(fate),
                    fate_kind: s(kind),
                }
            })
            .collect(),
    ));
}

fn render_system(store: &Store<'_>, snapshot: &Snapshot, now: i64) {
    let row = |f: &SystemFinding| {
        let evidence_kind = match f.evidence {
            FindingEvidence::Installed => "ok",
            FindingEvidence::Location => "neutral",
            _ if f.note.contains("keine globale Version") => "review",
            _ => "info",
        };
        SystemRow {
            id: s(&f.id),
            name: s(&f.name),
            location: s(&f.location),
            evidence: s(f.evidence.label()),
            evidence_kind: s(evidence_kind),
            note: s(&f.note),
            approvable: f.scope.is_some(),
            approved: f.scope_id.is_some(),
            coverage: s(&f.coverage),
            scope_id: f.scope_id.unwrap_or(-1) as i32,
        }
    };
    let group = |g: FindingGroup| model(snapshot.system.of(g).map(row).collect::<Vec<_>>());
    store.set_system_tools(group(FindingGroup::Tool));
    store.set_system_stores(group(FindingGroup::Store));
    store.set_system_ai(group(FindingGroup::AiConfig));
    store.set_system_checked(s(match snapshot.system.checked_at {
        _ if snapshot.demo => String::new(),
        Some(t) => format!(
            "Geprüft {} · {} Suchpfad-Ordner · {} Orte",
            if now - t < DAY { format!("heute, {}", display_time(t)) } else { display_relative(t, now) },
            snapshot.system.path_entries,
            snapshot.system.locations
        ),
        None => "Noch nicht geprüft".into(),
    }));
}

pub fn render_search(ui: &Dashboard, hits: &[SearchHit]) {
    let store = ui.global::<Store>();
    store.set_search_results(model(
        hits.iter()
            .map(|h| SearchRow {
                area: s(h.area.as_str()),
                area_label: s(h.area.label()),
                key: s(&h.key),
                title: s(&h.title),
                detail: s(&h.detail),
            })
            .collect(),
    ));
}

pub fn render_progress(ui: &Dashboard, progress: &ScanProgress) {
    let store = ui.global::<Store>();
    store.set_scan_running(progress.running);
    store.set_scan_cancelling(progress.running && progress.cancelling);
    if !progress.running {
        return;
    }
    let total = progress.total.max(1);
    store.set_scan_progress(((progress.done as f32 + 0.5) / total as f32).min(1.0));
    store.set_scan_text(s(if progress.total > 1 {
        format!("Bereich {} von {} · {} Einträge gelesen", (progress.done + 1).min(total), total, progress.entries)
    } else {
        format!("{} Einträge gelesen · nichts wird gestartet", progress.entries)
    }));
}

/// What a finished pass found, in one sentence, and its tone (`ok`/`warn`).
/// Scopes are named by project or kind, never by their internal number.
pub fn scan_notice(value: &Value, snapshot: &Snapshot) -> (String, &'static str) {
    let reports = value.as_array().map(Vec::as_slice).unwrap_or_default();
    if reports.is_empty() {
        return ("Noch nichts freigegeben. Füge zuerst einen Projektordner hinzu.".into(), "warn");
    }
    let name = |report: &Value| {
        let id = report["scope_id"].as_i64();
        match snapshot.scopes.iter().find(|scope| Some(scope.id) == id) {
            Some(scope) => scope
                .project_id
                .and_then(|project| snapshot.projects.iter().find(|p| p.id == project))
                .map_or_else(|| scope.kind.label().to_string(), |p| p.name.clone()),
            None => format!("Bereich {}", report["scope_id"]),
        }
    };
    let problems: Vec<String> =
        reports.iter().filter_map(|r| r["problem"].as_str().map(|problem| format!("{}: {problem}", name(r)))).collect();
    if !problems.is_empty() {
        return (problems.join(" · "), "warn");
    }
    let partial: Vec<String> = reports.iter().filter(|r| r["state"] == "partial").map(name).collect();
    if !partial.is_empty() {
        return (
            format!(
                "Teilweise erfasst: {} (Limit oder Lesefehler). Details unter Einstellungen › Lesefreigaben.",
                partial.join(", ")
            ),
            "warn",
        );
    }
    let subject = match reports {
        [one] => name(one),
        _ => format!("{} Bereiche", reports.len()),
    };
    let sum = |key: &str| reports.iter().filter_map(|r| r[key].as_u64()).sum::<u64>();
    let (added, removed, changed) = (sum("added"), sum("removed"), sum("changed"));
    let text = if reports.iter().all(|r| r["baseline"] == true) {
        format!("{subject} erfasst · Vergleichsbasis erstellt.")
    } else if added + removed + changed == 0 {
        format!("{subject} erfasst · keine Änderungen seit der letzten Erfassung.")
    } else {
        format!("{subject} erfasst · {added} neu, {removed} entfernt, {changed} geändert.")
    };
    (text, "ok")
}

pub fn render_scan_notice(ui: &Dashboard, text: &str, kind: &str) {
    let store = ui.global::<Store>();
    store.set_scan_notice_kind(s(kind));
    store.set_scan_notice(s(text));
}

/// Status changes leave the data models and their scroll/focus state intact.
pub fn render_status(ui: &Dashboard, busy: bool, status: &str) {
    let store = ui.global::<Store>();
    store.set_busy(busy);
    store.set_status(s(status));
}

fn render_settings(store: &Store<'_>, snapshot: &Snapshot, settings: &Settings, now: i64) {
    store.set_monitoring(settings.monitoring_enabled);
    store.set_notifications(settings.notifications_enabled);
    store.set_updates(settings.network_updates_enabled);
    store.set_guide_enabled(settings.guide_enabled);
    store.set_daily_summary(settings.daily_summary);
    store.set_summary_hour(i32::from(settings.summary_hour));
    store.set_quiet_enabled(settings.quiet_hours.is_some());
    let quiet = settings.quiet_hours.unwrap_or(QuietHours { from: 19, to: 8 });
    store.set_quiet_from(i32::from(quiet.from));
    store.set_quiet_to(i32::from(quiet.to));
    store.set_pause_text(s(settings
        .pause_pending(now)
        .map(|until| format!("Pausiert bis {}", display_time(until)))
        .unwrap_or_default()));
    store.set_autostart_available(snapshot.autostart.is_some() || snapshot.demo);
    store.set_autostart(snapshot.autostart.is_some_and(|a| a.enabled));
    store.set_autostart_note(s(match snapshot.autostart {
        Some(a) if a.enabled && !a.current => {
            "Der Starteintrag zeigt auf ein anderes Programm oder Datenverzeichnis. Aus- und wieder einschalten aktualisiert ihn."
        }
        _ => "",
    }));
    store.set_sources(model(
        PUBLIC_UPDATE_SOURCES
            .iter()
            .map(|host| SourceToggle { host: s(*host), enabled: settings.update_sources.iter().any(|h| h == host) })
            .collect(),
    ));
    store.set_scratch_root(s(settings.scratch_root.clone().unwrap_or_default()));
}
