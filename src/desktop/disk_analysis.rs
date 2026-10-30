//! Only presentation of one cached analysis page. No filesystem access.
use super::ui::*;
use super::view::model;
use mogumogu::clock::{Clock, SystemClock, display_local, display_relative};
use mogumogu::domain::{DiskPage, DiskView, Scope, format_bytes};
use slint::ComponentHandle;

fn size(bytes: u64) -> String {
    format_bytes(i64::try_from(bytes).ok())
}

/// A size of something not read in full is a lower bound, never a total.
fn bound(bytes: u64, complete: bool) -> String {
    match (complete, bytes) {
        (true, _) => size(bytes),
        (false, 0) => "nicht gelesen".into(),
        (false, _) => format!("mind. {}", size(bytes)),
    }
}

/// Logical share of the shown folder; the base of each row's bar.
fn share(bytes: u64, folder: u64) -> f32 {
    if folder == 0 { 0.0 } else { (bytes as f64 / folder as f64).clamp(0.0, 1.0) as f32 }
}

pub fn clear(ui: &Dashboard) {
    let store = ui.global::<Store>();
    store.set_analysis_ready(false);
    store.set_analysis_rows(model(Vec::new()));
    store.set_analysis_tiles(model(Vec::new()));
    store.set_analysis_search("".into());
    store.set_analysis_hover("".into());
}

pub(super) fn targets(ui: &Dashboard, scopes: &[Scope]) {
    let store = ui.global::<Store>();
    let scopes: Vec<_> = scopes.iter().filter(|s| s.analyzable()).collect();
    let old = i64::from(store.get_analysis_scope());
    let index = scopes.iter().position(|s| s.id == old).or_else(|| (!scopes.is_empty()).then_some(0));
    let selected = index.map_or(-1, |i| scopes[i].id as i32);
    store.set_analysis_scopes(model(scopes.iter().map(|s| s.path.clone().into()).collect()));
    store.set_analysis_target(index.map_or(-1, |i| i as i32));
    store.set_analysis_scope(selected);
    if old != i64::from(selected) {
        clear(ui);
    }
}

pub fn render(ui: &Dashboard, page: &DiskPage) {
    let store = ui.global::<Store>();
    let now = SystemClock.now();
    store.set_analysis_ready(true);
    store.set_analysis_complete(page.summary.complete);
    store.set_analysis_scope(page.scope_id as i32);
    store.set_analysis_path(
        if page.path.is_empty() { page.root.clone() } else { format!("{} / {}", page.root, page.path) }.into(),
    );
    store.set_analysis_has_parent(page.parent.is_some());
    store.set_analysis_folder_complete(page.folder_complete);
    store.set_analysis_view(match page.view {
        DiskView::Tree => 0,
        DiskView::Files => 1,
        DiskView::Types => 2,
        DiskView::Duplicates => 3,
    });
    store.set_analysis_summary(
        format!(
            "{}{} · {} auf Datenträger · {} Dateien · {} Ordner · {}",
            if page.summary.complete { "" } else { "Mindestens " },
            size(page.summary.observed_bytes),
            size(page.summary.allocated_bytes),
            page.summary.files,
            page.summary.folders,
            display_local(page.summary.checked_at)
        )
        .into(),
    );
    store.set_analysis_note(if page.summary.complete {
        "Logische Größe und belegter Platz auf dem Datenträger zum Analysezeitpunkt. Hardlinks zählen je Namen; Verwaltungsdaten des Dateisystems (MFT, Ordnerindizes) fehlen. Größe belegt keine Entbehrlichkeit.".into()
    } else { format!("Unvollständig: {} · {} ausgelassen · {} nicht lesbar. Sichtbare Größen sind Teilsummen; die Gesamtgröße bleibt unbekannt.", page.summary.reason, page.summary.skipped, page.summary.unreadable) }.into());
    store.set_analysis_rows(model(
        page.rows
            .iter()
            .map(|row| AnalysisRow {
                id: row.id as i32,
                name: row.name.clone().into(),
                path: row.path.clone().into(),
                directory: row.directory,
                size: bound(row.bytes, row.complete).into(),
                allocated: if row.complete || row.allocated > 0 {
                    format!("{} belegt", bound(row.allocated, row.complete))
                } else {
                    String::new()
                }
                .into(),
                share: share(row.bytes, page.folder_bytes),
                share_text: format!(
                    "{}{:.1} %",
                    if row.complete { "" } else { "≥ " },
                    share(row.bytes, page.folder_bytes) * 100.0
                )
                .replace('.', ",")
                .into(),
                counts: if row.directory {
                    format!("{} Dateien · {} Ordner", row.files, row.folders)
                } else if page.view == DiskView::Types {
                    format!("{} Dateien", row.files)
                } else {
                    String::new()
                }
                .into(),
                modified: if row.modified == 0 { String::new() } else { display_relative(row.modified, now) }.into(),
                duplicates: if row.duplicate_candidates > 1 {
                    format!("{} Kandidaten", row.duplicate_candidates)
                } else {
                    String::new()
                }
                .into(),
            })
            .collect(),
    ));
    store.set_analysis_tiles(model(
        page.tiles
            .iter()
            .map(|tile| AnalysisTile {
                id: tile.id as i32,
                label: tile.name.clone().into(),
                size: bound(tile.bytes, tile.complete).into(),
                directory: tile.directory,
                remainder: tile.remainder,
                left: tile.x,
                top: tile.y,
                wide: tile.width,
                tall: tile.height,
            })
            .collect(),
    ));
    store.set_analysis_page(
        if page.total == 0 {
            "Keine Treffer".into()
        } else {
            format!("{}–{} von {}", page.offset + 1, (page.offset + page.rows.len()).min(page.total), page.total)
        }
        .into(),
    );
    store.set_analysis_previous(page.offset > 0);
    store.set_analysis_next(page.offset + page.rows.len() < page.total);
    store.set_analysis_hover("".into());
}
