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
        store.set_form_project_picker(form == Form::Candidate);
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

    /// The project field is a picker; its choice is read from the selection.
    pub(super) fn submit_form(&mut self, _project: String, second: String) {
        if self.busy {
            return;
        }
        match self.form {
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

    /// A search folder is read by file names only; the core binds the
    /// approval to the chosen folder's identity.
    pub(super) fn approve_search_folder(&mut self) {
        if let Some(path) = self.pick_folder("Suchordner für mogumogu wählen") {
            self.submit(Request::ApproveSystem { kind: mogumogu::domain::ScopeKind::ProjectSearch, path: Some(path) });
        }
    }

    /// One click per local drive: the core approves a names-only search of
    /// the whole drive, bound to its identity. An approved drive is searched
    /// again; a replaced one needs this click as its new approval.
    pub(super) fn search_drive(&mut self, root: &str) {
        let search = self.snapshot.disks.drives.iter().find(|d| d.root == root).and_then(|d| d.search.as_ref());
        let request = match search {
            Some(search) if search.status != mogumogu::domain::ScopeStatus::IdentityChanged => {
                Request::Scan { scope_id: Some(search.scope_id) }
            }
            _ => Request::ApproveSystem { kind: mogumogu::domain::ScopeKind::ProjectSearch, path: Some(root.into()) },
        };
        self.submit(request);
    }

    /// Stores outside their usual place, chosen by kind in plain words.
    pub(super) fn add_store_dialog(&mut self) {
        let dialog = Dialog::new("Ort hinzufügen")
            .row("Python-Umgebung", "Interpreter- oder venv-Ordner mit Lib\\site-packages")
            .row("npm global", "Ordner node_modules der global installierten Pakete")
            .row("uv-Tools", "Ordner mit uv-Toolumgebungen, auch an älteren Orten")
            .row("Cargo", "Cargo-Installationsordner mit .crates2.json")
            .row("NuGet", "Paketordner, zum Beispiel %USERPROFILE%\\.nuget\\packages")
            .action("add-store:python", "Python-Umgebung …", true)
            .action("add-store:npm-global", "npm global …", false)
            .action("add-store:uv-tools", "uv-Tools …", false)
            .action("add-store:cargo-tools", "Cargo …", false)
            .action("add-store:nuget-cache", "NuGet …", false)
            .body("Wähle die Art und danach den Ordner. mogumogu liest dort nur Paketmetadaten und startet nichts.");
        self.dialog(dialog);
    }

    pub(super) fn add_store(&mut self, kind: &str) {
        self.close_modal();
        let Some(kind) = mogumogu::domain::ScopeKind::parse(kind) else { return };
        if let Some(path) = self.pick_folder("Ordner für mogumogu wählen") {
            self.submit(Request::ApproveSystem { kind, path: Some(path) });
        }
    }

    // ---------------------------------------------- adding a project in steps

    /// Opens "Projekt hinzufügen" at the folder step. Nothing opens on its own:
    /// the overview's empty state and the toolbar offer it.
    pub(super) fn guide_open(&mut self) {
        let Some(ui) = &self.dashboard else { return };
        let store = ui.global::<Store>();
        store.set_guide_step(1);
        store.set_guide_error("".into());
        store.set_guide_result("".into());
        store.set_search_open(false);
        store.set_guide_open(true);
        self.guide.project = None;
        self.guide.scope = None;
    }

    pub(super) fn guide_close(&mut self) {
        if let Some(ui) = &self.dashboard {
            ui.global::<Store>().set_guide_open(false);
            ui.invoke_dismiss_dialog();
        }
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
            // A name that came from the previous folder follows the new one;
            // a name the user typed stays.
            let name = store.get_guide_name().trim().to_string();
            let previous = store.get_guide_path().to_string();
            if name.is_empty() || name == folder_name(&previous) {
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

    // ------------------------------------------------------ data, folders

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

    pub(super) fn backup_now(&mut self) {
        if let Some(folder) = self.pick_folder("Ordner für die Sicherung wählen") {
            let path = format!("{}\\mogumogu-sicherung-{}.sqlite3", folder.trim_end_matches('\\'), stamp());
            self.submit(Request::Backup { path });
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
        Some("partial") => {
            "Teilweise erfasst (Limit oder Lesefehler). Details unter Einstellungen › Lesefreigaben.".into()
        }
        _ => "Erfassung beendet. Details unter Einstellungen › Lesefreigaben.".into(),
    }
}
