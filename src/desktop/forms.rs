//! Modal presentation, forms and the guided start. All work is submitted to
//! the owner; the native folder dialog only returns a path text.
use super::dialogs::Dialog;
use super::view::model;
use super::{Desktop, Form, dialogs, ui::*};
use mogumogu::clock::{Clock, SystemClock, iso_utc};
use mogumogu::service::Request;
use serde_json::Value;
use slint::ComponentHandle;

impl Desktop {
    // ------------------------------------------------------------ dialogs

    pub(super) fn dialog(&mut self, dialog: Dialog) {
        self.content_scope = None;
        let Some(ui) = &self.dashboard else { return };
        let store = ui.global::<Store>();
        store.set_detail_title(dialog.title.into());
        store.set_detail_path(dialog.path.into());
        store.set_detail_body(dialog.body.into());
        store.set_detail_rows(model(
            dialog
                .rows
                .into_iter()
                .map(|(label, value)| DetailRow { label: label.into(), value: value.into() })
                .collect(),
        ));
        store.set_detail_actions(model(
            dialog
                .actions
                .into_iter()
                .map(|(id, label, primary)| DetailAction { id: id.into(), label: label.into(), primary })
                .collect(),
        ));
        self.form = Form::None;
        store.set_modal(1);
        ui.invoke_focus_dialog();
    }

    fn form(&mut self, form: Form, title: &str, text: &str, fields: [(&str, &str, String); 2], submit: &str) {
        self.content_scope = None;
        let Some(ui) = &self.dashboard else { return };
        let store = ui.global::<Store>();
        store.set_form_title(title.into());
        store.set_form_text(text.into());
        let [(label1, hint1, value1), (label2, hint2, value2)] = fields;
        store.set_form_label_1(label1.into());
        store.set_form_hint_1(hint1.into());
        store.set_form_value_1(value1.into());
        store.set_form_label_2(label2.into());
        store.set_form_hint_2(hint2.into());
        store.set_form_value_2(value2.into());
        store.set_form_submit(submit.into());
        store.set_form_error("".into());
        store.set_form_project_picker(matches!(form, Form::Candidate | Form::Scratch));
        self.form = form;
        store.set_modal(2);
        ui.invoke_focus_dialog();
    }

    pub(super) fn close_modal(&mut self) {
        if let Some(ui) = &self.dashboard {
            ui.invoke_dismiss_dialog();
        }
        self.form = Form::None;
        self.adoption_candidate = None;
        self.content_scope = None;
    }

    pub(super) fn adopt_candidate_form(&mut self, index: usize) {
        let Some(candidate) = self.snapshot.discoveries.get(index).cloned() else { return };
        if self.snapshot.projects.is_empty() {
            self.dialog(dialogs::message(
                "Projekt zuordnen",
                "Registriere zuerst ein Projekt, dem dieser Fund gehört.",
            ));
            return;
        }
        self.form(
            Form::Candidate,
            "Fund einem Projekt zuordnen",
            "Bestätige den Besitzer und halte den Zweck fest. Der Fund bleibt geschützt.",
            [
                ("Projekt", "", String::new()),
                ("Zweck (optional)", "Zum Beispiel: Testdaten für den Parser", String::new()),
            ],
            "Zuordnen",
        );
        self.project_choices(candidate.project_id);
        self.adoption_candidate = Some(candidate);
    }

    fn project_choices(&self, selected: Option<i64>) {
        let Some(ui) = &self.dashboard else { return };
        let store = ui.global::<Store>();
        store.set_form_project_names(model(
            self.snapshot.projects.iter().map(|p| format!("{} · {}", p.name, p.path).into()).collect(),
        ));
        store.set_form_project_index(
            self.snapshot.projects.iter().position(|p| Some(p.id) == selected).unwrap_or(0) as i32
        );
    }

    fn selected_project(&self) -> Option<i64> {
        let ui = self.dashboard.as_ref()?;
        self.snapshot.projects.get(ui.global::<Store>().get_form_project_index() as usize).map(|p| p.id)
    }

    pub(super) fn open_project(&mut self, id: i64) {
        if let Some(project) = self.snapshot.projects.iter().find(|p| p.id == id).cloned() {
            let dialog = dialogs::project(&self.snapshot, &project);
            self.dialog(dialog);
        }
    }

    pub(super) fn open_resource(&mut self, id: i64) {
        if let Some(resource) = self.snapshot.resources.iter().find(|r| r.id == id).cloned() {
            let dialog = dialogs::resource(&self.snapshot, &resource);
            self.dialog(dialog);
        }
    }

    pub(super) fn new_scratch(&mut self, project_id: Option<i64>) {
        if self.snapshot.demo {
            self.submit(Request::ScratchCreate {
                project_id: 0,
                name: String::new(),
                purpose: String::new(),
                review_days: None,
            });
            return;
        }
        if self.snapshot.projects.is_empty() {
            self.dialog(dialogs::message(
                "Scratchpad anlegen",
                "Füge zuerst ein Projekt hinzu, dem das Scratchpad gehört.",
            ));
            return;
        }
        self.form(
            Form::Scratch,
            "Scratchpad anlegen",
            "Erstellt einen Arbeitsordner für Quellcode, Umgebung, temporäre Daten und Ergebnisse. Quellcode und Ergebnisse bleiben geschützt.",
            [
                ("Projekt", "", String::new()),
                ("Name", "Zum Beispiel: parser-spike", String::new()),
            ],
            "Anlegen",
        );
        self.project_choices(project_id);
    }

    /// The reason is the only way to end a stuck lock; nothing is stopped.
    pub(super) fn release_session(&mut self, session_id: i64) {
        self.form(
            Form::Release(session_id),
            "Session begründet freigeben",
            "Prüfe zuerst die Ergebnisse in results\\ und ob noch ein Prozess läuft. Dies löst nur die Session-Sperre; Prozesse und Dateien bleiben erhalten.",
            [
                ("Begründung", "Zum Beispiel: Absturz geprüft, Diagnose gesichert", String::new()),
                ("", "", String::new()),
            ],
            "Freigeben",
        );
    }

    pub(super) fn submit_form(&mut self, first: String, second: String) {
        if self.busy {
            return;
        }
        match self.form {
            Form::Scope => {
                match mogumogu::domain::ScopeKind::parse(first.trim())
                    .filter(|k| *k != mogumogu::domain::ScopeKind::Project)
                {
                    Some(kind) => self.submit(Request::ApproveSystem {
                        kind,
                        path: (!second.trim().is_empty()).then(|| second.trim().to_string()),
                    }),
                    None => self.form_error("Bereichsart nicht erkannt. Einen der genannten Bezeichner eingeben."),
                }
            }
            Form::Scratch => match self.selected_project() {
                Some(project_id) => self.submit(Request::ScratchCreate {
                    project_id,
                    name: second,
                    purpose: "Über das Dashboard angelegt".into(),
                    review_days: None,
                }),
                None => self.form_error("Wähle ein Projekt für das Scratchpad."),
            },
            Form::Release(session_id) => self.submit(Request::SessionRelease { session_id, reason: first }),
            Form::Candidate => {
                if let (Some(project_id), Some(candidate)) = (self.selected_project(), &self.adoption_candidate) {
                    self.submit(Request::AdoptCandidate {
                        scope_id: candidate.scope_id,
                        rel_path: candidate.rel_path.clone(),
                        project_id,
                        purpose: (!second.trim().is_empty()).then(|| second.trim().to_string()),
                    });
                }
            }
            Form::None => {}
        }
        self.render_status();
    }

    fn form_error(&self, message: &str) {
        if let Some(ui) = &self.dashboard {
            ui.global::<Store>().set_form_error(message.into());
        }
    }

    pub(super) fn approve_scope(&mut self, kind: &str) {
        self.form(Form::Scope,"Lesebereich freigeben",
            "Bereichsart: projects (Dateinamensuche), npm-global, uv-tools, python, cargo-tools oder nuget-cache. Für projects und python einen Ordner angeben; sonst kann der Standardpfad verwendet werden.",
            [("Bereichsart","npm-global",kind.to_string()),("Ordnerpfad","C:\\dev oder leer für Standardpfad",String::new())],"Zum Lesen freigeben");
    }

    // ------------------------------------------------------- guided start

    /// Opens the guide; an existing inventory starts at the folder step.
    pub(super) fn guide_open(&mut self, step: i32) {
        let Some(ui) = &self.dashboard else { return };
        let store = ui.global::<Store>();
        store.set_guide_step(step);
        store.set_guide_error("".into());
        store.set_guide_result("".into());
        store.set_search_open(false);
        store.set_guide_open(true);
        self.guide.project = None;
        self.guide.scope = None;
    }

    /// An empty local inventory greets with the guide unless it was skipped
    /// in this run or switched off in the settings.
    pub(super) fn maybe_open_guide(&self, ui: &Dashboard) {
        let store = ui.global::<Store>();
        let enabled = self.snapshot.settings.as_ref().is_some_and(|s| s.guide_enabled);
        if !self.snapshot.demo
            && enabled
            && self.snapshot.settings.is_some()
            && self.snapshot.projects.is_empty()
            && !self.guide.skipped
            && !store.get_guide_open()
        {
            store.set_guide_step(0);
            store.set_guide_error("".into());
            store.set_guide_open(true);
        }
    }

    pub(super) fn guide_close(&mut self) {
        if let Some(ui) = &self.dashboard {
            ui.global::<Store>().set_guide_open(false);
            ui.invoke_dismiss_dialog();
        }
    }

    pub(super) fn guide_skip(&mut self) {
        self.guide.skipped = true;
        self.guide_close();
    }

    pub(super) fn guide_disable(&mut self) {
        self.guide.skipped = true;
        self.guide_close();
        self.change_settings(|s| s.guide_enabled = false);
    }

    pub(super) fn guide_finish(&mut self) {
        self.guide_close();
        if let Some(ui) = &self.dashboard {
            ui.global::<Store>().set_page(0);
        }
    }

    pub(super) fn guide_browse(&mut self) {
        let Some(path) = self.pick_folder("Projektordner für mogumogu wählen") else { return };
        if let Some(ui) = &self.dashboard {
            let store = ui.global::<Store>();
            if store.get_guide_name().trim().is_empty() {
                store.set_guide_name(folder_name(&path).into());
            }
            store.set_guide_path(path.into());
            store.set_guide_error("".into());
        }
    }

    pub(super) fn guide_approve(&mut self) {
        let Some(ui) = &self.dashboard else { return };
        let store = ui.global::<Store>();
        let path = store.get_guide_path().trim().trim_end_matches('\\').to_string();
        if path.is_empty() {
            store.set_guide_error("Bitte einen Ordner wählen.".into());
            store.set_guide_step(1);
            return;
        }
        let name = match store.get_guide_name().trim() {
            "" => folder_name(&path),
            name => name.to_string(),
        };
        store.set_guide_name(name.clone().into());
        store.set_guide_error("".into());
        self.submit(Request::RegisterProject { name, path });
    }

    /// Advances the guide on its own requests; other outcomes pass through.
    pub(super) fn guide_step(&mut self, request: &Request, value: &Value) -> bool {
        let Some(ui) = &self.dashboard else { return false };
        let store = ui.global::<Store>();
        if !store.get_guide_open() {
            return false;
        }
        match request {
            Request::RegisterProject { .. } if self.guide.project.is_none() => {
                let Some(project_id) = value["project_id"].as_i64() else { return false };
                self.guide.project = Some(project_id);
                self.submit(Request::ApproveProject { project_id });
                true
            }
            Request::ApproveProject { project_id } if self.guide.project == Some(*project_id) => {
                let Some(scope_id) = value["scope_id"].as_i64() else { return false };
                self.guide.scope = Some(scope_id);
                if store.get_guide_scan_now() {
                    store.set_guide_step(3);
                    self.status = "Lesefreigabe erteilt – erste Erfassung läuft.".into();
                    self.submit(Request::Scan { scope_id: Some(scope_id) });
                } else {
                    store.set_guide_step(4);
                    store.set_guide_result(
                        "Freigegeben, noch nicht erfasst. „Jetzt erfassen“ in der Werkzeugleiste startet die Erfassung.".into(),
                    );
                }
                true
            }
            Request::Scan { scope_id: Some(id) } if self.guide.scope == Some(*id) => {
                store.set_guide_step(4);
                store.set_guide_result(guide_result(value).into());
                self.status = super::view::scan_notice(value, &self.snapshot).0;
                true
            }
            _ => false,
        }
    }

    /// A failed guide request returns to the step that can fix it.
    pub(super) fn guide_failed(&mut self, request: &Request, message: &str) -> bool {
        let Some(ui) = &self.dashboard else { return false };
        let store = ui.global::<Store>();
        if !store.get_guide_open() {
            return false;
        }
        let step = match request {
            Request::RegisterProject { .. } => 1,
            Request::ApproveProject { .. } => 2,
            Request::Scan { scope_id } if *scope_id == self.guide.scope => 2,
            _ => return false,
        };
        self.guide.project = None;
        store.set_guide_error(message.into());
        store.set_guide_step(step);
        true
    }

    // ----------------------------------------------- data, folders, export

    /// Native folder dialog; `None` when cancelled. Nothing is read or created.
    fn pick_folder(&mut self, title: &str) -> Option<String> {
        match mogumogu::platform::pick_folder(title) {
            Ok(path) => path,
            Err(error) => {
                self.status = format!("Ordnerauswahl nicht möglich: {error}");
                self.render_status();
                None
            }
        }
    }

    pub(super) fn browse_scratch_root(&mut self) {
        if let Some(path) = self.pick_folder("Ort für neue Scratchpads wählen") {
            self.change_settings(|s| s.scratch_root = Some(path));
        }
    }

    pub(super) fn backup_now(&mut self) {
        if let Some(folder) = self.pick_folder("Ordner für die Sicherung wählen") {
            let path = format!("{}\\mogumogu-sicherung-{}.sqlite3", folder.trim_end_matches('\\'), stamp());
            self.submit(Request::Backup { path });
        }
    }

    /// Shows what leaves before anything is written (PROJEKTPLAN §7.3).
    pub(super) fn export_preview(&mut self, include_paths: bool) {
        let s = &self.snapshot;
        let dialog = Dialog::new(if include_paths { "Export mit Pfaden" } else { "Redigierter Export" })
            .row("Projekte", format!("{} · {}", s.projects.len(), if include_paths { "mit Namen und Pfaden" } else { "als „Projekt 1 … n“" }))
            .row("Pakete", if include_paths { "Alle Namen, Versionen und Quellenklassen" } else { "Öffentliche Namen; private als „<privat oder unbekannt>“" })
            .row("Weitere Angaben", "Anzahl der Ressourcen, KI-Konfigurationen und Prüfhinweise")
            .row("Nicht enthalten", "Systembefund, Sessions, Einstellungen, Freigaben, Inhalte von Dateien")
            .action(format!("export-confirm:{}", u8::from(include_paths)), "Ordner wählen und speichern", true)
            .body(if include_paths {
                "Enthält lokale Pfade und Projektnamen. Nur bewusst teilen. Eine vorhandene Datei wird nie überschrieben."
            } else {
                "Kein Anonymitätsversprechen: Auch Paketnamen können etwas über deine Arbeit verraten. Eine vorhandene Datei wird nie überschrieben."
            });
        self.dialog(dialog);
    }

    pub(super) fn export_to_folder(&mut self, include_paths: bool) {
        self.close_modal();
        if let Some(folder) = self.pick_folder("Ordner für den Export wählen") {
            let path = format!("{}\\mogumogu-export-{}.json", folder.trim_end_matches('\\'), stamp());
            self.submit(Request::ExportTo { path, include_paths });
        }
    }
}

fn folder_name(path: &str) -> String {
    path.trim_end_matches('\\')
        .rsplit('\\')
        .next()
        .filter(|n| !n.is_empty() && !n.ends_with(':'))
        .unwrap_or("Projekt")
        .to_string()
}

fn stamp() -> String {
    iso_utc(SystemClock.now()).replace(['-', ':'], "")
}

fn guide_result(value: &Value) -> String {
    let Some(report) = value.as_array().and_then(|r| r.first()) else {
        return "Erfasst.".into();
    };
    if let Some(problem) = report["problem"].as_str() {
        return format!("Nicht vollständig: {problem}");
    }
    match report["state"].as_str() {
        Some("complete") => {
            "Vollständig erfasst. Die Vergleichsbasis steht; Hinweise brauchen Beobachtungszeit.".into()
        }
        Some("partial") => "Teilweise erfasst (Limit oder Lesefehler). Details unter Aktivität.".into(),
        _ => "Erfassung beendet. Details unter Aktivität.".into(),
    }
}
