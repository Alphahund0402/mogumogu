//! Modal presentation and form validation; all work is submitted to the owner.
use super::dialogs::Dialog;
use super::view::model;
use super::{Desktop, Form, dialogs, ui::*};
use mogumogu::service::Request;
use slint::ComponentHandle;

impl Desktop {
    // ------------------------------------------------------------ dialogs

    pub(super) fn dialog(&mut self, dialog: Dialog) {
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
        self.form = form;
        store.set_modal(2);
        ui.invoke_focus_dialog();
    }

    pub(super) fn close_modal(&mut self) {
        if let Some(ui) = &self.dashboard {
            ui.global::<Store>().set_modal(0);
        }
        self.form = Form::None;
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

    pub(super) fn new_project(&mut self) {
        if self.snapshot.demo {
            self.submit(Request::RegisterProject { name: String::new(), path: String::new() });
            return;
        }
        self.form(
            Form::Project,
            "Projekt registrieren",
            "Nur Name und Pfad werden gespeichert. Der Ordner wird nicht gelesen, erstellt oder verändert; die Lesefreigabe ist ein eigener Schritt.",
            [
                ("Projektname", "Zum Beispiel: api-service", String::new()),
                ("Lokaler Windows-Pfad", "C:\\dev\\api-service", String::new()),
            ],
            "Geschützt registrieren",
        );
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
        let project = project_id.or_else(|| self.snapshot.projects.first().map(|p| p.id));
        self.form(
            Form::Scratch,
            "Scratchpad anlegen",
            "mogumogu legt unter der Scratchpad-Wurzel source, environment, temporary und results an. Quelle und Ergebnisse bleiben immer erhalten.",
            [
                ("Name", "Zum Beispiel: parser-spike", String::new()),
                ("Projekt-Nr.", "1", project.map(|p| p.to_string()).unwrap_or_default()),
            ],
            "Anlegen",
        );
    }

    pub(super) fn submit_form(&mut self, first: String, second: String) {
        if self.busy {
            return;
        }
        match self.form {
            Form::Project => self.submit(Request::RegisterProject { name: first, path: second }),
            Form::Scratch => match second.trim().parse::<i64>().ok().filter(|id| *id > 0) {
                Some(project_id) => self.submit(Request::ScratchCreate {
                    project_id,
                    name: first,
                    purpose: "Über das Dashboard angelegt".into(),
                    review_days: None,
                }),
                None => {
                    if let Some(ui) = &self.dashboard {
                        ui.global::<Store>().set_form_error("Projekt-Nr. muss eine positive ganze Zahl sein.".into());
                    }
                }
            },
            Form::None => {}
        }
        self.render_status();
    }
}
