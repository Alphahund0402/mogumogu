//! Only synthetic folders under a temporary root. The storage overview uses
//! volume metadata and recorded sizes; it never walks a project tree.
#![cfg(windows)]
mod common;

use mogumogu::clock::FixedClock;
use mogumogu::config::Config;
use mogumogu::domain::{ScopeKind, StorageOverview};
use mogumogu::service::Core;
use mogumogu::{Error, platform};
use std::sync::Arc;

fn core_at(root: &std::path::Path, demo: bool) -> Core {
    Core::open_with(
        Config::new(Some(root.join(if demo { "demo" } else { "data" })), demo).unwrap(),
        Arc::new(FixedClock::new(1_800_000_000)),
    )
    .unwrap()
}

#[test]
fn drive_metadata_is_bounded_and_unknown_capacity_is_not_zero() {
    let drives = platform::logical_drives().unwrap();
    assert!(drives.len() <= 26);
    for drive in drives {
        assert_eq!(drive.root.len(), 3);
        assert!(drive.root.ends_with(":\\"));
        match (drive.total_bytes, drive.free_bytes, drive.used_bytes) {
            (Some(total), Some(free), Some(used)) => {
                assert!(free <= total);
                assert_eq!(used, total - free);
            }
            (None, None, None) => {}
            _ => panic!("volume capacity must be complete or unknown"),
        }
        if matches!(drive.kind.as_str(), "Netzlaufwerk" | "Optisches Laufwerk" | "Wechseldatenträger") {
            assert!(drive.total_bytes.is_none());
        }
    }
}

#[test]
fn storage_uses_recorded_sizes_without_reading_unapproved_projects() {
    let temp = tempfile::tempdir().unwrap();
    let mut core = core_at(temp.path(), false);
    let parent = core.register_project("parent", r"Q:\synthetic\project").unwrap();
    let nested = core.register_project("nested", r"Q:\synthetic\project\tool").unwrap();
    let duplicate = core.register_project("alias", r"Q:\SYNTHETIC\PROJECT").unwrap();
    let unknown = core.register_project("unknown", r"Q:\synthetic\another").unwrap();
    core.repository().record_project_size(parent, Some(1_000)).unwrap();
    core.repository().record_project_size(nested, Some(600)).unwrap();
    core.repository().record_project_size(duplicate, Some(1_000)).unwrap();
    let overview = core.storage_overview().unwrap();
    assert!(!overview.projects.iter().find(|p| p.project_id == nested).unwrap().included);
    assert!(!overview.projects.iter().find(|p| p.project_id == duplicate).unwrap().included);
    assert_eq!(drive(&overview, r"Q:\").project_bytes, None, "one unknown project makes the sum unknown");
    core.repository().record_project_size(unknown, Some(200)).unwrap();
    let measured = core.storage_overview().unwrap();
    assert_eq!(drive(&measured, r"Q:\").project_bytes, Some(1_200), "nested project is counted once");
    assert!(measured.projects.iter().all(|p| p.scope_id.is_none()));
    assert!(measured.note.contains("Hardlinks"));
}

fn drive<'a>(overview: &'a StorageOverview, root: &str) -> &'a mogumogu::domain::DriveUsage {
    overview.drives.iter().find(|d| d.root == root).unwrap()
}

#[test]
fn demo_drives_are_synthetic() {
    let temp = tempfile::tempdir().unwrap();
    let mut core = core_at(temp.path(), true);
    let overview = core.storage_overview().unwrap();
    assert_eq!(overview.drives.len(), 2);
    assert!(overview.drives.iter().all(|d| d.kind == "Beispiellaufwerk"));
    assert!(overview.projects.iter().all(|p| p.scope_id.is_none()), "demo projects grant no reading");
}

#[test]
fn only_a_names_only_search_may_cover_a_whole_local_drive() {
    let temp = tempfile::tempdir().unwrap();
    let mut core = core_at(temp.path(), false);
    for kind in [ScopeKind::CargoTools, ScopeKind::NpmGlobal, ScopeKind::Python] {
        assert!(matches!(core.approve_system(kind, Some("C:\\")), Err(Error::Invalid(_))), "{kind:?}");
    }
    let used: Vec<String> = platform::logical_drives().unwrap().into_iter().map(|d| d.root).collect();
    let absent = ('D'..='Z').map(|c| format!("{c}:\\")).find(|root| !used.contains(root)).unwrap();
    let refused = core.approve_system(ScopeKind::ProjectSearch, Some(&absent)).unwrap_err();
    assert!(refused.to_string().contains("lokale Festplatten"), "{refused}");
    assert!(core.repository().scopes().unwrap().is_empty());
}

#[test]
fn a_drive_search_shows_on_its_drive_and_is_never_measured() {
    let temp = tempfile::tempdir().unwrap();
    let mut core = core_at(temp.path(), false);
    let Some(local) = core.storage_overview().unwrap().drives.into_iter().find(|d| d.searchable) else { return };
    // A metadata record only: no identity is taken and nothing is scanned.
    let scope = core.repository().approve_scope(ScopeKind::ProjectSearch, None, &local.root, None).unwrap();
    let overview = core.storage_overview().unwrap();
    let search = drive(&overview, &local.root).search.clone().unwrap();
    assert_eq!(search.scope_id, scope);
    assert!(search.findings.is_none() && search.checked_at.is_none() && !search.partial);
    let refused = core.measure_scope(scope).unwrap_err();
    assert!(refused.to_string().contains("ganzes Laufwerk"), "{refused}");
}
