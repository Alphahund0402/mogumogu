//! Presentation of drive metadata and bounded directory listings from the owner.
use super::ui::*;
use super::view::model;
use mogumogu::clock::display_local;
use mogumogu::domain::{FolderTree, StorageOverview, format_bytes};
use slint::ComponentHandle;

impl super::Desktop {
    pub(super) fn request_tree(&mut self, scope_id: i64, rel_path: String) {
        if self.folder_pending.is_some() {
            return;
        }
        let request = mogumogu::service::Request::FolderTree { scope_id, rel_path: rel_path.clone() };
        if !self.owner.submit(request) {
            self.status = "Der Kern ist ausgelastet; bitte gleich erneut versuchen.".into();
            self.render_status();
            return;
        }
        self.folder_pending = Some((scope_id, rel_path));
        if let Some(ui) = &self.dashboard {
            ui.global::<Store>().set_tree_loading(true);
            ui.global::<Store>().set_tree_note("Ordnerstruktur wird gelesen …".into());
        }
    }
}

fn size(value: Option<u64>) -> String {
    format_bytes(value.and_then(|bytes| i64::try_from(bytes).ok()))
}

fn fraction(part: Option<u64>, total: Option<u64>) -> f32 {
    match (part, total) {
        (Some(part), Some(total)) if total > 0 => (part as f64 / total as f64).clamp(0.0, 1.0) as f32,
        _ => 0.0,
    }
}

pub(super) fn render(ui: &Dashboard, report: &StorageOverview) {
    let store = ui.global::<Store>();
    let selected = store.get_selected_drive().to_string();
    store.set_disk_checked(
        report
            .checked_at
            .map_or_else(|| "Beispieldaten".into(), |at| format!("Laufwerksstand: {}", display_local(at)))
            .into(),
    );
    store.set_disk_note(report.note.clone().into());
    store.set_drives(model(
        report
            .drives
            .iter()
            .map(|drive| DriveRow {
                root: drive.root.clone().into(),
                label: drive.label.clone().into(),
                kind: drive.kind.clone().into(),
                total: size(drive.total_bytes).into(),
                used: size(drive.used_bytes).into(),
                free: size(drive.free_bytes).into(),
                projects: size(drive.project_bytes).into(),
                usage: fraction(drive.used_bytes, drive.total_bytes),
                project_share: fraction(drive.project_bytes, drive.total_bytes),
                note: drive.status.clone().into(),
            })
            .collect(),
    ));
    store.set_disk_projects(model(
        report
            .projects
            .iter()
            .filter(|project| selected.is_empty() || project.drive.eq_ignore_ascii_case(&selected))
            .map(|project| DiskProjectRow {
                id: project.project_id as i32,
                name: project.name.clone().into(),
                path: project.path.clone().into(),
                drive: project.drive.clone().into(),
                size: size(project.bytes).into(),
                share: project
                    .share
                    .map_or_else(
                        || "Anteil unbekannt".into(),
                        |share| format!("{share:.2} % des Laufwerks").replace('.', ","),
                    )
                    .into(),
                fraction: (project.share.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0) as f32,
                scope_id: project.scope_id.map_or(-1, |id| id as i32),
                note: project.note.clone().into(),
            })
            .collect(),
    ));
    store.set_tree_scopes(model(
        report
            .scopes
            .iter()
            .filter(|scope| {
                selected.is_empty() || scope.path.to_ascii_lowercase().starts_with(&selected.to_ascii_lowercase())
            })
            .map(|scope| TreeScopeRow {
                id: scope.scope_id as i32,
                label: scope.label.clone().into(),
                path: scope.path.clone().into(),
            })
            .collect(),
    ));
}

pub(super) fn render_tree(ui: &Dashboard, tree: &FolderTree, root: &str) {
    let store = ui.global::<Store>();
    store.set_tree_scope_id(tree.scope_id as i32);
    store.set_tree_path(
        if tree.rel_path.is_empty() {
            root.to_string()
        } else {
            format!("{root}\\{}", tree.rel_path.replace('/', "\\"))
        }
        .into(),
    );
    store.set_tree_up(!tree.rel_path.is_empty());
    store.set_tree_note(tree.note.clone().into());
    store.set_tree_rows(model(
        tree.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| TreeRow {
                index: index as i32,
                name: entry.name.clone().into(),
                path: entry.rel_path.clone().into(),
                size: size(entry.bytes).into(),
                is_dir: entry.is_dir,
                note: entry.status.clone().into(),
            })
            .collect(),
    ));
}

pub(super) fn clear_tree(ui: &Dashboard, note: &str) {
    let store = ui.global::<Store>();
    store.set_tree_rows(model(Vec::new()));
    store.set_tree_scope_id(-1);
    store.set_tree_path("".into());
    store.set_tree_up(false);
    store.set_tree_note(note.into());
}
