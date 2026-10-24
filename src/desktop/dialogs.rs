//! Content of the generic detail dialog. Each builder explains *why*
//! something is shown and offers only the actions that the core allows;
//! the core still re-checks every action.
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
    pub(super) fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Default::default() }
    }
    pub(super) fn row(mut self, label: &str, value: impl Into<String>) -> Self {
        self.rows.push((label.into(), value.into()));
        self
    }
    pub(super) fn action(mut self, id: impl Into<String>, label: impl Into<String>, primary: bool) -> Self {
        self.actions.push((id.into(), label.into(), primary));
        self
    }
    pub(super) fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
    pub(super) fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }
}

pub fn message(title: &str, body: &str) -> Dialog {
    Dialog::new(title).body(body)
}

pub fn package(pkg: &PackageFinding) -> Dialog {
    let occurrences = pkg
        .occurrences
        .iter()
        .map(|o| {
            format!(
                "{} · {} · {}{}{}",
                o.project,
                o.rel_path,
                o.state.map_or("Cache/Metadaten", InstallState::label),
                if o.current { "" } else { " (älterer Stand)" },
                o.owned_files.map_or(String::new(), |n| format!(" · {n} RECORD-Dateieinträge, nicht abgeglichen"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut dialog =
        Dialog::new(format!("{} @ {}", pkg.id.name, if pkg.id.version.is_empty() { "?" } else { &pkg.id.version }))
            .row("Quelle", format!("{} · {} · {}", pkg.id.ecosystem, pkg.id.source, pkg.id.source_host))
            .row("Abdeckung", &pkg.coverage)
            .row(
                "Projektbelege",
                if pkg.references.is_empty() {
                    "Keine passenden Lockfile-Belege gefunden".into()
                } else {
                    pkg.references.join("\n")
                },
            )
            .row(
                "Mögliche Verweise",
                if pkg.possible_references.is_empty() {
                    "Keine erkannt; manuelle und dynamische Nutzung unbekannt".into()
                } else {
                    pkg.possible_references.join("\n")
                },
            )
            .row("Fundstellen (Auswahl)", occurrences);
    if let Some(reason) = &pkg.local_deprecation {
        dialog = dialog.row("Lokaler Lockfile-Hinweis", reason);
    }
    dialog.row("Nächster Schritt", &pkg.next_action)
        .action(format!("keep-package:{}:{}",if pkg.retained {"0"} else {"1"},pkg.key),if pkg.retained {"Behalten-Vormerkung lösen"} else {"Behalten"},true)
        .body("Belege stammen aus Manifesten und Lockdateien; dynamische Nutzung sieht mogumogu nicht. Pakete werden hier nie geändert.")
}

pub fn artifact(snapshot: &Snapshot, index: usize) -> Dialog {
    let Some(item) = snapshot.artifacts.get(index) else {
        return message("Fundstelle nicht gefunden", "Inventar aktualisieren.");
    };
    let root = snapshot.scopes.iter().find(|s| s.id == item.scope_id).map_or("?", |s| s.path.as_str());
    Dialog::new(&item.name).path(format!("{root}\\{}",item.rel_path.replace('/',"\\")))
        .row("Bezug",&item.project).row("Beleg",&item.detail)
        .row("Abdeckung",if item.current {"Aktueller vollständiger Stand"} else {"Älterer vollständiger Stand"})
        .body("Aus den Metadaten der Erfassung erkannt. Größe und weitere Nutzer sind nicht geprüft; entfernt wird hier nichts.")
}

pub fn candidate(snapshot: &Snapshot, index: usize) -> Dialog {
    let Some(candidate) = snapshot.discoveries.get(index) else {
        return message("Fund nicht gefunden", "Suchordner erneut erfassen.");
    };
    let root = snapshot.scopes.iter().find(|s| s.id == candidate.scope_id).map_or("?", |s| s.path.as_str());
    let mut dialog=Dialog::new(format!("{} · {}", candidate.kind.label(), candidate.name)).path(format!("{root}\\{}",candidate.rel_path.replace('/',"\\")))
        .row("Beleg",&candidate.evidence).row("Abdeckung",if candidate.current {"Vollständige Dateinamensuche"} else {"Älterer vollständiger Stand"})
        .row("Projektbezug", super::view::candidate_owner(candidate))
        .row("Größe", format_bytes(candidate.bytes))
        .row("Änderungsdatum", candidate.modified_at.map_or("Nicht erfasst".into(), display_local))
        .body("Nur am Namen erkannt, Inhalte wurden nicht gelesen. Ordne den Fund einem Projekt zu, damit er bei der nächsten Prüfung bekannt ist.");
    if candidate.current && !candidate.registered && candidate.is_dir && candidate.kind.is_project() {
        dialog = dialog.action(
            format!("register-candidate:{}:{}", candidate.scope_id, candidate.rel_path),
            "Projekt registrieren",
            true,
        );
    }
    if candidate.current && !candidate.registered && !candidate.kind.is_project() {
        dialog = dialog.action(format!("adopt-candidate:{index}"), "Projekt zuordnen", true);
    }
    if let Some(id) = candidate.resource_id {
        dialog = dialog.action(format!("open-resource:{id}"), "Ressource öffnen", true);
    }
    dialog
}

/// What AI tools may reach here and how to narrow it. mogumogu changes no
/// settings; the steps are for the user or a later, separate approval.
pub fn access(access: &ProjectAccess) -> Dialog {
    let mut dialog = Dialog::new(format!("KI-Zugriff · {}", access.name))
        .path(&access.path)
        .row("Einstufung", access.level.label())
        .row("Werkzeuge", access.tools.join(", "))
        .row("Geltung", if access.audience == "Benutzer" { "Alle Projekte dieses Benutzers" } else { "Dieser Ordner" })
        .row("Befunde", access.findings.join("\n"));
    if !access.keep_out.is_empty() {
        dialog = dialog.row("Kontextfilter-Kandidaten", access.keep_out.join("\n"));
    }
    if access.audience == "Projekt" {
        dialog = dialog.action(format!("filter-preview:{}", access.scope_id), "Kontextfilter vorbereiten …", true);
    }
    dialog.body(
        "Engere Grenzen in drei Stufen: 1. Kontextfilter – Ausschlüsse in den Einstellungen des Werkzeugs (z. B. permissions.deny in .claude/settings.local.json, .cursorignore, .geminiignore) für die genannten Ordner, Testdaten und .env-Dateien. 2. Arbeitskopie – ein Git-Worktree nur mit versionierten Quellen. 3. Isolierte Umgebung – WSL2 mit eingeschalteter Sandbox, ein Dev Container oder Windows-Sandbox mit nur diesem Ordner. Claude Code sandboxt Shell-Befehle unter nativem Windows nicht. mogumogu ändert Einstellungen nur über „Kontextfilter vorbereiten …“ und nach deiner Bestätigung.",
    )
}

/// The exact additions before anything is written.
pub fn filter_plan(plan: &mogumogu::service::FilterPlan) -> Dialog {
    let mut dialog = Dialog::new(format!("Kontextfilter · {}", plan.project));
    for file in &plan.files {
        let state = if file.exists {
            format!("vorhanden, {} bestehende Verbote bleiben", file.kept)
        } else {
            "wird angelegt".to_string()
        };
        dialog = dialog.row(&format!("{} · {}", file.tool, file.rel_path), state).row(
            "Hinzu",
            if file.add.is_empty() { "Nichts – alle Regeln sind schon da".to_string() } else { file.add.join("\n") },
        );
    }
    dialog = dialog.row("Hinweise", plan.hints.join("\n"));
    if plan.files.iter().any(|f| !f.add.is_empty()) && !plan.written {
        dialog =
            dialog.action(format!("filter-apply:{}:{}", plan.scope_id, plan.fingerprint), "In Datei schreiben", true);
    }
    dialog.body("mogumogu ergänzt nur die Liste permissions.deny in deiner persönlichen Datei; .claude/settings.json, andere Einstellungen und bestehende Einträge bleiben. Ändert sich eine Datei vor dem Schreiben, ist eine neue Vorschau nötig.")
}

/// The batch for every approved project before anything is written.
pub fn filter_batch(batch: &mogumogu::service::FilterBatch) -> Dialog {
    let mut dialog = Dialog::new("Kontextfilter · alle Projekte");
    for plan in &batch.plans {
        let file = plan.files.first();
        let value = match (plan.written, file) {
            (true, _) => format!("{} Leseregeln geschrieben", plan.additions()),
            (false, Some(file)) if file.exists => format!("{} Leseregeln hinzu · {}", plan.additions(), file.rel_path),
            (false, _) => format!("{} Leseregeln · .claude/settings.local.json wird angelegt", plan.additions()),
        };
        let mut value = value;
        if plan.hints.iter().any(|h| h.starts_with("Git schließt")) {
            value.push_str("\nGit schließt die Datei nicht erkennbar aus");
        }
        dialog = dialog.row(&plan.project, value);
    }
    if batch.complete > 0 {
        dialog = dialog.row("Schon vollständig", format!("{} Projekte", batch.complete));
    }
    for skip in &batch.skipped {
        dialog = dialog.row(&format!("{} · ausgelassen", skip.project), &skip.reason);
    }
    let pending = batch.plans.iter().any(|p| !p.written);
    if pending && batch.written == 0 {
        dialog = dialog.action(format!("filter-all-apply:0:{}", batch.fingerprint), "In alle Dateien schreiben", true);
    }
    dialog.body(if batch.plans.is_empty() {
        "Kein Projekt braucht neue Leseregeln."
    } else {
        "Für jedes Projekt ergänzt mogumogu nur permissions.deny in .claude/settings.local.json: Umgebungen, Ausgaben, Caches und Testdaten des Projekts sowie .env- und Schlüsseldateien. Die Regeln je Projekt zeigt „Kontextfilter vorbereiten …“ im Zugriff-Dialog. Ändert sich vor dem Schreiben eine Datei, ist eine neue Vorschau nötig."
    })
}

/// Certain outer projects of the searches, before and after one confirmed
/// step that registers them and approves them for reading.
pub fn found_projects(batch: &mogumogu::service::ProjectBatch) -> Dialog {
    const SHOWN: usize = 60;
    let mut dialog = Dialog::new(format!("Gefundene Projekte übernehmen · {}", batch.projects.len()));
    for project in batch.projects.iter().take(SHOWN) {
        dialog = dialog.row(&project.name, &project.path);
    }
    if batch.projects.len() > SHOWN {
        dialog = dialog.row("Weitere", format!("{} Projekte", batch.projects.len() - SHOWN));
    }
    for skip in &batch.skipped {
        dialog = dialog.row("Ausgelassen", format!("{} · {}", skip.path, skip.reason));
    }
    if !batch.projects.is_empty() && batch.approved.is_empty() && batch.skipped.is_empty() {
        dialog = dialog.action(format!("found-apply:0:{}", batch.fingerprint), "Registrieren und Lesen erlauben", true);
    }
    dialog.body(if batch.projects.is_empty() {
        "Keine weiteren sicheren Projekte: Alles Gefundene ist registriert, liegt in einem registrierten Projekt oder braucht einen Blick im einzelnen Fund."
    } else {
        "Projekte mit Versionsverwaltung und Manifest oder Lockfile, die in keinem anderen Fund und keinem registrierten Projekt liegen. mogumogu registriert sie und erlaubt, sie zu lesen – gebunden an die jeweilige Ordneridentität; danach folgt eine Erfassung. Teilprojekte, unsichere und redigierte Funde übernimmst du einzeln. Ändern sich die Funde vorher, ist eine neue Vorschau nötig."
    })
}

/// What one project holds, largest first. Nothing here removes anything.
pub fn tidy(report: &ProjectTidy) -> Dialog {
    let size = |b: u64| format_bytes(i64::try_from(b).ok());
    let mut dialog = Dialog::new(format!("Aufräumen · {}", report.name))
        .path(&report.path)
        .row("Gesamt", report.total.map_or_else(|| "Unvollständig gemessen".to_string(), size))
        .row("Wiederherstellbar", size(report.regenerable))
        .row("Prüfen", size(report.review))
        .row(
            "Davon Testdaten",
            match report.own_content().filter(|own| *own > 0) {
                Some(own) if report.test_data > 0 => format!(
                    "{} · {} % des eigenen Projektinhalts{}",
                    size(report.test_data),
                    report.test_data * 100 / own,
                    if report.heavy_test_data() { " – auffällig viel" } else { "" }
                ),
                _ => size(report.test_data),
            },
        )
        .row("Versionsverwaltung", size(report.vcs))
        .row("Zuletzt geändert", report.last_change.map_or("Unbekannt".into(), display_local))
        .row("Gemessen", display_local(report.checked_at));
    if !report.note.is_empty() {
        dialog = dialog.row("Hinweis", &report.note);
    }
    for item in report.items.iter().take(20) {
        dialog = dialog.row(
            &format!("{} · {}", item.class.label(), item.label),
            format!("{} · {} · {}", item.rel_path.replace('/', "\\"), size(item.bytes), item.evidence),
        );
    }
    if report.regenerable > 0 {
        dialog = dialog.action(format!("tidy-plan:{}", report.project_id), "Papierkorb-Plan …", true);
    }
    dialog
        .action(format!("tidy:{}", report.project_id), "Neu messen", false)
        .body("Wiederherstellbar heißt: Lockfile oder Build-Datei daneben, Werkzeugcache oder Test-Ausgabe – ein Werkzeug baut es neu. Name und Alter allein machen nichts entfernbar. Der Papierkorb-Plan zeigt zuerst, was verschoben würde; ohne Bestätigung passiert nichts.")
}

pub fn project(snapshot: &Snapshot, project: &Project) -> Dialog {
    let scope = snapshot.scopes.iter().find(|s| s.project_id == Some(project.id));
    let dialog = Dialog::new(&project.name)
        .path(&project.path)
        .row("Technologie", &project.ecosystem)
        .row("Beobachtung", format!("{} – {}", project.state.label(), project.evidence))
        .row("Pakete", project.packages.map_or("Unbekannt (keine vollständige Erfassung)".into(), |n| n.to_string()))
        .row("Größe", format_bytes(project.bytes))
        .row(
            "Lesefreigabe",
            if project.read_approved { "Erteilt, an Ordneridentität gebunden" } else { "Nicht erteilt" },
        )
        .row("Abdeckung", &project.coverage);
    if snapshot.demo {
        return dialog.body("Dies sind Beispieldaten. Der Demomodus ist schreibgeschützt.");
    }
    match scope {
        None => dialog.action(format!("approve-project:{}", project.id), "Lesefreigabe erteilen", true),
        Some(scope) => dialog
            .action(format!("scan-scope:{}", scope.id), "Jetzt erfassen", true)
            .action(format!("measure-scope:{}", scope.id), "Größe messen", false)
            .action(format!("revoke-scope:{}", scope.id), "Freigabe widerrufen", false),
    }
}

pub fn resource(snapshot: &Snapshot, resource: &Resource) -> Dialog {
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
    let mut dialog = Dialog::new(&resource.name)
        .path(&resource.path)
        .row("Typ", resource.kind.label())
        .row("Herkunft", resource.acquisition.label())
        .row("Besitzer", if owners.is_empty() { "Unbekannt".into() } else { owners.join("\n") })
        .row("Größe", format_bytes(resource.bytes))
        .row("Beleg", &resource.evidence)
        .row("Schutz", if resource.protected { "Geschützt" } else { "Nicht geschützt" });
    if let Some(review) = resource.review_at {
        dialog = dialog.row("Prüftermin", display_local(review));
    }
    if let Some(purpose) = &resource.purpose {
        dialog = dialog.row("Zweck", purpose);
    }
    if !children.is_empty() {
        dialog = dialog.row("Bestandteile", children.join("\n"));
    }
    if snapshot.demo || resource.origin == Origin::Demo {
        return dialog.body("Beispieldaten. Der Prototyp erzeugt hier keine ausführbare Freigabe.");
    }
    if !resource.protected {
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
    dialog
}

pub fn ai(entry: &AiEntry) -> Dialog {
    let references = if entry.references.is_empty() { "Keine".to_string() } else { entry.references.join("\n") };
    let mut dialog = Dialog::new(format!("{} · {}", entry.client, entry.kind))
        .path(&entry.path)
        .row("Zusammenfassung", &entry.label)
        .row("Geltungsbereich", &entry.scope)
        .row("Verweise", references);
    if entry.viewable {
        let action = entry
            .scope_id
            .map_or_else(|| "ai-content:demo".into(), |scope| format!("ai-content:{scope}:{}", entry.rel_path));
        dialog = dialog.action(action, "Inhalt ansehen", true);
    }
    dialog.body("Nur als Text gelesen: Es wird nichts gestartet, geladen oder kontaktiert. Ein npx- oder uvx-Verweis heißt nicht, dass das Paket installiert ist.")
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
    dialog.body("Ein Hinweis ist kein Urteil. Verschoben wird nur über einen Papierkorb-Plan, den du bestätigst.")
}

pub fn plan(plan: &PlanSummary) -> Dialog {
    let mut dialog = Dialog::new(format!("Papierkorb-Plan {} · {}", plan.id, plan.state.label()))
        .path(&plan.root)
        .row("Projekt", &plan.project)
        .row("Fingerabdruck", plan.short_fingerprint())
        .row("Zusammen", format_bytes(Some(plan.bytes())));
    for target in &plan.targets {
        let mut value = format!("{} · {}", target.label, format_bytes(Some(target.bytes)));
        if !target.evidence.is_empty() {
            value.push_str(&format!("\nBeleg: {}", target.evidence));
        }
        for blocker in &target.blockers {
            value.push_str(&format!("\nBlocker: {blocker}"));
        }
        if target.state != JournalState::Planned {
            value.push_str(&format!("\n{}", target.state.label()));
            if let Some(error) = &target.error {
                value.push_str(&format!(": {error}"));
            }
        }
        dialog = dialog.row(&target.rel_path.replace('/', "\\"), value);
    }
    dialog = dialog.row("Bleibt erhalten", plan.kept.join("\n")).row("Wiederherstellung", &plan.recovery);
    if plan.done + plan.failed > 0 {
        dialog = dialog.row("Ergebnis", format!("{} verschoben, {} nicht verschoben", plan.done, plan.failed));
    }
    if let Some(note) = &plan.note {
        dialog = dialog.row("Hinweis", note);
    }
    match plan.state {
        PlanState::Draft if plan.executable() => dialog
            .action(format!("approve-plan:{}:{}", plan.id, plan.short_fingerprint()), format!("Plan {} bestätigen", plan.short_fingerprint()), true)
            .action(format!("cancel-plan:{}", plan.id), "Verwerfen", false)
            .body("Mit der Bestätigung bindest du genau diese Ordner (Fingerabdruck). Vor dem Verschieben wird jeder Ordner erneut geprüft; jede Änderung stoppt ihn."),
        PlanState::Draft => dialog
            .action(format!("cancel-plan:{}", plan.id), "Verwerfen", false)
            .body("Der Plan enthält Blocker und kann nicht bestätigt werden."),
        PlanState::Approved => dialog
            .action(format!("apply-plan:{}", plan.id), "In den Papierkorb verschieben", true)
            .action(format!("cancel-plan:{}", plan.id), "Abbrechen", false)
            .body("Jeder Ordner wird unmittelbar vorher geprüft und als Ganzes in den Papierkorb verschoben; jeder Schritt wird protokolliert. Wäre ein Ordner zu groß für den Papierkorb, fragt Windows vor dem endgültigen Löschen."),
        PlanState::RecoveryRequired => dialog
            .action(format!("reconcile-plan:{}", plan.id), "Mit Dateisystem abgleichen", true)
            .body("Die Ausführung wurde unterbrochen. Der Abgleich verschiebt nichts erneut."),
        _ => dialog,
    }
}

pub fn scope(scope: &Scope) -> Dialog {
    let mut dialog = Dialog::new(scope.kind.label())
        .path(&scope.path)
        .row("Status", scope.status.label())
        .row("Abdeckung", scope.coverage_label());
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
        .body("Bei einem Limit oder Lesefehler bleibt der letzte vollständige Stand sichtbar.")
}

pub const HELP: &str = "Tastenkürzel\n\
Strg+K  Inventar durchsuchen\n\
Strg+N  Projekt hinzufügen\n\
F5  Ansicht neu laden\n\
Alt+1 … Alt+6  Bereiche in der Reihenfolge der Seitenleiste\n\
Esc  Dialog, Suche oder „Projekt hinzufügen“ schließen\n\n\
Das Fenster zu schließen beendet mogumogu nicht; es läuft im Tray weiter.\n\n\
Kommandozeile (mogumogu-cli)\n\
Bestand anzeigen: mogumogu-cli list\n\
Projekt aufnehmen: mogumogu-cli project register --name api --path C:\\dev\\api\n\
Lesen erlauben: mogumogu-cli project approve --project 1\n\
Erfassen: mogumogu-cli scan\n\
Aufräumbericht: mogumogu-cli tidy --project 1\n\
Papierkorb-Plan: mogumogu-cli cleanup plan --project 1\n\
Prüfhinweise: mogumogu-cli review\n\n\
Die CLI spricht über eine lokale, benutzergebundene Pipe mit diesem Prozess.";
