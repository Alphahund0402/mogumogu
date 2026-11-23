//! Human-readable output. `--json` prints the versioned machine format.
use crate::commands::View;
use mogumogu::clock::{display_date, display_local};
use mogumogu::domain::{
    CheckGroup, FinishItem, FinishReport, PlanSummary, ProjectCheck, ProjectTidy, ReviewHint, Snapshot, TidyReport,
    checks_summary, format_bytes, growth_text, report_size,
};
use serde_json::{Value, json};

pub const JSON_VERSION: u32 = 1;

pub fn json_ok(result: &Value) -> String {
    serde_json::to_string_pretty(&json!({ "v": JSON_VERSION, "ok": true, "result": result })).unwrap_or_default()
}

pub fn json_error(error: &mogumogu::Error) -> String {
    serde_json::to_string_pretty(&json!({
        "v": JSON_VERSION,
        "ok": false,
        "code": error.code(),
        "message": error.to_string(),
    }))
    .unwrap_or_default()
}

pub fn human(view: View, value: &Value) -> String {
    let rendered = match view {
        View::Status => status(value),
        View::List => serde_json::from_value::<Snapshot>(value.clone()).ok().map(|s| list(&s)),
        View::Review => serde_json::from_value::<Vec<ReviewHint>>(value.clone()).ok().map(|h| review(&h)),
        View::Plan => serde_json::from_value::<PlanSummary>(value.clone()).ok().map(|p| plan(&p)),
        View::Json => None,
        View::DiskCsv => {
            serde_json::from_value::<mogumogu::domain::DiskPage>(value.clone()).ok().map(|page| page.csv())
        }
        View::Checks(id) => serde_json::from_value::<TidyReport>(value.clone())
            .ok()
            .and_then(|report| report.projects.into_iter().find(|p| p.project_id == id))
            .map(|project| checks(&project)),
        View::Finish => serde_json::from_value::<FinishReport>(value.clone()).ok().map(|report| finish(&report)),
    };
    rendered.unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default())
}

fn status(value: &Value) -> Option<String> {
    let map = value.as_object()?;
    let labels = [
        ("mode", "Modus"),
        ("data_dir", "Datenverzeichnis"),
        ("schema_version", "Schema"),
        ("sqlite_version", "SQLite"),
        ("projects", "Projekte"),
        ("resources", "Ressourcen"),
        ("scopes", "Lesefreigaben"),
        ("catalog", "Adapter/Katalog"),
        ("cleanup", "Bereinigung"),
        ("network", "Netzwerk"),
    ];
    Some(
        labels
            .iter()
            .filter_map(|(key, label)| {
                map.get(*key).map(|v| format!("{label:<18} {}", v.as_str().map_or(v.to_string(), str::to_string)))
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn list(snapshot: &Snapshot) -> String {
    let mut lines =
        vec![format!("{} Projekte{}", snapshot.projects.len(), if snapshot.demo { " (Demodaten)" } else { "" })];
    for p in &snapshot.projects {
        let packages = p.packages.map_or("–".into(), |n| n.to_string());
        lines.push(format!(
            "  #{:<4} {:<24} {:<10} Pakete {:<6} {}  [{}]",
            p.id,
            p.name,
            p.state.label(),
            packages,
            p.path,
            if p.protected { "geschützt" } else { "ungeschützt" }
        ));
    }
    lines.push(format!("{} Ressourcen", snapshot.resources.len()));
    for r in &snapshot.resources {
        lines.push(format!(
            "  #{:<4} {:<32} {:<11} {:<10} {}",
            r.id,
            r.name,
            r.kind.label(),
            format_bytes(r.bytes),
            r.state.label()
        ));
    }
    lines.push(format!("{} Lesefreigaben", snapshot.scopes.len()));
    for s in &snapshot.scopes {
        lines.push(format!("  #{:<4} {:<26} {}  ({})", s.id, s.kind.label(), s.path, s.coverage_label()));
    }
    lines.join("\n")
}

fn checks(project: &ProjectTidy) -> String {
    let mut lines = vec![format!("Offene Punkte · {} · {}", project.name, checks_summary(&project.checks))];
    for group in CheckGroup::ALL {
        lines.push(group.label().to_string());
        for check in project.checks.iter().filter(|c| c.group == *group) {
            lines.push(format!("  [{:<9}] {} – {}", check.state.label(), check.title, check.detail));
        }
    }
    lines.push("Kein Löschurteil: „erledigt“ heißt nur, dass die gelesenen Dateien nichts Offenes zeigen.".into());
    lines.join("\n")
}

fn finish(report: &FinishReport) -> String {
    let size = report_size;
    let check = |c: &ProjectCheck| format!("  [{:<9}] {} – {}", c.state.label(), c.title, c.detail);
    let since = report.growth_since.map(display_date);
    let folder = |item: &FinishItem| {
        let grown = growth_text(item.grown, since.as_deref());
        let changed =
            item.modified_at.map_or("geändert: unbekannt".to_string(), |t| format!("geändert {}", display_date(t)));
        format!(
            "  {} · {} · {} · {grown} · {changed}\n      {}",
            item.rel_path,
            item.label,
            size(item.bytes),
            item.evidence
        )
    };
    let mut lines = vec![format!(
        "Abschlussbericht · {} · erfasst {} · nur lesend, ändert nichts",
        report.name,
        display_local(report.checked_at)
    )];
    if !report.complete {
        lines.push("Teilweise erfasst: Größen sind Untergrenzen, der eigene Inhalt ist unbekannt.".into());
    }
    if !report.note.is_empty() {
        lines.push(format!("Hinweis: {}", report.note));
    }
    match report.finished_at {
        Some(at) if report.changed_since_finish() => lines.push(format!(
            "Abgeschlossen am {}; seither geändert. Reaktivieren: project reactivate --project {}",
            display_date(at),
            report.project_id
        )),
        Some(at) => lines.push(format!("Abgeschlossen am {}.", display_date(at))),
        None => lines.push("Aktiv. Abschließen geht nur im Dashboard (Projektseite → Abschlussbericht …).".into()),
    }
    lines.push(format!("\n1. Ist die Arbeit gesichert? ({} offen oder unbekannt)", report.unsaved()));
    lines.extend(report.saved.iter().map(check));
    if !report.completeness.is_empty() {
        lines.push("   Vollständig, zur Kenntnis:".into());
        lines.extend(report.completeness.iter().map(check));
    }
    lines.push("\n2. Bleibt ohne Frage".into());
    lines.push(format!(
        "  Eigener Inhalt {} · Versionsverwaltung {}",
        report.own.map_or("unbekannt".to_string(), size),
        size(report.vcs)
    ));
    lines.extend(report.held.iter().map(folder));
    lines.extend(
        report
            .held_unmeasured
            .iter()
            .map(|rel| format!("  {rel} · von dir behalten · nicht unter den größten Einträgen gemessen")),
    );
    lines.push(format!(
        "\n3. Du entscheidest ({} unter „Prüfen“; nie Teil eines Plans, gelöscht wird außerhalb von mogumogu)",
        size(report.decide_bytes)
    ));
    if report.decide.is_empty() {
        lines.push("  Nichts unter den größten Einträgen.".into());
    }
    lines.extend(report.decide.iter().map(folder));
    lines.push(format!(
        "\n4. Kann in den Papierkorb ({}, belegt {}; nur über einen Papierkorb-Plan im Dashboard)",
        size(report.recoverable_bytes),
        size(report.recoverable_allocated)
    ));
    if report.recoverable.is_empty() {
        lines.push("  Nichts Wiederherstellbares.".into());
    }
    lines.extend(report.recoverable.iter().map(folder));
    if !report.runs.is_empty() {
        lines.push("\nAußerhalb, zur Kenntnis: Läufe, die du noch nicht freigegeben hast".into());
        lines.extend(report.runs.iter().map(|run| {
            format!("  #{} {} · seit {} · {} Artefakte", run.id, run.title, display_date(run.started_at), run.artifacts)
        }));
    }
    lines.push("\nAbschließen ändert keinen Schutz; „erledigt“ ist kein Löschurteil.".into());
    lines.join("\n")
}

fn review(hints: &[ReviewHint]) -> String {
    if hints.is_empty() {
        return "Keine Prüfhinweise. Unbekannte Nutzung wird nicht als entbehrlich eingestuft.".into();
    }
    let mut lines = vec!["Prüfliste – kein Löschplan:".to_string()];
    for h in hints {
        lines.push(format!("  #{} {} — {}", h.resource_id, h.resource_name, h.kind.label()));
        lines.push(format!("      Auslöser: {}", h.trigger));
        lines.push(format!("      Beleg: {} · Besitzer: {} · {}", h.evidence, h.owner, h.observed_period));
        lines.push(format!("      Unsicherheit: {}", h.uncertainty));
        lines.push(format!("      Nächster Schritt: {}", h.next_action));
        for blocker in &h.blockers {
            lines.push(format!("      Blocker: {blocker}"));
        }
    }
    lines.join("\n")
}

fn plan(plan: &PlanSummary) -> String {
    let mut lines = vec![
        format!("Plan #{} · {} · Fingerabdruck {}", plan.id, plan.state.label(), plan.short_fingerprint()),
        format!("  Projekt {} · {}", plan.project, plan.root),
    ];
    for t in &plan.targets {
        lines.push(format!("  In den Papierkorb: {} ({}, {})", t.rel_path, t.label, format_bytes(Some(t.bytes))));
        if !t.evidence.is_empty() {
            lines.push(format!("      Beleg: {}", t.evidence));
        }
        for blocker in &t.blockers {
            lines.push(format!("      Blocker: {blocker}"));
        }
        if let Some(error) = &t.error {
            lines.push(format!("      {}: {error}", t.state.label()));
        }
    }
    lines.push("  Bleibt erhalten:".into());
    lines.extend(plan.kept.iter().map(|k| format!("      {k}")));
    lines.push(format!("  Wiederherstellung: {}", plan.recovery));
    if let Some(outcome) = plan.outcome_text() {
        lines.push(format!("  {outcome}"));
    }
    if let Some(space) = plan.space_text() {
        lines.push(format!("  Laufwerk: {space}"));
    }
    if let Some(note) = &plan.note {
        lines.push(format!("  Hinweis: {note}"));
    }
    if plan.state == mogumogu::domain::PlanState::Draft && plan.executable() {
        lines.push(format!(
            "  Bestätigen und ausführen: im Dashboard unter Übersicht → Bereinigungspläne → Plan {} (Fingerabdruck {})",
            plan.id,
            plan.short_fingerprint()
        ));
    }
    lines.join("\n")
}
