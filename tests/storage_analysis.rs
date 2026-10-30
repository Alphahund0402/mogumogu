//! Synthetic disposable NTFS fixtures only. Never analyzes a real drive root.
#![cfg(windows)]
mod common;

use common::{junction, path_text, write};
use mogumogu::clock::FixedClock;
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::fsread::{ScanControl, ScopeRoot};
use mogumogu::inventory::disk::DiskIndex;
use mogumogu::limits::{DISK_MAX_PAGE, IPC_MAX_MESSAGE, ScanLimits};
use mogumogu::service::{Core, Request};
use mogumogu::{Error, platform};
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn core_at(path: &Path) -> Core {
    Core::open_with(Config::new(Some(path.join("data")), false).unwrap(), Arc::new(FixedClock::new(1_800_000_000)))
        .unwrap()
}

fn fixture(path: &Path) {
    write(path, "a/large.ZIP", &"z".repeat(100));
    write(path, "a/same.bin", &"a".repeat(25));
    write(path, "b/same.bin", &"b".repeat(25));
    write(path, "b/unique.rs", &"r".repeat(10));
    fs::create_dir_all(path.join("empty")).unwrap();
}

fn index_at(path: &Path, limits: ScanLimits, control: Option<Arc<ScanControl>>) -> DiskIndex {
    DiskIndex::scan(
        1,
        &path_text(path),
        ScopeRoot::open_local(path, Some(platform::path_identity(path).unwrap())).unwrap(),
        1_800_000_000,
        limits,
        control,
    )
}

#[test]
fn analysis_has_tree_largest_files_types_search_and_metadata_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("folder");
    fixture(&root);
    let mut core = core_at(temp.path());
    let scope = core.approve_system(ScopeKind::Storage, Some(&path_text(&root))).unwrap();
    assert!(core.scan_all().unwrap().is_empty(), "analysis is never an automatic inventory scan");
    assert!(core.scan(scope).is_err());
    let first = core.analyze_storage(scope).unwrap();
    assert!(first.summary.complete);
    assert_eq!(first.summary.total_bytes, Some(160));
    assert_eq!((first.summary.files, first.summary.folders), (4, 3));
    assert_eq!(first.rows.iter().map(|r| r.bytes).sum::<u64>(), 160);
    let a = first.rows.iter().find(|r| r.name == "a").unwrap();
    assert_eq!((a.bytes, a.files), (125, 2));
    let child = core.storage_page(scope, &DiskQuery { folder: a.id, ..Default::default() }).unwrap();
    assert_eq!(child.parent, Some(0));
    assert_eq!(child.rows[0].name, "large.ZIP");
    let largest = core.storage_page(scope, &DiskQuery { view: DiskView::Files, ..Default::default() }).unwrap();
    assert_eq!(largest.total, 4);
    assert_eq!(largest.rows[0].bytes, 100);
    assert!(largest.rows[0].path.ends_with("large.ZIP"));
    let types = core.storage_page(scope, &DiskQuery { view: DiskView::Types, ..Default::default() }).unwrap();
    let bins = types.rows.iter().find(|r| r.name == ".bin").unwrap();
    assert_eq!((bins.bytes, bins.files), (50, 2));
    assert!(types.rows.iter().any(|r| r.name == ".zip"));
    let filtered = core
        .storage_page(scope, &DiskQuery { view: DiskView::Files, text: "*.BIN".into(), ..Default::default() })
        .unwrap();
    assert_eq!(filtered.total, 2);
    let duplicates = core.storage_page(scope, &DiskQuery { view: DiskView::Duplicates, ..Default::default() }).unwrap();
    assert_eq!(duplicates.total, 2);
    assert!(duplicates.rows.iter().all(|r| r.duplicate_candidates == 2));
    assert_ne!(
        fs::read(root.join("a/same.bin")).unwrap(),
        fs::read(root.join("b/same.bin")).unwrap(),
        "candidates do not claim content equality"
    );
    assert!(core.storage_page(scope, &DiskQuery { folder: largest.rows[0].id, ..Default::default() }).is_err());
    assert!(core.storage_page(scope, &DiskQuery { text: "x".repeat(201), ..Default::default() }).is_err());
}

#[test]
fn allocated_space_is_aggregated_and_sortable_beside_the_logical_size() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("folder");
    fixture(&root);
    // Large enough to leave the MFT record: allocation is whole clusters.
    write(&root, "b/clustered.dat", &"c".repeat(5_000));
    let index = index_at(&root, ScanLimits::storage_analysis(), None);
    let page =
        index.page(&DiskQuery { view: DiskView::Files, sort: DiskSort::Allocated, ..Default::default() }).unwrap();
    let clustered = &page.rows[0];
    assert_eq!(clustered.name, "clustered.dat", "tiny files may live in the MFT and allocate nothing");
    assert!(clustered.allocated >= clustered.bytes && clustered.allocated.is_multiple_of(512));
    assert_eq!(page.summary.allocated_bytes, page.rows.iter().map(|r| r.allocated).sum::<u64>());
    let tree = index.page(&DiskQuery::default()).unwrap();
    assert_eq!(tree.folder_bytes, 5_160);
    let b = tree.rows.iter().find(|r| r.name == "b").unwrap();
    assert!(b.allocated >= clustered.allocated, "folders sum the allocation of their files");
    assert!(page.csv().contains(&format!(",{},{},1,0,", clustered.bytes, clustered.allocated)));
}

#[test]
fn pages_are_cached_bounded_and_have_a_treemap_remainder() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("folder");
    for n in 0..221 {
        write(&root, &format!("file-{n:03}.bin"), "123");
    }
    let index = index_at(&root, ScanLimits::storage_analysis(), None);
    let first = index.page(&DiskQuery { limit: usize::MAX, ..Default::default() }).unwrap();
    assert_eq!(first.rows.len(), DISK_MAX_PAGE);
    assert_eq!(first.total, 221);
    assert!(first.tiles.iter().any(|t| t.remainder));
    assert_eq!(first.tiles.iter().map(|t| t.bytes).sum::<u64>(), 221 * 3);
    let next = index.page(&DiskQuery { offset: 200, limit: 200, ..Default::default() }).unwrap();
    assert_eq!(next.rows.len(), 21);
    assert!(first.rows.iter().all(|a| next.rows.iter().all(|b| a.id != b.id)));
    let names: Vec<_> = first.rows.iter().chain(&next.rows).map(|r| r.name.as_str()).collect();
    assert!(names.is_sorted() && names.len() == 221, "pages continue one order");
    write(&root, "new.bin", "changed after the scan");
    assert_eq!(index.page(&DiskQuery::default()).unwrap().total, 221, "paging never rescans");
    assert!(serde_json::to_vec(&first).unwrap().len() < IPC_MAX_MESSAGE / 2);
}

#[test]
fn a_wide_directory_retains_its_bounded_partial_sample() {
    let temp = tempfile::tempdir().unwrap();
    for n in 0..10 {
        write(temp.path(), &format!("{n}.bin"), "123");
    }
    let index = index_at(temp.path(), ScanLimits { max_entries: 3, ..ScanLimits::storage_analysis() }, None);
    let page = index.page(&DiskQuery::default()).unwrap();
    assert!(!page.summary.complete);
    assert_eq!(page.summary.total_bytes, None);
    assert_eq!(page.summary.observed_bytes, 9);
    assert_eq!(page.rows.len(), 3);
    assert!(page.summary.reason.contains("Eintragslimit"));
}

#[test]
fn cancellation_depth_and_links_never_report_a_complete_total() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("folder");
    fixture(&root);
    let outside = temp.path().join("outside");
    write(&outside, "outside-secret.bin", "outside");
    junction(&root.join("link"), &outside);
    let index = index_at(&root, ScanLimits::storage_analysis(), None);
    let page = index.page(&DiskQuery { view: DiskView::Files, ..Default::default() }).unwrap();
    assert_eq!(page.summary.total_bytes, None);
    assert_eq!(page.summary.skipped, 1);
    assert!(!page.rows.iter().any(|r| r.name.contains("outside")));
    let shallow = index_at(&root, ScanLimits { max_depth: 0, ..ScanLimits::storage_analysis() }, None);
    assert!(shallow.summary.reason.contains("Tiefenlimit"));
    assert!(shallow.summary.total_bytes.is_none());
    let control = Arc::new(ScanControl::default());
    control.begin(1);
    control.cancel();
    let cancelled = index_at(&root, ScanLimits::storage_analysis(), Some(control));
    assert!(!cancelled.summary.complete);
    assert_eq!(cancelled.summary.total_bytes, None);
    assert_eq!(cancelled.summary.reason, mogumogu::fsread::CANCELLED);
}

#[test]
fn unread_folders_stay_unknown_and_a_stop_outranks_a_skipped_link() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("folder");
    fixture(&root);
    write(&root, "a/deep/hidden.bin", &"h".repeat(40));
    let outside = temp.path().join("outside");
    write(&outside, "o.bin", "o");
    junction(&root.join("link"), &outside);

    // The root listing meets the junction first; the limit ends the walk later.
    let limited = index_at(&root, ScanLimits { max_entries: 6, ..ScanLimits::storage_analysis() }, None);
    assert_eq!(limited.summary.reason, "Eintragslimit erreicht");
    assert!(!limited.page(&DiskQuery::default()).unwrap().folder_complete);

    let shallow = index_at(&root, ScanLimits { max_depth: 1, ..ScanLimits::storage_analysis() }, None);
    let tree = shallow.page(&DiskQuery::default()).unwrap();
    let row = |name: &str| tree.rows.iter().find(|r| r.name == name).unwrap().clone();
    let (a, b) = (row("a"), row("b"));
    assert!(!a.complete && b.complete, "only the folder above the unread one is a lower bound");
    assert_eq!(a.bytes, 125, "what was read still counts");
    assert!(!tree.folder_complete && tree.csv().contains(",false,false\r\n"));
    let inside = shallow.page(&DiskQuery { folder: a.id, ..Default::default() }).unwrap();
    let deep = inside.rows.iter().find(|r| r.name == "deep").unwrap();
    assert!(deep.directory && !deep.complete && deep.bytes == 0);
    assert!(inside.rows.iter().filter(|r| !r.directory).all(|r| r.complete));
    let unread = shallow.page(&DiskQuery { folder: deep.id, ..Default::default() }).unwrap();
    assert!(unread.rows.is_empty() && !unread.folder_complete, "unknown, not empty");
    let full = index_at(&root, ScanLimits::storage_analysis(), None);
    assert!(full.page(&DiskQuery::default()).unwrap().rows.iter().all(|r| r.complete), "a skipped link is no gap");
}

#[test]
fn an_intermediate_junction_is_refused_before_opening_its_child() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("child")).unwrap();
    junction(&temp.path().join("alias"), &target);
    let aliased = temp.path().join("alias/child");
    assert!(ScopeRoot::open_local(&aliased, None).is_err());
    let mut core = core_at(temp.path());
    assert!(core.approve_system(ScopeKind::Storage, Some(&path_text(&aliased))).is_err());
    assert!(core.repository().scopes().unwrap().is_empty());
    assert!(ScopeRoot::open_local(Path::new(r"\\unreachable\share\folder"), None).is_err());
}

#[test]
fn reading_needs_an_appropriate_current_identity_bound_approval() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("folder");
    fixture(&root);
    let mut core = core_at(temp.path());
    let path = path_text(&root);
    for kind in [ScopeKind::ProjectSearch, ScopeKind::NpmCache, ScopeKind::UserAi] {
        let scope =
            core.repository().approve_scope(kind, None, &path, Some(platform::path_identity(&root).unwrap())).unwrap();
        assert!(matches!(core.analyze_storage(scope), Err(Error::Blocked(_))));
    }
    let scope = core.approve_system(ScopeKind::Storage, Some(&path)).unwrap();
    core.analyze_storage(scope).unwrap();
    core.revoke_scope(scope).unwrap();
    assert!(core.storage_page(scope, &DiskQuery::default()).is_err());
    assert!(core.analyze_storage(scope).is_err());
    let current = core.approve_system(ScopeKind::Storage, Some(&path)).unwrap();
    fs::rename(&root, temp.path().join("old")).unwrap();
    fixture(&root);
    assert!(core.analyze_storage(current).is_err());
    assert_eq!(core.repository().scope(current).unwrap().status, ScopeStatus::IdentityChanged);
    assert!(core.storage_page(current, &DiskQuery::default()).is_err());
}

#[test]
fn an_unreachable_folder_is_retried_and_a_refusal_keeps_the_project_status() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("folder");
    fixture(&root);
    let mut core = core_at(temp.path());
    let scope = core.approve_system(ScopeKind::Storage, Some(&path_text(&root))).unwrap();
    let away = temp.path().join("away");
    fs::rename(&root, &away).unwrap();
    assert!(matches!(core.analyze_storage(scope), Err(Error::Blocked(_))));
    assert_eq!(core.repository().scope(scope).unwrap().status, ScopeStatus::Unavailable);
    fs::rename(&away, &root).unwrap();
    assert!(core.analyze_storage(scope).unwrap().summary.complete, "same folder back: analysis retries");
    assert_eq!(core.repository().scope(scope).unwrap().status, ScopeStatus::Approved);

    // A project reached through a junction scans normally, but the
    // ancestor-by-ancestor analysis refuses it without touching its approval.
    let target = temp.path().join("target");
    fixture(&target.join("app"));
    junction(&temp.path().join("alias"), &target);
    let project = core.register_project("app", &path_text(&temp.path().join("alias/app"))).unwrap();
    let project_scope = core.approve_project(project).unwrap();
    assert!(matches!(core.analyze_storage(project_scope), Err(Error::Blocked(_))));
    let after = core.repository().scope(project_scope).unwrap();
    assert_eq!((after.status, after.last_error), (ScopeStatus::Approved, None));

    // Reachability of a project belongs to its inventory: it must stay in "scan all".
    let plain = temp.path().join("plain");
    fixture(&plain);
    let plain_project = core.register_project("plain", &path_text(&plain)).unwrap();
    let plain_scope = core.approve_project(plain_project).unwrap();
    fs::rename(&plain, temp.path().join("plain-away")).unwrap();
    assert!(matches!(core.analyze_storage(plain_scope), Err(Error::Blocked(_))));
    assert_eq!(core.repository().scope(plain_scope).unwrap().status, ScopeStatus::Approved);
}

#[test]
fn folder_numbers_belong_to_one_analysis_and_measuring_stays_with_it() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("folder");
    fixture(&root);
    let mut core = core_at(temp.path());
    let scope = core.approve_system(ScopeKind::Storage, Some(&path_text(&root))).unwrap();
    let first = core.analyze_storage(scope).unwrap();
    let second = core.analyze_storage(scope).unwrap();
    assert_ne!(first.analysis, second.analysis, "same second, still a new token");
    let stale = DiskQuery { analysis: first.analysis, folder: 1, ..Default::default() };
    assert!(matches!(core.storage_page(scope, &stale), Err(Error::Conflict(_))));
    assert!(core.storage_page(scope, &DiskQuery { analysis: second.analysis, ..Default::default() }).is_ok());
    assert!(core.storage_page(scope, &DiskQuery::default()).is_ok(), "zero accepts the current analysis");
    // The path-based measurement would not refuse a link on the way.
    assert!(matches!(core.measure_scope(scope), Err(Error::Blocked(_))));
}

#[test]
fn long_paths_and_redacted_names_fit_the_ipc_budget() {
    let temp = tempfile::tempdir().unwrap();
    let mut deep = temp.path().join("folder");
    let root = deep.clone();
    for _ in 0..20 {
        deep.push("long-folder-".repeat(12));
    }
    for n in 0..200 {
        write(&deep, &format!("{n:03}-name.rs"), "x");
    }
    let index = index_at(&root, ScanLimits::storage_analysis(), None);
    let page = index.page(&DiskQuery { view: DiskView::Files, limit: 200, ..Default::default() }).unwrap();
    assert_eq!(page.rows.len(), 200);
    assert!(page.rows.iter().all(|r| r.path.chars().count() <= 200));
    assert!(page.rows.iter().all(|r| r.path.starts_with('…') && r.path.ends_with(&r.name)), "the file name survives");
    assert!(serde_json::to_vec(&page).unwrap().len() < IPC_MAX_MESSAGE / 2);
}

#[test]
fn the_protocol_cannot_widen_an_analysis_request() {
    assert!(serde_json::from_str::<Request>(r#"{"cmd":"analyze_storage","scope_id":1,"path":"C:\\"}"#).is_err());
    // The CLI gets the result of a whole drive instead of a timeout.
    let analysis = Request::AnalyzeStorage { scope_id: 1 }.reply_within();
    assert!(analysis > ScanLimits::storage_analysis().time_budget);
    assert_eq!(Request::Status.reply_within(), mogumogu::limits::IPC_CLIENT_TIMEOUT);
    assert!(
        serde_json::from_str::<Request>(r#"{"cmd":"storage_page","scope_id":1,"query":{"max_entries":9999999}}"#)
            .is_err()
    );
    let temp = tempfile::tempdir().unwrap();
    let mut demo = Core::open_with(
        Config::new(Some(temp.path().join("demo")), true).unwrap(),
        Arc::new(FixedClock::new(1_800_000_000)),
    )
    .unwrap();
    assert!(demo.analyze_storage(1).is_err());
}
