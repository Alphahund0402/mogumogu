//! Human-readable output. `--json` prints the versioned machine format.
use crate::commands::View;
use mogumogu::domain::{Observation, PlanSummary, ReviewHint, Snapshot, format_bytes};
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
        ("open_sessions", "Offene Sessions"),
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
        let state =
            if r.state == Observation::Unknown && r.expendable { "entbehrlich markiert" } else { r.state.label() };
        lines.push(format!(
            "  #{:<4} {:<32} {:<11} {:<10} {}",
            r.id,
            r.name,
            r.kind.label(),
            format_bytes(r.bytes),
            state
        ));
    }
    lines.push(format!("{} Lesefreigaben", snapshot.scopes.len()));
    for s in &snapshot.scopes {
        lines.push(format!("  #{:<4} {:<26} {}  ({})", s.id, s.kind.label(), s.path, s.coverage_label()));
    }
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
    let mut lines =
        vec![format!("Plan #{} · {} · Fingerabdruck {}", plan.id, plan.state.label(), plan.short_fingerprint())];
    for t in &plan.targets {
        lines.push(format!("  Ziel {} ({} Einträge, {})", t.resource_name, t.entries, format_bytes(Some(t.bytes))));
        lines.push(format!("      {}", t.path));
        for blocker in &t.blockers {
            lines.push(format!("      Blocker: {blocker}"));
        }
    }
    lines.push("  Bleibt erhalten:".into());
    lines.extend(plan.kept.iter().map(|k| format!("      {k}")));
    lines.push(format!("  Wiederherstellung: {}", plan.recovery));
    if plan.done + plan.failed > 0 {
        lines.push(format!("  Ergebnis: {} entfernt, {} nicht entfernt", plan.done, plan.failed));
    }
    if let Some(note) = &plan.note {
        lines.push(format!("  Hinweis: {note}"));
    }
    if plan.state == mogumogu::domain::PlanState::Draft && plan.executable() {
        lines.push(format!(
            "  Bestätigen: mogumogu-cli cleanup approve --plan {} --confirm {}",
            plan.id,
            plan.short_fingerprint()
        ));
    }
    lines.join("\n")
}
