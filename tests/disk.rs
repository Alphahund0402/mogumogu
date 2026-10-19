//! Only synthetic folders under a temporary root. Storage browsing never
//! reads file content or traverses a link to an unapproved directory.
#![cfg(windows)]
mod common;

use common::{junction, path_text, write};
use mogumogu::clock::FixedClock;
use mogumogu::config::Config;
use mogumogu::domain::{ScopeKind, StorageOverview};
use mogumogu::service::Core;
use mogumogu::updates::NoNetwork;
use mogumogu::{Error, platform};
use std::sync::Arc;

fn core_at(root: &std::path::Path, demo: bool) -> Core {
    Core::open_with(
        Config::new(Some(root.join(if demo { "demo" } else { "data" })), demo).unwrap(),
        Arc::new(FixedClock::new(1_800_000_000)),
        Box::new(NoNetwork),
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
    assert!(overview.scopes.is_empty(), "registration grants no file browser");
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
fn project_folder_sizes_are_requested_and_links_stay_unknown() {
    let temp = tempfile::tempdir().unwrap();
    let project_path = temp.path().join("project");
    let outside = temp.path().join("outside");
    write(&project_path, "src/main.rs", "12345");
    write(&project_path, "src/helper.rs", "1234567");
    write(&outside, "outside-only.txt", "outside");
    std::fs::create_dir_all(project_path.join("empty")).unwrap();
    junction(&project_path.join("linked"), &outside);
    let mut core = core_at(temp.path(), false);
    let project = core.register_project("project", &path_text(&project_path)).unwrap();
    let scope = core.approve_project(project).unwrap();
    let tree = core.folder_tree(scope, "").unwrap();
    assert_eq!(tree.entries.iter().find(|e| e.name == "src").unwrap().bytes, Some(12));
    assert_eq!(tree.entries.iter().find(|e| e.name == "empty").unwrap().bytes, Some(0));
    assert_eq!(tree.entries.iter().find(|e| e.name == "linked").unwrap().bytes, None);
    assert!(!tree.complete, "link contents stay outside coverage");
    assert!(matches!(core.folder_tree(scope, "linked"), Err(Error::Blocked(_))));
    assert!(!tree.entries.iter().any(|e| e.name == "outside-only.txt"));
    let inside = core.folder_tree(scope, "src").unwrap();
    assert!(inside.complete);
    assert_eq!(inside.entries.len(), 2);
    assert!(inside.entries.iter().all(|e| e.rel_path.starts_with("src/")));
    core.revoke_scope(scope).unwrap();
    assert!(matches!(core.folder_tree(scope, ""), Err(Error::Blocked(_))));
}

#[test]
fn search_grants_only_show_names_and_narrow_config_grants_cannot_browse() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("search");
    write(&folder, "test-data/sample.json", "{\"secret\":\"never opened\"}");
    write(&folder, "scratch.py", "print('never started')");
    let mut core = core_at(temp.path(), false);
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&folder))).unwrap();
    let tree = core.folder_tree(scope, "").unwrap();
    assert!(tree.entries.iter().all(|e| e.bytes.is_none()));
    assert!(tree.note.contains("nur Namen"));
    assert!(core.folder_tree(scope, "test-data").unwrap().entries.iter().all(|e| e.bytes.is_none()));
    let narrow = core
        .repository()
        .approve_scope(ScopeKind::UserAi, None, &path_text(&folder), Some(platform::path_identity(&folder).unwrap()))
        .unwrap();
    assert!(matches!(core.folder_tree(narrow, ""), Err(Error::Blocked(_))));
}

#[test]
fn replaced_roots_and_path_escape_attempts_never_expose_names() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("project");
    write(&folder, "original.txt", "original");
    let mut core = core_at(temp.path(), false);
    let project = core.register_project("project", &path_text(&folder)).unwrap();
    let scope = core.approve_project(project).unwrap();
    for rel in ["../outside", "src/../outside", r"src\outside", "C:/outside", "name:stream", "/outside"] {
        assert!(matches!(core.folder_tree(scope, rel), Err(Error::Invalid(_))), "{rel}");
    }
    std::fs::rename(&folder, temp.path().join("original")).unwrap();
    write(&folder, "replacement-secret.txt", "replacement");
    assert!(matches!(core.folder_tree(scope, ""), Err(Error::Blocked(_))));
}

#[test]
fn folder_views_are_bounded_and_do_not_report_hidden_entries_as_complete() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("large");
    for index in 0..205 {
        write(&folder, &format!("file-{index:04}.txt"), "x");
    }
    let mut core = core_at(temp.path(), false);
    let project = core.register_project("large", &path_text(&folder)).unwrap();
    let scope = core.approve_project(project).unwrap();
    let tree = core.folder_tree(scope, "").unwrap();
    assert_eq!(tree.entries.len(), 200);
    assert!(!tree.complete);
    assert!(tree.note.starts_with("Teilweise Ansicht"));
}

#[test]
fn demo_drives_and_folder_contents_are_synthetic() {
    let temp = tempfile::tempdir().unwrap();
    let mut core = core_at(temp.path(), true);
    let overview = core.storage_overview().unwrap();
    assert_eq!(overview.drives.len(), 2);
    assert!(overview.drives.iter().all(|d| d.kind == "Beispiellaufwerk"));
    assert_eq!(overview.scopes.len(), 6);
    let scope = overview.scopes.first().unwrap().scope_id;
    assert!(scope > 0);
    let tree = core.folder_tree(scope, "src").unwrap();
    assert!(tree.entries.iter().all(|e| e.status == "Synthetische Beispieldaten"));
    assert!(matches!(core.folder_tree(7, ""), Err(Error::NotFound(_))));
}

#[test]
fn only_a_names_only_search_may_cover_a_whole_local_drive() {
    let temp = tempfile::tempdir().unwrap();
    let mut core = core_at(temp.path(), false);
    for kind in [ScopeKind::Scoop, ScopeKind::NpmGlobal, ScopeKind::Python] {
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
