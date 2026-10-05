//! Core contracts without UI: storage, migration, generations, privacy,
//! sessions and the cleanup crash/recovery matrix.

mod common;

use common::*;
use mogumogu::cleanup::{self, DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT, ExecutorHooks};
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

// --------------------------------------------- sessions and cleanup path

struct Env {
    _temp: tempfile::TempDir,
    core: Core,
    temp_dir: std::path::PathBuf,
    temp_id: i64,
}

/// Managed scratchpad with a verified session, an expendable temporary
/// folder holding `files` files, and a registered disposable test root.
fn cleanup_env(files: usize) -> Env {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("disposable");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(DISPOSABLE_MARKER), DISPOSABLE_MARKER_TEXT).unwrap();
    let project_dir = sample_project(temp.path());
    let config = Config::new(Some(temp.path().join("data")), false).unwrap();
    let mut core = Core::open_with(config, Arc::new(FixedClock::new(1_800_000_000)), Box::new(NoNetwork)).unwrap();
    let project = core.register_project("p", &path_text(&project_dir)).unwrap();
    let settings = Settings { scratch_root: Some(path_text(&root.join("scratch"))), ..Settings::default() };
    core.save_settings(&settings).unwrap();
    let scratch = core.create_scratch(project, "crash", "Test", None).unwrap();
    // A managed session whose job is empty: report + empty job = verified.
    let start = core.session_start(scratch, "test").unwrap();
    let job = mogumogu::platform::Job::create(&start.job_name).unwrap();
    core.session_attach(start.session_id, mogumogu::platform::current_process_identity().unwrap()).unwrap();
    let session = core.session_report(start.session_id, 0, 0).unwrap();
    assert_eq!(session.state, SessionState::CompletedVerified);
    drop(job);
    let explanation = core.explain(scratch).unwrap();
    let temporary = explanation.children.iter().find(|c| c.kind == ResourceKind::Temporary).unwrap().clone();
    for i in 0..files {
        write(std::path::Path::new(&temporary.path), &format!("d{}\\f{i}.tmp", i % 3), "x");
    }
    core.mark_expendable(temporary.id).unwrap();
    core.register_disposable_root(&path_text(&root)).unwrap();
    Env { _temp: temp, core, temp_dir: temporary.path.clone().into(), temp_id: temporary.id }
}

fn approved(env: &mut Env) -> i64 {
    let plan = env.core.cleanup_plan(&[env.temp_id]).unwrap();
    assert!(plan.executable(), "{:?}", plan.targets);
    env.core.cleanup_approve(plan.id, plan.short_fingerprint()).unwrap();
    plan.id
}

struct CrashOnBatch(usize, usize);
impl ExecutorHooks for CrashOnBatch {
    fn after_started(&mut self, _batch: &[i64]) -> mogumogu::Result<()> {
        self.1 += 1;
        if self.1 == self.0 { Err(Error::conflict("simulierter Absturz")) } else { Ok(()) }
    }
}

fn remaining_files(dir: &std::path::Path) -> usize {
    fs::read_dir(dir).unwrap().flatten().map(|e| if e.path().is_dir() { remaining_files(&e.path()) } else { 1 }).sum()
}

#[test]
fn crash_after_journal_commit_is_reconciled_not_repeated() {
    let mut env = cleanup_env(5);
    let plan = approved(&mut env);
    let repo = env.core.repository();
    assert!(cleanup::apply(repo, plan, &mut CrashOnBatch(1, 0)).is_err());
    assert_eq!(remaining_files(&env.temp_dir), 5, "crash happened before any removal");
    assert_eq!(repo.recover_after_restart().unwrap().plans_recovery, 1);
    assert_eq!(cleanup::reconcile(repo, plan).unwrap(), PlanState::Failed);
    let journal = repo.journal(plan).unwrap();
    assert!(
        journal.iter().filter(|j| !j.is_dir).all(|j| j.state == JournalState::Pending),
        "never re-executed (AT-20)"
    );
    assert_eq!(remaining_files(&env.temp_dir), 5);
    assert!(cleanup::apply(repo, plan, &mut cleanup::NoHooks).is_err(), "an interrupted plan is never resumed");
}

#[test]
fn crash_after_a_completed_batch_reports_partial_success() {
    let mut env = cleanup_env(100);
    let plan = approved(&mut env);
    let repo = env.core.repository();
    assert!(cleanup::apply(repo, plan, &mut CrashOnBatch(2, 0)).is_err());
    let left = remaining_files(&env.temp_dir);
    assert!(left > 0 && left < 100, "first batch removed, second untouched: {left}");
    repo.recover_after_restart().unwrap();
    assert_eq!(cleanup::reconcile(repo, plan).unwrap(), PlanState::Partial, "no claimed rollback (AT-18)");
    assert_eq!(remaining_files(&env.temp_dir), left);
}

#[test]
fn replaced_temp_folder_does_not_inherit_the_decision() {
    let mut env = cleanup_env(2);
    fs::remove_dir_all(&env.temp_dir).unwrap();
    fs::create_dir_all(&env.temp_dir).unwrap();
    fs::write(env.temp_dir.join("new.txt"), "neu").unwrap();
    let plan = env.core.cleanup_plan(&[env.temp_id]).unwrap();
    assert!(plan.targets[0].blockers.iter().any(|b| b.contains("ersetzt")), "{:?}", plan.targets[0].blockers);
}

#[test]
fn protecting_again_blocks_an_approved_plan() {
    let mut env = cleanup_env(2);
    let plan = approved(&mut env);
    env.core.protect(env.temp_id).unwrap();
    assert!(matches!(env.core.cleanup_apply(plan), Err(Error::Blocked(_))));
    assert_eq!(remaining_files(&env.temp_dir), 2);
}

#[test]
fn only_managed_temporary_folders_can_be_marked_expendable() {
    let env = cleanup_env(0);
    let mut core = env.core;
    let scratch = core.repository().resources().unwrap().into_iter().find(|r| r.kind == ResourceKind::Results).unwrap();
    assert!(matches!(core.mark_expendable(scratch.id), Err(Error::Blocked(_))));
}

#[test]
fn review_dates_create_hints_but_never_plans() {
    let mut env = cleanup_env(0);
    let clock_now = env.core.repository().now();
    let hints = env.core.review().unwrap();
    assert!(hints.iter().any(|h| h.plannable), "expendable temp output is plannable");
    let scratch =
        env.core.repository().resources().unwrap().into_iter().find(|r| r.kind == ResourceKind::Scratchpad).unwrap();
    assert!(scratch.review_at.unwrap() > clock_now && scratch.review_at.unwrap() <= clock_now + 7 * DAY);
}
