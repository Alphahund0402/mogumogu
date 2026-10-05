//! Windows session and cleanup contracts, isolated from portable storage tests.
use super::common::*;
use mogumogu::Error;
use mogumogu::cleanup::{self, DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT, ExecutorHooks};
use mogumogu::clock::{DAY, FixedClock};
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::Core;
use mogumogu::updates::NoNetwork;
use std::{fs, sync::Arc};

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
