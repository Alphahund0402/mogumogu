//! Presentation of drive metadata and the tidy-up report from the owner.
use super::ui::*;
use super::view::model;
use mogumogu::clock::{Clock, SystemClock, display_local, display_relative};
use mogumogu::domain::{DriveUsage, ScopeStatus, StorageOverview, TidyClass, TidyReport, format_bytes};
use slint::ComponentHandle;

fn size(value: Option<u64>) -> String {
    format_bytes(value.and_then(|bytes| i64::try_from(bytes).ok()))
}

fn fraction(part: Option<u64>, total: Option<u64>) -> f32 {
    match (part, total) {
        (Some(part), Some(total)) if total > 0 => (part as f64 / total as f64).clamp(0.0, 1.0) as f32,
        _ => 0.0,
    }
}

/// Where the names-only search of a drive stands, in one short line.
fn search_text(drive: &DriveUsage, now: i64) -> String {
    let Some(search) = &drive.search else {
        return if drive.searchable { "Noch nicht nach Projekten durchsucht".into() } else { String::new() };
    };
    let found = |n: i64| if n == 1 { "1 Fund".to_string() } else { format!("{n} Funde") };
    match (search.status, search.findings, search.checked_at) {
        (ScopeStatus::IdentityChanged, ..) => "Suche blockiert: Laufwerk ersetzt".into(),
        (ScopeStatus::Unavailable, ..) => "Suche: Laufwerk nicht erreichbar".into(),
        (_, findings, _) if search.partial => match findings {
            Some(n) => format!("Letzte Suche unvollständig · {} aus früherem Lauf", found(n)),
            None => "Letzte Suche unvollständig · Details unter Lesefreigaben".into(),
        },
        (_, Some(n), Some(at)) => format!("{} · durchsucht {}", found(n), display_relative(at, now)),
        _ => "Suche freigegeben, noch nicht durchsucht".into(),
    }
}

pub(super) fn render(ui: &Dashboard, report: &StorageOverview) {
    let now = SystemClock.now();
    let store = ui.global::<Store>();
    let selected = store.get_selected_drive().to_string();
    store.set_disk_checked(
        report
            .checked_at
            .map_or_else(|| "Beispieldaten".into(), |at| format!("Laufwerksstand: {}", display_local(at)))
            .into(),
    );
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
                search: search_text(drive, now).into(),
                search_id: drive.search.as_ref().map_or(-1, |s| s.scope_id as i32),
                searchable: drive.searchable,
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
}

/// A project untouched this long is worth a look with its outputs.
const DORMANT_DAYS: i64 = 180;

fn bytes(value: u64) -> String {
    size(Some(value))
}

pub(super) fn render_tidy(ui: &Dashboard, report: &TidyReport) {
    let store = ui.global::<Store>();
    let now = SystemClock.now();
    store.set_tidy_summary(
        match report.checked_at {
            None => "Noch kein Aufräumbericht.".to_string(),
            Some(_) => format!(
                "Wiederherstellbar {} · Prüfen {} · {} Projekt{} gemessen",
                bytes(report.regenerable()),
                bytes(report.review()),
                report.projects.len(),
                if report.projects.len() == 1 { "" } else { "e" }
            ),
        }
        .into(),
    );
    store.set_tidy_checked(
        report.checked_at.map_or_else(String::new, |at| format!("Stand: {}", display_local(at))).into(),
    );
    store.set_tidy_unapproved(
        match report.unapproved {
            0 => String::new(),
            1 => "1 Projekt ohne Lesefreigabe – dort misst mogumogu nichts.".into(),
            n => format!("{n} Projekte ohne Lesefreigabe – dort misst mogumogu nichts."),
        }
        .into(),
    );
    store.set_tidy_projects(model(
        report
            .projects
            .iter()
            .map(|p| TidyProjectRow {
                id: p.project_id as i32,
                name: p.name.clone().into(),
                path: p.path.clone().into(),
                total: p.total.map_or_else(|| "Unvollständig".to_string(), bytes).into(),
                regenerable: bytes(p.regenerable).into(),
                review: bytes(p.review).into(),
                vcs: bytes(p.vcs).into(),
                test_data: if p.test_data > 0 { bytes(p.test_data) } else { String::new() }.into(),
                test_heavy: p.heavy_test_data(),
                share: p.total.filter(|t| *t > 0).map_or(0.0, |t| (p.regenerable as f64 / t as f64) as f32),
                last_change: p
                    .last_change
                    .map_or_else(String::new, |t| format!("zuletzt geändert {}", display_relative(t, now)))
                    .into(),
                dormant: p.last_change.is_some_and(|t| now - t > DORMANT_DAYS * 86_400) && p.regenerable > 0,
                note: p.note.clone().into(),
            })
            .collect(),
    ));
    store.set_tidy_stores(model(
        report
            .stores
            .iter()
            .map(|s| TidyStoreRow {
                name: s.label.clone().into(),
                path: s.path.clone().into(),
                size: size(s.bytes).into(),
                regenerable: s.class == TidyClass::Regenerable,
                evidence: s.evidence.clone().into(),
            })
            .collect(),
    ));
}
