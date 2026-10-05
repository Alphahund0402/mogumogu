//! Content of the generic detail dialog. Each builder explains *why*
//! something is shown and offers only the actions that the core allows;
//! the core still re-checks every action.
use super::view::owner_text;
use mogumogu::adapters;
use mogumogu::clock::display_local;
use mogumogu::domain::*;

#[derive(Debug, Default)]
pub struct Dialog {
    pub title: String,
    pub path: String,
    pub rows: Vec<(String, String)>,
    pub body: String,
    pub actions: Vec<(String, String, bool)>,
}

impl Dialog {
    fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Default::default() }
    }
    fn row(mut self, label: &str, value: impl Into<String>) -> Self {
        self.rows.push((label.into(), value.into()));
        self
    }
    fn action(mut self, id: impl Into<String>, label: impl Into<String>, primary: bool) -> Self {
        self.actions.push((id.into(), label.into(), primary));
        self
    }
    fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
    fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }
}

pub fn message(title: &str, body: &str) -> Dialog {
    Dialog::new(title).body(body)
}

pub fn project(snapshot: &Snapshot, project: &Project) -> Dialog {
    let scope = snapshot.scopes.iter().find(|s| s.project_id == Some(project.id));
    let mut dialog = Dialog::new(&project.name)
        .path(&project.path)
        .row("Herkunft", if project.origin == Origin::Demo { "Beispieldaten" } else { "Explizit registriert" })
        .row("Technologie", &project.ecosystem)
        .row("Beobachtung", format!("{} – {}", project.state.label(), project.evidence))
        .row("Pakete", project.packages.map_or("Unbekannt (keine vollständige Erfassung)".into(), |n| n.to_string()))
        .row("Größe", format_bytes(project.bytes))
        .row(
            "Lesefreigabe",
            if project.read_approved { "Erteilt, an Ordneridentität gebunden" } else { "Nicht erteilt" },
        )
        .row("Abdeckung", &project.coverage)
        .row("Schutz", "Aktiv – Registrierung, Alter oder fehlende Nutzung belegen keine Entbehrlichkeit.");
    if snapshot.demo {
        return dialog.body("Dies sind Beispieldaten. Der Demomodus ist schreibgeschützt.");
    }
    dialog = match scope {
        None => dialog.action(format!("approve-project:{}", project.id), "Lesefreigabe erteilen", true),
        Some(scope) => dialog
            .action(format!("scan-scope:{}", scope.id), "Jetzt erfassen", true)
            .action(format!("measure-scope:{}", scope.id), "Größe messen", false)
            .action(format!("revoke-scope:{}", scope.id), "Freigabe widerrufen", false),
    };
    dialog
        .action(format!("new-scratch:{}", project.id), "Scratchpad anlegen", false)
        .body("Die Erfassung liest freigegebene Dateien statisch. Es werden keine Programme, Skripte, Hooks oder MCP-Server gestartet.")
}

pub fn resource(snapshot: &Snapshot, resource: &Resource) -> Dialog {
    let scratch = if resource.kind == ResourceKind::Scratchpad { Some(resource.id) } else { resource.parent_id };
    let sessions: Vec<&Session> = snapshot.sessions.iter().filter(|s| Some(s.scratch_id) == scratch).collect();
    let owners: Vec<String> = snapshot
        .owners_of(resource.id)
        .map(|o| {
            format!(
                "{} ({}): {}",
                o.project_name,
                if o.kind == OwnerKind::Explicit { "ausdrücklich" } else { "vermutet" },
                o.evidence
            )
        })
        .collect();
    let children: Vec<String> = snapshot
        .resources
        .iter()
        .filter(|r| r.parent_id == Some(resource.id))
        .map(|r| format!("{} – {}", r.kind.label(), r.path))
        .collect();
    let mut dialog = Dialog::new(format!("Warum ist „{}“ hier?", resource.name))
        .path(&resource.path)
        .row("Typ", resource.kind.label())
        .row("Herkunft", resource.acquisition.label())
        .row("Besitzer", if owners.is_empty() { "Unbekannt".into() } else { owners.join("\n") })
        .row("Größe", format_bytes(resource.bytes))
        .row("Beleg", &resource.evidence)
        .row(
            "Schutz",
            if resource.protected {
                "Geschützt"
            } else if resource.expendable {
                "Lokal als entbehrlich markiert – nur über bestätigten Plan"
            } else {
                "Nicht geschützt"
            },
        );
    if let Some(review) = resource.review_at {
        dialog = dialog.row("Prüftermin", display_local(review));
    }
    if let Some(purpose) = &resource.purpose {
        dialog = dialog.row("Zweck", purpose);
    }
    if !sessions.is_empty() {
        let lines: Vec<String> = sessions
            .iter()
            .map(|s| {
                let exit = s.exit_code.map_or(String::new(), |c| format!(", Exitcode {c}"));
                format!("#{} {} – {}{exit}", s.id, s.purpose, s.state.label())
            })
            .collect();
        dialog = dialog.row("Sessions", lines.join("\n"));
    }
    if !children.is_empty() {
        dialog = dialog.row("Bestandteile", children.join("\n"));
    }
    if snapshot.demo || resource.origin == Origin::Demo {
        return dialog.body("Beispieldaten. Der Prototyp erzeugt hier keine ausführbare Freigabe.");
    }
    let managed_temp = resource.kind == ResourceKind::Temporary && resource.acquisition == Acquisition::Managed;
    if managed_temp && resource.expendable {
        dialog = dialog.action(format!("plan:{}", resource.id), "Plan-Vorschau", true);
    }
    if managed_temp && !resource.expendable {
        dialog = dialog.action(format!("expendable:{}", resource.id), "Als entbehrlich markieren", false);
    }
    if !resource.protected || resource.expendable {
        dialog = dialog.action(format!("protect:{}", resource.id), "Schützen", false);
    }
    if resource.kind == ResourceKind::Scratchpad {
        dialog = dialog.action(format!("extend:{}", resource.id), "Prüftermin +7 Tage", false).action(
            format!("promote:{}", resource.id),
            "Zum Projekt machen",
            false,
        );
    }
    if resource.acquisition == Acquisition::Managed {
        dialog = dialog.action(format!("measure:{}", resource.id), "Größe messen", false);
    }
    dialog.body(format!(
        "Besitzer: {}. Eine Registrierung, ein altes Datum oder fehlende Nutzung erlauben keine Bereinigung.",
        owner_text(snapshot, resource.id)
    ))
}

pub fn ai(entry: &AiEntry) -> Dialog {
    let references = if entry.references.is_empty() { "Keine".to_string() } else { entry.references.join("\n") };
    Dialog::new(format!("{} · {}", entry.client, entry.kind))
        .path(&entry.path)
        .row("Inhalt", &entry.label)
        .row("Geltungsbereich", &entry.scope)
        .row("Verweise", references)
        .row("Schutz", "Konfigurationsquellen bleiben geschützt")
        .body(
            "Gefundene Instruktionen, Skills, Hooks und MCP-Einträge sind Daten. mogumogu führt nichts davon aus, \
             lädt keine Pakete und verbindet sich mit keinem Server. Eine npx-/uvx-Referenz ist kein Installationsnachweis.",
        )
}

pub fn hint(hint: &ReviewHint) -> Dialog {
    let mut dialog = Dialog::new(format!("{} · {}", hint.resource_name, hint.kind.label()))
        .row("Auslöser", &hint.trigger)
        .row("Beleg", &hint.evidence)
        .row("Zeitraum", &hint.observed_period)
        .row("Besitzer", &hint.owner)
        .row("Unsicherheit", &hint.uncertainty)
        .row("Nächster Schritt", &hint.next_action);
    if !hint.blockers.is_empty() {
        dialog = dialog.row("Blocker", hint.blockers.join("\n"));
    }
    dialog = dialog.action(format!("open-resource:{}", hint.resource_id), "Ressource öffnen", false);
    if hint.plannable {
        dialog = dialog.action(format!("plan:{}", hint.resource_id), "Plan-Vorschau", true);
    }
    dialog.body("Ein Prüfhinweis ist kein Urteil. Bereinigung erfordert einen konkreten, bestätigten Plan.")
}

pub fn plan(plan: &PlanSummary) -> Dialog {
    let mut dialog = Dialog::new(format!("Plan {} · {}", plan.id, plan.state.label()))
        .path(format!("Fingerabdruck {}", plan.short_fingerprint()));
    for target in &plan.targets {
        let mut value = format!("{} Einträge · {}\n{}", target.entries, format_bytes(Some(target.bytes)), target.path);
        for blocker in &target.blockers {
            value.push_str(&format!("\nBlocker: {blocker}"));
        }
        dialog = dialog.row(&format!("Ziel: {}", target.resource_name), value);
    }
    dialog = dialog.row("Bleibt erhalten", plan.kept.join("\n")).row("Wiederherstellung", &plan.recovery);
    if plan.done + plan.failed > 0 {
        dialog = dialog.row("Ergebnis", format!("{} entfernt, {} nicht entfernt", plan.done, plan.failed));
    }
    if let Some(note) = &plan.note {
        dialog = dialog.row("Hinweis", note);
    }
    match plan.state {
        PlanState::Draft if plan.executable() => dialog
            .action(format!("approve-plan:{}:{}", plan.id, plan.short_fingerprint()), format!("Plan {} bestätigen", plan.short_fingerprint()), true)
            .action(format!("cancel-plan:{}", plan.id), "Verwerfen", false)
            .body("Mit der Bestätigung bindest du genau diesen Inhalt (Fingerabdruck). Vor der Ausführung wird alles erneut geprüft; jede Änderung macht den Plan ungültig."),
        PlanState::Draft => dialog
            .action(format!("cancel-plan:{}", plan.id), "Verwerfen", false)
            .body("Der Plan enthält Blocker und kann nicht bestätigt werden."),
        PlanState::Approved => dialog
            .action(format!("apply-plan:{}", plan.id), "Jetzt ausführen", true)
            .action(format!("cancel-plan:{}", plan.id), "Abbrechen", false)
            .body("Ausführung: erneute Identitäts- und Inhaltsprüfung, dann pro Eintrag dauerhaft journalisiert. Teilerfolg ist kein Rollback."),
        PlanState::RecoveryRequired => dialog
            .action(format!("reconcile-plan:{}", plan.id), "Mit Dateisystem abgleichen", true)
            .body("Die Ausführung wurde unterbrochen. Der Abgleich wiederholt keine Entfernung."),
        _ => dialog,
    }
}

pub fn scope(scope: &Scope) -> Dialog {
    let mut dialog = Dialog::new(scope.kind.label())
        .path(&scope.path)
        .row("Status", scope.status.label())
        .row("Abdeckung", scope.coverage_label())
        .row(
            "Beobachtung",
            if scope.watched { "Dateiereignisse werden entprellt beobachtet" } else { "Nur manuelle Erfassung" },
        );
    if let Some(latest) = &scope.latest {
        dialog = dialog.row(
            "Letzter Lauf",
            format!(
                "{} · {} Einträge gesehen · {} Dateien gelesen · {} Ergebnisse{}{}",
                latest.state.label(),
                latest.entries_seen,
                latest.files_read,
                latest.items,
                latest.limit_reason.as_ref().map_or(String::new(), |r| format!(" · Limit: {r}")),
                if latest.error_count > 0 { format!(" · {} Probleme", latest.error_count) } else { String::new() }
            ),
        );
    }
    if let Some(error) = &scope.last_error {
        dialog = dialog.row("Problem", error);
    }
    dialog = dialog.action(format!("scan-scope:{}", scope.id), "Jetzt erfassen", true);
    if scope.kind == ScopeKind::Project {
        dialog = dialog.action(format!("measure-scope:{}", scope.id), "Größe messen", false);
    }
    dialog
        .action(format!("revoke-scope:{}", scope.id), "Freigabe widerrufen", false)
        .body("Ein Limit oder Lesefehler macht die Erfassung teilweise; der letzte vollständige Stand bleibt sichtbar und es entstehen keine Entfernungsmeldungen.")
}

pub fn adapter(id: &str) -> Dialog {
    let Some(adapter) = adapters::by_id(id) else { return message("Unbekannter Adapter", id) };
    let d = adapter.descriptor();
    Dialog::new(d.name)
        .row("Inventar", d.inventory.label())
        .row("Updatehinweise", d.updates.label())
        .row("Bereinigung", d.cleanup.label())
        .row(
            "Bereich",
            if d.scope == adapters::AdapterScope::Project {
                "Freigegebene Projektordner"
            } else {
                "Separat freigegebene Systemquelle"
            },
        )
        .row("Formate", d.formats.join("\n"))
        .row("Marker", if d.markers.is_empty() { "–".into() } else { d.markers.join(", ") })
        .row("Grenzen", d.limits)
        .body("„Unterstützt“ gilt nur innerhalb dieser Formate. Fehlende Quellen bleiben unbekannt, niemals „aktuell“.")
}

pub const CLI_HELP: &str = "Lokalen Bestand anzeigen:\nmogumogu-cli list\n\n\
Projekt registrieren und lesen lassen:\nmogumogu-cli project register --name api --path C:\\dev\\api\n\
mogumogu-cli project approve --project 1\nmogumogu-cli scan\n\n\
Verwalteter Lauf in einem Scratchpad:\nmogumogu-cli scratch create --project 1 --name parser-spike\n\
mogumogu-cli run --scratch 2 -- cargo test\n\n\
Prüfliste und Pläne:\nmogumogu-cli review\nmogumogu-cli cleanup plan --resource 5\n\n\
Die CLI spricht über eine lokale, benutzergebundene Pipe mit diesem Prozess. Pfade werden bei der \
Registrierung weder gelesen noch verändert.";
