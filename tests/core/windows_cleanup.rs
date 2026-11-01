//! Cleanup contracts on Windows: recoverable folders of an approved project
//! move to a recycle bin, everything else stays. Tests use their own bin
//! below a temporary directory; the real recycle bin is never touched.
use super::common::*;
use mogumogu::Error;
use mogumogu::cleanup::{self, DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT, ExecutorHooks, KEEP_MARKER};
use mogumogu::clock::FixedClock;
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::Core;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

struct Env {
    _temp: tempfile::TempDir,
    core: Core,
    project: PathBuf,
    project_id: i64,
    bin: TestBin,
}

/// What the sample project has committed; its outputs are untracked.
const TRACKED: &[&str] =
    &["package-lock.json", "package.json", "requirements.txt", "tests/fixtures/sample.json", "tool/Cargo.toml"];

/// A sample project; inside a registered disposable test root unless
/// `outside` is set.
fn env(outside: bool) -> Env {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("disposable");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(DISPOSABLE_MARKER), DISPOSABLE_MARKER_TEXT).unwrap();
    let bin = TestBin::new(temp.path().join("papierkorb"));
    let project = sample_project(if outside { temp.path() } else { &root });
    write(&project, "tests/fixtures/sample.json", "{}");
    git_repo(&project, TRACKED);
    let config = Config::new(Some(temp.path().join("data")), false).unwrap();
    let mut core = Core::open_with(config, Arc::new(FixedClock::new(1_800_000_000))).unwrap();
    core.use_recycler(Box::new(bin.clone()));
    core.register_disposable_root(&path_text(&root)).unwrap();
    let project_id = core.register_project("web", &path_text(&project)).unwrap();
    core.approve_project(project_id).unwrap();
    Env { _temp: temp, core, project, project_id, bin }
}

fn approved(env: &mut Env) -> PlanSummary {
    let plan = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    assert!(plan.executable(), "{:#?}", plan.targets);
    env.core.cleanup_approve(plan.id, plan.short_fingerprint()).unwrap()
}

fn moved(env: &Env) -> usize {
    env.bin.count()
}

/// A path Windows would not normalise, so names ending in a dot stay.
fn verbatim(path: &std::path::Path) -> PathBuf {
    PathBuf::from(format!(r"\\?\{}", path_text(path)))
}

#[test]
fn only_recoverable_folders_move_and_everything_else_stays() {
    let mut env = env(false);
    let draft = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    let paths: Vec<&str> = draft.targets.iter().map(|t| t.rel_path.as_str()).collect();
    assert_eq!(paths.len(), 2, "{paths:?}");
    assert!(paths.contains(&"node_modules") && paths.contains(&"tool/target"), "{paths:?}");
    assert!(draft.targets.iter().all(|t| !t.evidence.is_empty()), "every folder names its evidence");
    assert!(matches!(env.core.cleanup_approve(draft.id, "000000000000"), Err(Error::Invalid(_))));
    env.core.cleanup_approve(draft.id, draft.short_fingerprint()).unwrap();
    let done = env.core.cleanup_apply(draft.id).unwrap();
    assert_eq!(done.state, PlanState::Completed, "{done:#?}");
    assert!(done.targets.iter().all(|t| t.state == JournalState::Done));
    assert_eq!(moved(&env), 2);
    assert!(!env.project.join("node_modules").exists() && !env.project.join("tool/target").exists());
    for kept in [
        "package.json",
        "package-lock.json",
        "tool/Cargo.toml",
        ".venv/pyvenv.cfg",
        "tests/fixtures/sample.json",
        ".git/HEAD",
    ] {
        assert!(env.project.join(kept).exists(), "{kept} stays");
    }
    assert!(
        env.bin.dir.join("1-node_modules/left-pad/index.js").is_file()
            || env.bin.dir.join("2-node_modules/left-pad/index.js").is_file()
    );
    assert!(matches!(env.core.cleanup_apply(draft.id), Err(Error::Conflict(_))), "a plan runs at most once");
}

#[test]
fn folders_to_review_cannot_be_planned() {
    let mut env = env(false);
    for path in [".venv", "tests/fixtures", "package.json", "src"] {
        let result = env.core.cleanup_plan(env.project_id, &[path.to_string()]);
        assert!(matches!(result, Err(Error::Blocked(_))), "{path}: {result:?}");
    }
}

/// The project page plans what the person ticked: a folder left out stays,
/// although its evidence would have allowed it.
#[test]
fn a_selection_plans_and_moves_only_the_chosen_folders() {
    let mut env = env(false);
    let all = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    assert!(all.targets.len() >= 2, "{:#?}", all.targets);
    env.core.cleanup_cancel(all.id).unwrap();
    let (chosen, left) = (all.targets[0].rel_path.clone(), all.targets[1].rel_path.clone());
    let plan = env.core.cleanup_plan(env.project_id, std::slice::from_ref(&chosen)).unwrap();
    assert_eq!(plan.targets.iter().map(|t| t.rel_path.as_str()).collect::<Vec<_>>(), [chosen.as_str()]);
    let plan = env.core.cleanup_approve(plan.id, plan.short_fingerprint()).unwrap();
    env.core.cleanup_apply(plan.id).unwrap();
    assert_eq!(moved(&env), 1);
    assert!(!env.project.join(&chosen).exists(), "{chosen} moved");
    assert!(env.project.join(&left).is_dir(), "{left} was left out and stays");
}

#[test]
fn outside_a_test_root_the_plan_stays_locked_until_review() {
    let mut env = env(true);
    let plan = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    assert!(!plan.targets.is_empty());
    assert!(plan.targets.iter().all(|t| t.blockers.iter().any(|b| b.contains("G5"))), "{:#?}", plan.targets);
    assert!(matches!(env.core.cleanup_approve(plan.id, plan.short_fingerprint()), Err(Error::Blocked(_))));
    assert!(matches!(env.core.cleanup_apply(plan.id), Err(Error::Conflict(_))), "a draft never runs");
    assert_eq!(moved(&env), 0);
    assert!(env.project.join("node_modules").is_dir());
}

/// A project registered through a junction inside the test root lies
/// elsewhere on disk; the path alone must not unlock it.
#[test]
fn a_project_reached_through_a_junction_in_the_test_root_stays_locked() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("disposable");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(DISPOSABLE_MARKER), DISPOSABLE_MARKER_TEXT).unwrap();
    let real = temp.path().join("real");
    let project = sample_project(&real);
    junction(&root.join("link"), &real);
    let config = Config::new(Some(temp.path().join("data")), false).unwrap();
    let mut core = Core::open_with(config, Arc::new(FixedClock::new(1_800_000_000))).unwrap();
    core.use_recycler(Box::new(TestBin { fail: true, ..TestBin::new(temp.path().join("papierkorb")) }));
    core.register_disposable_root(&path_text(&root)).unwrap();
    let through = root.join("link").join(project.file_name().unwrap());
    let project_id = core.register_project("web", &path_text(&through)).unwrap();
    core.approve_project(project_id).unwrap();
    let plan = core.cleanup_plan(project_id, &[]).unwrap();
    assert!(!plan.targets.is_empty());
    assert!(
        plan.targets.iter().all(|t| t.blockers.iter().any(|b| b.contains("Testwurzel nicht sicher"))),
        "{:#?}",
        plan.targets
    );
    assert!(matches!(core.cleanup_approve(plan.id, plan.short_fingerprint()), Err(Error::Blocked(_))));
    assert!(project.join("node_modules").is_dir());
}

/// Windows reads `foo.\node_modules` as `foo\node_modules`. With a junction
/// `foo` next to the folder `foo.`, the shell would move what the junction
/// points at instead of the folder checked on handles.
#[test]
fn a_name_the_shell_reads_differently_is_never_moved() {
    let mut env = env(false);
    let dotted = verbatim(&env.project.join("foo."));
    fs::create_dir_all(dotted.join("node_modules").join("dep")).unwrap();
    fs::write(dotted.join("package-lock.json"), "{}").unwrap();
    fs::write(dotted.join("node_modules").join("dep").join("index.js"), "1").unwrap();
    let outside = env.project.parent().unwrap().parent().unwrap().join("outside");
    write(&outside, "node_modules/PRECIOUS.txt", "keep me");
    junction(&env.project.join("foo"), &outside);
    let plan = env.core.cleanup_plan(env.project_id, &["foo./node_modules".to_string()]).unwrap();
    assert!(plan.targets[0].blockers.iter().any(|b| b.contains("nicht eindeutig")), "{:#?}", plan.targets);
    assert!(matches!(env.core.cleanup_approve(plan.id, plan.short_fingerprint()), Err(Error::Blocked(_))));
    assert_eq!(moved(&env), 0);
    assert!(outside.join("node_modules/PRECIOUS.txt").is_file());
    fs::remove_dir_all(&dotted).unwrap();
}

#[test]
fn version_control_keep_markers_or_links_anywhere_inside_keep_the_folder() {
    let mut env = env(false);
    write(&env.project, &format!("node_modules/left-pad/{KEEP_MARKER}"), "");
    write(&env.project, "tool/target/debug/vendored/.git/HEAD", "ref");
    let plan = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    let blocker = |rel: &str, text: &str| {
        let target = plan.targets.iter().find(|t| t.rel_path == rel).unwrap();
        assert!(target.blockers.iter().any(|b| b.contains(text)), "{rel}: {:?}", target.blockers);
    };
    blocker("node_modules", "geschützt");
    blocker("tool/target", "Versionsverwaltung");

    fs::remove_file(env.project.join(format!("node_modules/left-pad/{KEEP_MARKER}"))).unwrap();
    write(&env.project, "packages/ui/index.js", "source");
    fs::create_dir_all(env.project.join("node_modules/@ws")).unwrap();
    junction(&env.project.join("node_modules").join("@ws").join("ui"), &env.project.join("packages").join("ui"));
    let plan = env.core.cleanup_plan(env.project_id, &["node_modules".to_string()]).unwrap();
    assert!(plan.targets[0].blockers.iter().any(|b| b.contains("Verknüpfung")), "{:?}", plan.targets[0].blockers);
    assert!(env.project.join("packages/ui/index.js").is_file());
}

#[test]
fn a_protected_resource_inside_keeps_the_folder() {
    let mut env = env(false);
    let patched = env.project.join("node_modules").join("left-pad");
    env.core.register_scratch(env.project_id, "gepatcht", &path_text(&patched)).unwrap();
    let plan = env.core.cleanup_plan(env.project_id, &["node_modules".to_string()]).unwrap();
    assert!(plan.targets[0].blockers.iter().any(|b| b.contains("Geschützte Ressource")), "{:?}", plan.targets);
}

#[test]
fn a_missing_lockfile_after_approval_invalidates_the_plan() {
    let mut env = env(false);
    let plan = approved(&mut env);
    fs::remove_file(env.project.join("package-lock.json")).unwrap();
    assert!(matches!(env.core.cleanup_apply(plan.id), Err(Error::Blocked(_))));
    let after = cleanup::summary(env.core.repository(), plan.id).unwrap();
    assert_eq!(after.state, PlanState::Invalidated);
    assert_eq!(moved(&env), 0, "nothing moves once one folder fails the check");
    assert!(env.project.join("node_modules").is_dir() && env.project.join("tool/target").is_dir());
}

#[test]
fn a_replaced_folder_or_a_junction_on_the_way_invalidates_the_plan() {
    let mut env = env(false);
    let plan = approved(&mut env);
    fs::remove_dir_all(env.project.join("node_modules")).unwrap();
    write(&env.project, "node_modules/other/index.js", "neu");
    assert!(matches!(env.core.cleanup_apply(plan.id), Err(Error::Blocked(_))));

    let plan = approved(&mut env);
    let real = env.project.join("tool-real");
    fs::rename(env.project.join("tool"), &real).unwrap();
    junction(&env.project.join("tool"), &real);
    assert!(matches!(env.core.cleanup_apply(plan.id), Err(Error::Blocked(_))));
    assert_eq!(moved(&env), 0);
    assert!(real.join("target/debug/tool.exe").is_file(), "nothing behind the junction moved");
}

#[test]
fn a_keep_marker_on_the_way_or_inside_blocks_the_folder() {
    let mut env = env(false);
    write(&env.project, &format!("tool/{KEEP_MARKER}"), "");
    write(&env.project, &format!("node_modules/{KEEP_MARKER}"), "");
    let plan = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    assert!(plan.targets.iter().all(|t| t.blockers.iter().any(|b| b.contains(KEEP_MARKER))), "{:#?}", plan.targets);
    assert!(matches!(env.core.cleanup_approve(plan.id, plan.short_fingerprint()), Err(Error::Blocked(_))));
}

struct CrashOn(i64, i64);
impl ExecutorHooks for CrashOn {
    fn after_started(&mut self, _target_id: i64) -> mogumogu::Result<()> {
        self.1 += 1;
        if self.1 == self.0 { Err(Error::conflict("simulierter Absturz")) } else { Ok(()) }
    }
}

#[test]
fn crash_after_the_journal_commit_is_reconciled_not_repeated() {
    let mut env = env(false);
    let plan = approved(&mut env);
    let mut bin = env.bin.clone();
    let repo = env.core.repository();
    assert!(cleanup::apply(repo, plan.id, &mut bin, &mut CrashOn(1, 0), &|_| None).is_err());
    assert_eq!(repo.recover_after_restart().unwrap().plans_recovery, 1);
    // The second folder was never started; it disappears some other way.
    fs::remove_dir_all(env.project.join("tool/target")).unwrap();
    assert_eq!(cleanup::reconcile(repo, plan.id).unwrap(), PlanState::Failed, "nothing counts as moved");
    let after = cleanup::summary(repo, plan.id).unwrap();
    let states: Vec<JournalState> = after.targets.iter().map(|t| t.state).collect();
    assert_eq!(states, vec![JournalState::Pending, JournalState::Failed], "{:#?}", after.targets);
    assert_eq!(after.done, 0);
    assert!(cleanup::apply(repo, plan.id, &mut bin, &mut cleanup::NoHooks, &|_| None).is_err(), "never resumed");
    assert_eq!(moved(&env), 0);
    assert!(env.project.join("node_modules").is_dir());
}

/// Swaps the folder for another one right after its row was committed.
struct SwapAfterStart(PathBuf, bool);
impl ExecutorHooks for SwapAfterStart {
    fn after_started(&mut self, _target_id: i64) -> mogumogu::Result<()> {
        let folder = self.0.join("node_modules");
        if !std::mem::replace(&mut self.1, true) {
            fs::rename(&folder, self.0.join("node_modules-away")).unwrap();
            fs::create_dir_all(folder.join("other")).unwrap();
        }
        Ok(())
    }
}

#[test]
fn a_folder_swapped_after_the_journal_commit_is_not_moved() {
    let mut env = env(false);
    let plan = approved(&mut env);
    let mut bin = env.bin.clone();
    let project = env.project.clone();
    let repo = env.core.repository();
    let state =
        cleanup::apply(repo, plan.id, &mut bin, &mut SwapAfterStart(project.clone(), false), &|_| None).unwrap();
    assert_eq!(state, PlanState::Partial);
    let after = cleanup::summary(repo, plan.id).unwrap();
    let modules = after.targets.iter().find(|t| t.rel_path == "node_modules").unwrap();
    assert!(modules.error.as_deref().is_some_and(|e| e.contains("vor dem Verschieben verändert")), "{modules:?}");
    assert!(project.join("node_modules/other").is_dir(), "the stranger stays");
    assert!(project.join("node_modules-away/left-pad/index.js").is_file());
}

#[test]
fn crash_after_one_move_reports_partial_success() {
    let mut env = env(false);
    let plan = approved(&mut env);
    let mut bin = env.bin.clone();
    let repo = env.core.repository();
    assert!(cleanup::apply(repo, plan.id, &mut bin, &mut CrashOn(2, 0), &|_| None).is_err());
    repo.recover_after_restart().unwrap();
    assert_eq!(cleanup::reconcile(repo, plan.id).unwrap(), PlanState::Partial, "no claimed rollback");
    let after = cleanup::summary(repo, plan.id).unwrap();
    let states: Vec<JournalState> = after.targets.iter().map(|t| t.state).collect();
    assert_eq!(states, vec![JournalState::Done, JournalState::Pending], "{:#?}", after.targets);
    assert_eq!(moved(&env), 1);
}

#[test]
fn a_failing_or_idle_recycle_bin_is_reported_and_leaves_the_folder() {
    let mut env = env(false);
    let plan = approved(&mut env);
    env.core.use_recycler(Box::new(TestBin { fail: true, ..env.bin.clone() }));
    let failed = env.core.cleanup_apply(plan.id).unwrap();
    assert_eq!(failed.state, PlanState::Failed);
    assert!(failed.targets.iter().all(|t| t.error.as_deref().is_some_and(|e| e.contains("Papierkorb"))));
    assert!(env.project.join("node_modules").is_dir());

    let plan = approved(&mut env);
    env.core.use_recycler(Box::new(TestBin { keep: true, ..env.bin.clone() }));
    let idle = env.core.cleanup_apply(plan.id).unwrap();
    assert_eq!(idle.state, PlanState::Failed, "a folder still in place is not counted as moved");
    assert!(idle.targets.iter().all(|t| t.error.as_deref().is_some_and(|e| e.contains("noch am Ort"))));
}

#[test]
fn a_folder_with_versioned_files_is_never_planned_and_its_evidence_names_git() {
    let mut env = env(false);
    // A JavaScript action commits its bundled dependencies.
    git_repo(&env.project, &[TRACKED, &["node_modules/left-pad/index.js"]].concat());
    let plan = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    let paths: Vec<&str> = plan.targets.iter().map(|t| t.rel_path.as_str()).collect();
    assert_eq!(paths, ["tool/target"], "{:#?}", plan.targets);
    assert!(plan.targets[0].evidence.ends_with("nicht in Git"), "{}", plan.targets[0].evidence);
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    let modules = report.projects[0].items.iter().find(|i| i.rel_path == "node_modules").unwrap();
    assert_eq!(modules.class, TidyClass::Review);
    assert!(modules.evidence.contains("in Git versioniert"), "{}", modules.evidence);
    let asked = env.core.cleanup_plan(env.project_id, &["node_modules".into()]);
    assert!(matches!(asked, Err(Error::Blocked(_))), "{asked:?}");
}

#[test]
fn files_git_starts_to_track_after_approval_invalidate_the_plan() {
    let mut env = env(false);
    let plan = approved(&mut env);
    git_repo(&env.project, &[TRACKED, &["tool/target/debug/tool.exe"]].concat());
    let result = env.core.cleanup_apply(plan.id);
    assert!(
        matches!(&result, Err(Error::Blocked(note)) if note.contains("Beleg geändert") || note.contains("Git")),
        "{result:?}"
    );
    assert_eq!(moved(&env), 0);
    assert!(env.project.join("node_modules").is_dir() && env.project.join("tool/target").is_dir());
}

#[test]
fn an_unreadable_index_or_a_worktree_link_keeps_everything() {
    let mut env = env(false);
    fs::write(env.project.join(".git/index"), b"DIRC\0\0\0\x02\0\0\0\x05broken").unwrap();
    assert!(matches!(env.core.cleanup_plan(env.project_id, &[]), Err(Error::Blocked(_))), "nothing proven");
    fs::remove_dir_all(env.project.join(".git")).unwrap();
    write(&env.project, ".git", "gitdir: C:/elsewhere/.git/worktrees/web\n");
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    assert_eq!(report.projects[0].regenerable, 0, "{:#?}", report.projects[0].items);
    assert!(report.projects[0].items.iter().any(|i| i.evidence.contains("Index liegt außerhalb")));
}

#[test]
fn kept_folders_stay_out_of_plans_and_stop_an_approved_one() {
    let mut env = env(false);
    let plan = approved(&mut env);
    let report = env.core.keep_folder(env.project_id, r"tool\target", true).unwrap();
    let project = &report.projects[0];
    assert_eq!(project.kept, ["tool/target"]);
    let target = project.items.iter().find(|i| i.rel_path == "tool/target").unwrap();
    assert_eq!(target.class, TidyClass::Review);
    assert!(target.evidence.ends_with("von dir behalten"), "{}", target.evidence);
    let result = env.core.cleanup_apply(plan.id);
    assert!(matches!(&result, Err(Error::Blocked(note)) if note.contains("Von dir behalten")), "{result:?}");
    assert_eq!(moved(&env), 0, "the approved plan stops before anything moves");
    let fresh = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    assert_eq!(fresh.targets.iter().map(|t| t.rel_path.as_str()).collect::<Vec<_>>(), ["node_modules"]);
    // Keeping something inside a folder keeps the folder; undoing a keep restores it.
    env.core.keep_folder(env.project_id, "node_modules/left-pad", true).unwrap();
    assert!(matches!(env.core.cleanup_plan(env.project_id, &[]), Err(Error::Blocked(_))));
    for rel in ["TOOL/target", "node_modules/left-pad"] {
        env.core.keep_folder(env.project_id, rel, false).unwrap();
    }
    assert_eq!(env.core.cleanup_plan(env.project_id, &[]).unwrap().targets.len(), 2);
    for bad in ["", "..", "a/../b", r"C:\dev", "a:b", "dist."] {
        assert!(matches!(env.core.keep_folder(env.project_id, bad, true), Err(Error::Invalid(_))), "{bad:?}");
    }
}

#[test]
fn an_executed_plan_records_the_free_space_it_measured() {
    let mut env = env(false);
    let plan = approved(&mut env);
    assert_eq!((plan.free_before, plan.free_after), (None, None), "nothing measured before execution");
    let done = env.core.cleanup_apply(plan.id).unwrap();
    assert!(done.free_before.is_some() && done.free_after.is_some(), "{done:#?}");
    let text = done.space_text().unwrap();
    assert!(text.contains("erst beim Leeren"), "{text}");
}

#[test]
fn agent_leftovers_are_reported_but_never_planned() {
    let mut env = env(false);
    write(&env.project, ".claude/worktrees/fix/package.json", "{}");
    write(&env.project, ".claude/worktrees/fix/package-lock.json", "{}");
    write(&env.project, ".claude/worktrees/fix/node_modules/x/index.js", "1");
    write(&env.project, ".playwright-mcp/console.log", "ok");
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    let project = &report.projects[0];
    assert!(project.agent > 0);
    for rel in [".claude/worktrees", ".playwright-mcp"] {
        let item = project.items.iter().find(|i| i.rel_path == rel).unwrap();
        assert_eq!((item.class, item.label.as_str()), (TidyClass::Review, AGENT_LABEL), "{rel}");
    }
    assert!(
        project.items.iter().all(|i| !i.rel_path.starts_with(".claude/worktrees/")),
        "one item per worktree folder"
    );
    for rel in [".claude/worktrees", ".playwright-mcp", ".claude/worktrees/fix/node_modules"] {
        assert!(matches!(env.core.cleanup_plan(env.project_id, &[rel.into()]), Err(Error::Blocked(_))), "{rel}");
    }
}

/// Registers and approves another project in the same core.
fn add_project(env: &mut Env, name: &str, path: &std::path::Path) -> i64 {
    let id = env.core.register_project(name, &path_text(path)).unwrap();
    env.core.approve_project(id).unwrap();
    id
}

#[test]
fn a_project_inside_a_repository_it_cannot_read_moves_nothing() {
    let mut env = env(false);
    // The sample project is a work tree; a package inside it is registered on its own.
    let package = env.project.join("tool");
    let inner = add_project(&mut env, "tool", &package);
    let report = env.core.tidy_report(Some(inner)).unwrap();
    let target = report.projects[0].items.iter().find(|i| i.rel_path == "target").unwrap();
    assert_eq!(target.class, TidyClass::Review);
    assert!(target.evidence.contains("oberhalb der Freigabe"), "{}", target.evidence);
    assert!(matches!(env.core.cleanup_plan(inner, &[]), Err(Error::Blocked(_))));
}

#[test]
fn a_keep_counts_for_every_project_that_reaches_the_folder() {
    let mut env = env(false);
    fs::remove_dir_all(env.project.join(".git")).unwrap();
    // The same folder under another spelling, and a project nested in it.
    let upper = PathBuf::from(path_text(&env.project).replace("project", "PROJECT"));
    let twin = add_project(&mut env, "twin", &upper);
    let package = env.project.join("tool");
    let tool = add_project(&mut env, "tool", &package);
    let plan = env.core.cleanup_plan(tool, &[]).unwrap();
    let plan = env.core.cleanup_approve(plan.id, plan.short_fingerprint()).unwrap();
    env.core.keep_folder(env.project_id, "tool/target", true).unwrap();
    env.core.keep_folder(env.project_id, "node_modules", true).unwrap();
    let result = env.core.cleanup_apply(plan.id);
    assert!(matches!(&result, Err(Error::Blocked(note)) if note.contains("Von dir behalten")), "{result:?}");
    assert!(matches!(env.core.cleanup_plan(twin, &[]), Err(Error::Blocked(_))), "kept under the other spelling too");
    assert_eq!(moved(&env), 0);
    assert!(env.project.join("tool/target").is_dir() && env.project.join("node_modules").is_dir());
}

#[test]
fn a_keep_for_a_short_name_is_resolved_or_refused() {
    let mut env = env(false);
    let missing = env.core.keep_folder(env.project_id, "LONGFO~1/node_modules", true);
    assert!(matches!(missing, Err(Error::Blocked(_))), "an alias that may later mean another folder: {missing:?}");
    write(&env.project, "longfoldername/package.json", "{}");
    write(&env.project, "longfoldername/package-lock.json", "{}");
    write(&env.project, "longfoldername/node_modules/x/index.js", "1");
    if !env.project.join("LONGFO~1").is_dir() {
        return; // no 8.3 names on this volume
    }
    let report = env.core.keep_folder(env.project_id, "LONGFO~1/node_modules", true).unwrap();
    let item = report.projects[0].items.iter().find(|i| i.rel_path == "longfoldername/node_modules").unwrap();
    assert!(item.evidence.ends_with("von dir behalten"), "the alias names the long folder: {}", item.evidence);
    let plan = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    assert!(plan.targets.iter().all(|t| t.rel_path != "longfoldername/node_modules"), "{:#?}", plan.targets);
}

#[test]
fn git_at_work_after_approval_invalidates_the_plan() {
    let mut env = env(false);
    let plan = approved(&mut env);
    write(&env.project, ".git/index.lock", "");
    let result = env.core.cleanup_apply(plan.id);
    assert!(matches!(&result, Err(Error::Blocked(note)) if note.contains("index.lock")), "{result:?}");
    assert_eq!(moved(&env), 0);
}
