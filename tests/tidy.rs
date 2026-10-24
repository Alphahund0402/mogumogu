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
