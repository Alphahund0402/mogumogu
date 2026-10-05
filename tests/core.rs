//! Core contracts without UI: storage, migration, generations, privacy,
//! sessions and the cleanup crash/recovery matrix.

#[cfg(windows)]
mod common;
#[cfg(windows)]
#[path = "core/windows_cleanup.rs"]
mod windows_cleanup;
use mogumogu::clock::{DAY, FixedClock};
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::Core;
use mogumogu::storage::{GenerationResult, Repository, SCHEMA_VERSION};
use mogumogu::updates::NoNetwork;
use mogumogu::{Error, validation};
use rusqlite::Connection;
use std::fs;
use std::sync::Arc;

fn package(name: &str, version: &str) -> InventoryItem {
    InventoryItem::new(ItemCategory::Package, "npm", name, "package-lock.json")
        .version(version)
        .state(InstallState::Resolved)
}

fn complete(items: Vec<InventoryItem>) -> GenerationResult {
    GenerationResult { items, ..Default::default() }
}

// ------------------------------------------------------------- storage

#[test]
fn new_local_inventory_has_no_fabricated_data() {
    let mut repo = Repository::memory(false).unwrap();
    assert!(repo.projects().unwrap().is_empty());
    assert!(repo.resources().unwrap().is_empty());
}

#[test]
fn demo_fixture_is_consistent_and_read_only() {
    let mut repo = Repository::memory(true).unwrap();
    let projects = repo.projects().unwrap();
    assert_eq!(projects.len(), 6);
    assert_eq!(projects.iter().filter_map(|p| p.packages).sum::<i64>(), 428);
    let resources = repo.resources().unwrap();
    assert_eq!(resources.iter().filter_map(|r| r.bytes).sum::<i64>(), 142_000_000_000);
    assert_eq!(resources.iter().filter(|r| r.state == Observation::Review).count(), 3);
    assert!(resources.iter().all(|r| r.protected));
    assert!(matches!(repo.register_project("x", r"C:\dev\x"), Err(Error::Blocked(_))));
}

#[test]
fn dashboard_and_cli_review_agree_without_inventing_coverage() {
    for demo in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let clock = Arc::new(FixedClock::new(1_800_000_000));
        let config = Config::new(Some(temp.path().join("data")), demo).unwrap();
        let mut core = Core::open_with(config, clock.clone(), Box::new(NoNetwork)).unwrap();
        if !demo {
            // Metadata only: these paths are never created or approved.
            let project = core.register_project("example", r"C:\mogumogu-not-created\example").unwrap();
            core.register_scratch(project, "work", r"C:\mogumogu-not-created\example\work").unwrap();
            clock.advance(90 * DAY);
        }
        let snapshot = core.snapshot().unwrap();
        assert_eq!(snapshot.hints, core.review().unwrap(), "UI and CLI use identical review evidence");
        assert!(!snapshot.hints.is_empty());
        assert!(snapshot.hints.iter().all(|hint| hint.protected && !hint.plannable));
        if !demo {
            assert!(snapshot.projects.iter().all(|project| !project.read_approved && project.packages.is_none()));
            assert!(snapshot.storage.iter().all(|slice| slice.bytes.is_none()));
            assert!(snapshot.total_bytes().is_none(), "unmeasured resources remain unknown");
        }
    }
}

#[test]
fn registration_is_idempotent_protected_and_unscanned() {
    let mut repo = Repository::memory(false).unwrap();
    let one = repo.register_project("Example", r"C:\dev\example").unwrap();
    assert_eq!(one, repo.register_project("Example", r"c:/dev/example").unwrap());
    let project = repo.project(one).unwrap();
    assert!(project.protected && !project.read_approved);
    assert_eq!((project.bytes, project.packages, project.state), (None, None, Observation::Unknown));
}

#[test]
fn scratchpads_keep_their_owner_and_infer_lexical_owners() {
    let mut repo = Repository::memory(false).unwrap();
    let a = repo.register_project("A", r"C:\dev\a").unwrap();
    let b = repo.register_project("B", r"C:\dev\b").unwrap();
    assert!(repo.register_scratch(999, "x", r"C:\dev\a\x").is_err());
    let scratch = repo.register_scratch(a, "temp", r"C:\dev\a\temp").unwrap();
    assert_eq!(scratch, repo.register_scratch(a, "temp", r"C:\dev\a\temp").unwrap());
    assert!(matches!(repo.register_scratch(b, "temp", r"C:\dev\a\temp"), Err(Error::Conflict(_))));
    let owners = repo.owners().unwrap();
    assert!(owners.iter().any(|o| o.project_id == a && o.kind == OwnerKind::Explicit));
    assert!(!owners.iter().any(|o| o.project_id == b), "no owner by similar names");
}

#[test]
fn path_validation_rejects_unsafe_metadata_forms() {
    for path in [
        r"C:\",
        r"C:\dev\..\private",
        r"\\server\share",
        r"\\?\C:\dev\x",
        r"relative\path",
        r"C:\dev\foo:stream",
        r"C:\dev\\foo",
        r"C:\dev\foo.",
        r"C:\dev\foo ",
    ] {
        assert!(validation::windows_path(path).is_err(), "{path}");
    }
    assert_eq!(validation::windows_path("c:/dev/Grüner Ordner").unwrap(), r"C:\dev\Grüner Ordner");
    assert!(
        validation::name("").is_err()
            && validation::name("a\nb").is_err()
            && validation::name(&"a".repeat(101)).is_err()
    );
}

#[test]
fn payload_strings_are_data_not_sql() {
    let mut repo = Repository::memory(false).unwrap();
    repo.register_project("'; DROP TABLE projects; --", r"C:\dev\sql").unwrap();
    assert_eq!(repo.projects().unwrap()[0].name, "'; DROP TABLE projects; --");
}

#[test]
fn demo_and_local_databases_never_mix() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db.sqlite3");
    Repository::open_path(&path, false, Arc::new(FixedClock::new(0))).unwrap();
    assert!(matches!(Repository::open_path(&path, true, Arc::new(FixedClock::new(0))), Err(Error::Conflict(_))));
}

#[test]
fn v1_database_is_backed_up_and_migrated() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inventory.sqlite3");
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch(include_str!("../migrations/001_initial.sql")).unwrap();
        db.execute("INSERT INTO app_meta(key,value) VALUES('mode','local')", []).unwrap();
        db.execute("INSERT INTO projects(name,path,origin) VALUES('alt','C:\\dev\\alt','registered')", []).unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();
    }
    let mut repo = Repository::open_path(&path, false, Arc::new(FixedClock::new(1_000))).unwrap();
    assert_eq!(repo.projects().unwrap()[0].name, "alt");
    let backups: Vec<_> = fs::read_dir(temp.path().join("backups")).unwrap().collect();
    assert_eq!(backups.len(), 1, "consistent backup before migration");
    let version: i64 = Connection::open(&path).unwrap().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn newer_unknown_schema_is_refused_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("future.sqlite3");
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 99).unwrap();
    assert!(matches!(Repository::open_path(&path, false, Arc::new(FixedClock::new(0))), Err(Error::Conflict(_))));
    let db = Connection::open(&path).unwrap();
    let mode: String = db.pragma_query_value(None, "journal_mode", |r| r.get(0)).unwrap();
    assert_ne!(mode.to_lowercase(), "wal", "options untouched");
}

#[test]
fn backups_use_the_backup_api_and_never_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let mut repo = Repository::open_path(&temp.path().join("db.sqlite3"), false, Arc::new(FixedClock::new(0))).unwrap();
    repo.register_project("x", r"C:\dev\x").unwrap();
    let target = temp.path().join("copy.sqlite3");
    repo.backup_to(&target).unwrap();
    assert!(repo.backup_to(&target).is_err());
    let count: i64 =
        Connection::open(&target).unwrap().query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1);
}

// ---------------------------------------------------------- generations

fn scope(repo: &mut Repository) -> i64 {
    let project = repo.register_project("p", r"C:\dev\p").unwrap();
    repo.approve_scope(ScopeKind::Project, Some(project), r"C:\dev\p", None).unwrap()
}

#[test]
fn baseline_then_grouped_changes_without_flood() {
    let mut repo = Repository::memory(false).unwrap();
    let scope = scope(&mut repo);
    let g1 = repo.begin_generation(scope, "basis-1").unwrap();
    let first = repo.finish_generation(g1, &complete(vec![package("a", "1.0.0"), package("b", "1.0.0")])).unwrap();
    assert!(first.baseline && first.added == 0, "first pass is a baseline (AT-01)");
    let g2 = repo.begin_generation(scope, "basis-1").unwrap();
    let second = repo.finish_generation(g2, &complete(vec![package("a", "1.1.0"), package("c", "1.0.0")])).unwrap();
    assert_eq!((second.added, second.removed, second.changed), (1, 1, 1));
    let events = repo.inventory_events(10).unwrap();
    assert_eq!(
        events.iter().filter(|e| e.kind != InventoryEventKind::Baseline).count(),
        3,
        "one grouped event per kind"
    );
}

#[test]
fn partial_and_failed_passes_never_report_removals() {
    let mut repo = Repository::memory(false).unwrap();
    let scope = scope(&mut repo);
    let g1 = repo.begin_generation(scope, "basis-1").unwrap();
    repo.finish_generation(g1, &complete(vec![package("a", "1"), package("b", "1")])).unwrap();
    let g2 = repo.begin_generation(scope, "basis-1").unwrap();
    let partial = GenerationResult {
        items: vec![package("a", "1")],
        limit_reason: Some("Zeitbudget".into()),
        ..Default::default()
    };
    let report = repo.finish_generation(g2, &partial).unwrap();
    assert_eq!((report.state, report.removed), (Some(GenerationState::Partial), 0));
    let g3 = repo.begin_generation(scope, "basis-1").unwrap();
    repo.finish_generation(g3, &GenerationResult { failed: true, ..Default::default() }).unwrap();
    assert_eq!(repo.published_items(scope).unwrap().len(), 2, "last complete state stays published (AT-03)");
}

#[test]
fn new_catalog_starts_a_new_baseline() {
    let mut repo = Repository::memory(false).unwrap();
    let scope = scope(&mut repo);
    let g1 = repo.begin_generation(scope, "basis-1").unwrap();
    repo.finish_generation(g1, &complete(vec![package("a", "1")])).unwrap();
    let g2 = repo.begin_generation(scope, "basis-2").unwrap();
    let report = repo.finish_generation(g2, &complete(vec![package("a", "1"), package("z", "1")])).unwrap();
    assert!(report.baseline && report.added == 0, "newly recognised items are no fresh installs (AT-04)");
}

#[test]
fn interrupted_generation_is_marked_failed_on_restart() {
    let mut repo = Repository::memory(false).unwrap();
    let scope = scope(&mut repo);
    repo.begin_generation(scope, "basis-1").unwrap();
    assert_eq!(repo.recover_after_restart().unwrap().generations, 1);
    assert_eq!(repo.scope(scope).unwrap().latest.unwrap().state, GenerationState::Failed);
}

#[test]
fn synthetic_secrets_do_not_reach_the_database_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db.sqlite3");
    let mut repo = Repository::open_path(&path, false, Arc::new(FixedClock::new(0))).unwrap();
    let scope = scope(&mut repo);
    let g = repo.begin_generation(scope, "b").unwrap();
    let secret = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let item = InventoryItem::new(ItemCategory::AiArtifact, "x", format!("token={secret}"), "a")
        .detail(format!("https://u:{secret}@h/x"));
    let result =
        GenerationResult { items: vec![item], errors: vec![format!("Fehler mit {secret}")], ..Default::default() };
    repo.finish_generation(g, &result).unwrap();
    repo.log("x", &format!("Bearer {secret}"), secret).unwrap();
    drop(repo);
    for entry in fs::read_dir(temp.path()).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
            "secret in database, WAL or journal (AT-08)"
        );
    }
}
