//! Tidy-up report and collective scratchpad adoption on synthetic folders.
//! Nothing is removed; only metadata is read and stored.
#![cfg(windows)]
mod common;

use common::{path_text, write};
use mogumogu::Error;
use mogumogu::clock::{DAY, FixedClock};
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::Core;
use std::sync::Arc;

const NOW: i64 = 1_800_000_000;

fn core(path: &std::path::Path) -> Core {
    Core::open_with(Config::new(Some(path.join("data")), false).unwrap(), Arc::new(FixedClock::new(NOW))).unwrap()
}

#[test]
fn the_report_measures_only_approved_projects_and_changes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    write(&app, "package.json", "{}");
    write(&app, "package-lock.json", "{}");
    write(&app, "node_modules/dep/index.js", &"x".repeat(4_000));
    write(&app, "src/index.js", "console.log(1)");
    write(&app, "fixtures/big.json", &"y".repeat(2_000));
    let other = temp.path().join("other");
    write(&other, "Cargo.toml", "[package]");
    let mut core = core(temp.path());
    let app_id = core.register_project("app", &path_text(&app)).unwrap();
    let scope = core.approve_project(app_id).unwrap();
    let other_id = core.register_project("other", &path_text(&other)).unwrap();
    let report = core.tidy_report(None).unwrap();
    assert_eq!(report.unapproved, 1);
    assert_eq!(report.projects.len(), 1);
    let project = &report.projects[0];
    assert_eq!(project.project_id, app_id);
    assert!(project.complete && project.regenerable >= 4_000 && project.review >= 2_000, "{project:#?}");
    let modules = project.items.iter().find(|i| i.rel_path == "node_modules").unwrap();
    assert_eq!(modules.class, TidyClass::Regenerable);
    assert!(modules.evidence.contains("package-lock.json"));
    assert!(app.join("node_modules/dep/index.js").exists(), "nothing is removed");
    assert_eq!(core.repository().project(app_id).unwrap().bytes, project.total.map(|b| b as i64), "size recorded");
    assert_eq!(core.snapshot().unwrap().tidy.projects.len(), 1, "the report stays for the dashboard");
    assert!(matches!(core.tidy_report(Some(other_id)), Err(Error::Blocked(_))), "no approval, no measurement");
    core.revoke_scope(scope).unwrap();
    assert!(core.snapshot().unwrap().tidy.projects.is_empty(), "a revoked approval hides its report");
}

#[test]
fn found_scratchpads_are_adopted_once_protected_with_a_review_date() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("dev");
    write(&root, "app/Cargo.toml", "[package]");
    write(&root, "app/experiments/run.py", "print(1)");
    write(&root, "parser-spike/main.py", "print(2)");
    write(&root, "notes/todo.txt", "not a scratchpad");
    let mut core = core(temp.path());
    let app = core.register_project("app", &path_text(&root.join("app"))).unwrap();
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&root))).unwrap();
    core.scan(scope).unwrap();
    assert_eq!(core.adopt_found_scratchpads().unwrap(), 2);
    let snapshot = core.snapshot().unwrap();
    let scratch: Vec<&Resource> = snapshot.resources.iter().filter(|r| r.kind == ResourceKind::Scratchpad).collect();
    assert_eq!(scratch.len(), 2);
    for resource in &scratch {
        assert!(resource.protected && resource.project_id.is_none(), "{resource:?}");
        assert_eq!(resource.review_at, Some(NOW + 30 * DAY));
        assert_eq!(resource.acquisition, Acquisition::Adopted);
    }
    let inside = scratch.iter().find(|r| r.path.ends_with(r"app\experiments")).unwrap();
    let owners: Vec<&OwnerLink> = snapshot.owners_of(inside.id).collect();
    assert!(owners.iter().any(|o| o.project_id == app && o.kind == OwnerKind::Inferred), "{owners:?}");
    assert_eq!(core.adopt_found_scratchpads().unwrap(), 0, "already tracked");
    assert!(core.repository().scopes().unwrap().iter().all(|s| s.kind != ScopeKind::Project), "no read approval");
}

#[test]
fn package_caches_are_measured_with_their_own_approval_and_never_listed() {
    let temp = tempfile::tempdir().unwrap();
    let npm = temp.path().join("npm-cache");
    write(&npm, "_cacache/content-v2/sha512/ab/blob", &"z".repeat(3_000));
    let maven = temp.path().join("m2");
    write(&maven, "com/example/app/1.0-SNAPSHOT/app.jar", &"j".repeat(1_000));
    let mut core = core(temp.path());
    let npm_scope = core.approve_system(ScopeKind::NpmCache, Some(&path_text(&npm))).unwrap();
    let maven_scope = core.approve_system(ScopeKind::MavenRepository, Some(&path_text(&maven))).unwrap();
    let global = core.approve_system(ScopeKind::NpmGlobal, Some(&path_text(&npm))).unwrap();
    assert_ne!(npm_scope, global, "a cache approval never stands in for another kind at the same path");
    let kinds: Vec<ScopeKind> = core.repository().scopes().unwrap().iter().map(|s| s.kind).collect();
    assert!(kinds.contains(&ScopeKind::NpmCache) && kinds.contains(&ScopeKind::MavenRepository), "{kinds:?}");
    let scan = core.scan(npm_scope).unwrap();
    assert_eq!(scan.state, Some(GenerationState::Complete), "{scan:?}");
    assert!(core.snapshot().unwrap().packages.packages.is_empty(), "nothing in a cache becomes a package");
    let report = core.tidy_report(None).unwrap();
    let store = |id: i64| report.stores.iter().find(|s| s.scope_id == id).unwrap_or_else(|| panic!("{report:#?}"));
    let npm_store = store(npm_scope);
    assert_eq!((npm_store.class, npm_store.bytes), (TidyClass::Regenerable, Some(3_000)));
    assert!(npm_store.evidence.contains("npm cache clean --force"), "{}", npm_store.evidence);
    assert_eq!(store(maven_scope).class, TidyClass::Review, "self-built artifacts may live there");
    assert!(report.regenerable() >= 3_000);
    assert!(npm.join("_cacache/content-v2/sha512/ab/blob").is_file(), "nothing is removed");
}

#[test]
fn the_finishing_report_sorts_every_folder_once_and_changes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    write(&app, "package.json", "{}");
    write(&app, "package-lock.json", "{}");
    write(&app, "node_modules/dep/index.js", &"x".repeat(4_000));
    write(&app, "src/index.js", "console.log(1)");
    write(&app, "fixtures/big.json", &"y".repeat(2_000));
    write(&app, "golden/expected.json", &"z".repeat(1_500));
    write(&app, "logs/app.log", &"l".repeat(1_200));
    let mut core = core(temp.path());
    let app_id = core.register_project("app", &path_text(&app)).unwrap();
    core.approve_project(app_id).unwrap();
    core.keep_folder(app_id, "fixtures", true).unwrap();
    core.keep_folder(app_id, "golden", true).unwrap();
    let before = core.tidy_report(Some(app_id)).unwrap().projects.remove(0);

    let report = core.finish_report(app_id).unwrap();
    let paths = |items: &[FinishItem]| items.iter().map(|i| i.rel_path.clone()).collect::<Vec<_>>();
    assert_eq!(paths(&report.recoverable), ["node_modules"], "{report:#?}");
    assert_eq!(paths(&report.decide), ["logs"], "{report:#?}");
    assert_eq!(paths(&report.held), ["fixtures"], "{report:#?}");
    assert_eq!(report.held[0].evidence, "von dir behalten");
    // A kept folder the report does not list as its own entry is still named.
    assert_eq!(report.held_unmeasured, ["golden"]);
    assert!(report.saved.iter().any(|c| c.state != CheckState::Done), "no repository: the work is not saved");
    assert!(report.complete && report.own.is_some_and(|own| own > 0), "{report:#?}");

    // Read only: protection, keeps and what is recoverable stay as they were.
    let after = core.tidy_report(Some(app_id)).unwrap().projects.remove(0);
    let recoverable = |p: &ProjectTidy| {
        p.items.iter().filter(|i| i.class == TidyClass::Regenerable).map(|i| i.rel_path.clone()).collect::<Vec<_>>()
    };
    assert_eq!(recoverable(&before), recoverable(&after));
    assert_eq!(before.kept, after.kept);
    assert!(core.repository().project(app_id).unwrap().protected);
    assert!(app.join("node_modules/dep/index.js").exists() && app.join("golden/expected.json").exists());
}

#[test]
fn no_finishing_report_without_a_read_approval() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    write(&app, "Cargo.toml", "[package]");
    let mut core = core(temp.path());
    let app_id = core.register_project("app", &path_text(&app)).unwrap();
    assert!(matches!(core.finish_report(app_id), Err(Error::Blocked(_))));
}

#[test]
fn finishing_binds_the_report_changes_no_protection_and_reactivating_undoes_only_the_state() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    write(&app, "package.json", "{}");
    write(&app, "package-lock.json", "{}");
    write(&app, "node_modules/dep/index.js", &"x".repeat(4_000));
    write(&app, "logs/app.log", &"l".repeat(1_200));
    // The finish lies before the files' real change times, so "changed since"
    // is set once the project is finished.
    let clock = Arc::new(FixedClock::new(mogumogu::clock::Clock::now(&mogumogu::clock::SystemClock) - 3_600));
    let mut core = Core::open_with(Config::new(Some(temp.path().join("data")), false).unwrap(), clock).unwrap();
    let app_id = core.register_project("app", &path_text(&app)).unwrap();
    core.approve_project(app_id).unwrap();
    core.keep_folder(app_id, "logs", true).unwrap();
    let before = core.tidy_report(Some(app_id)).unwrap().projects.remove(0);

    let report = core.finish_report(app_id).unwrap();
    assert_eq!(report.finished_at, None);
    assert!(matches!(core.finish_project(app_id, &"0".repeat(64)), Err(Error::Blocked(_))), "a stale confirmation");
    assert!(core.repository().lifecycle(app_id).unwrap().is_none(), "nothing recorded");

    let finished = core.finish_project(app_id, &report.fingerprint).unwrap();
    assert!(finished.finished_at.is_some());
    assert!(finished.changed_since_finish(), "files newer than the (earlier) finish");
    assert!(matches!(core.finish_project(app_id, &finished.fingerprint), Err(Error::Blocked(_))), "already finished");
    let project = core.snapshot().unwrap().projects.into_iter().find(|p| p.id == app_id).unwrap();
    assert_eq!(project.finished_at, finished.finished_at);
    assert!(project.protected, "finishing changes no protection");
    let after = core.tidy_report(Some(app_id)).unwrap().projects.remove(0);
    assert_eq!(before.kept, after.kept);
    let recoverable = |p: &ProjectTidy| p.items.iter().filter(|i| i.class == TidyClass::Regenerable).count();
    assert_eq!(recoverable(&before), recoverable(&after));
    assert!(app.join("node_modules/dep/index.js").exists() && app.join("logs/app.log").exists());

    core.reactivate_project(app_id).unwrap();
    assert!(matches!(core.reactivate_project(app_id), Err(Error::Blocked(_))), "not finished any more");
    let history = core.lifecycle_history(app_id).unwrap();
    let states: Vec<Lifecycle> = history.iter().map(|e| e.state).collect();
    assert_eq!(states, [Lifecycle::Active, Lifecycle::Finished], "newest first");
    assert_eq!(core.finish_report(app_id).unwrap().finished_at, None);
    assert!(core.snapshot().unwrap().projects.iter().all(|p| p.finished_at.is_none()));
}
