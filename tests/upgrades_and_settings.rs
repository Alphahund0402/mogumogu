//! Schema v4 user realm, schema v5 without the old deletion path, search and
//! settings.
//! Synthetic metadata in temporary folders only; nothing is started.
#![cfg(windows)]

mod common;

use mogumogu::Error;
use mogumogu::clock::FixedClock;
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::Core;
use mogumogu::storage::{Repository, SCHEMA_VERSION};
use rusqlite::Connection;
use std::sync::Arc;

fn core_at(temp: &std::path::Path, clock: Arc<FixedClock>) -> Core {
    let config = Config::new(Some(temp.join("data")), false).unwrap();
    Core::open_with(config, clock).unwrap()
}

#[test]
fn v3_scopes_keep_their_meaning_and_user_folders_get_their_own_realm() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inventory.sqlite3");
    {
        let db = Connection::open(&path).unwrap();
        for sql in [
            include_str!("../migrations/001_initial.sql"),
            include_str!("../migrations/002_inventory.sql"),
            include_str!("../migrations/003_package_evidence.sql"),
        ] {
            db.execute_batch(sql).unwrap();
        }
        db.execute("INSERT INTO app_meta(key,value) VALUES('mode','local')", []).unwrap();
        db.execute("INSERT INTO projects(name,path,origin) VALUES('cfg','C:\\Users\\x\\.claude','registered')", [])
            .unwrap();
        db.execute(
            "INSERT INTO scopes(project_id,kind,path,status,approved_at,profile) VALUES
               (1,'project','C:\\Users\\x\\.claude','approved',0,''),
               (NULL,'project','C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules','approved',0,'npm-global')",
            [],
        )
        .unwrap();
        db.pragma_update(None, "user_version", 3).unwrap();
    }
    let mut repo = Repository::open_path(&path, false, Arc::new(FixedClock::new(1_000))).unwrap();
    let kinds: Vec<ScopeKind> = repo.scopes().unwrap().iter().map(|s| s.kind).collect();
    assert_eq!(kinds, vec![ScopeKind::Project, ScopeKind::NpmGlobal], "existing approvals unchanged");
    let user = repo.approve_scope(ScopeKind::UserAi, None, r"C:\Users\x\.claude", None).unwrap();
    let scopes = repo.scopes().unwrap();
    assert_eq!(scopes.len(), 3, "a user folder never replaces a project approval at the same path");
    assert_eq!(scopes.iter().find(|s| s.id == user).unwrap().kind, ScopeKind::UserAi);
    let version: i64 = Connection::open(&path).unwrap().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn v4_plans_of_the_old_deletion_path_never_run_and_sessions_are_gone() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inventory.sqlite3");
    {
        let db = Connection::open(&path).unwrap();
        for sql in [
            include_str!("../migrations/001_initial.sql"),
            include_str!("../migrations/002_inventory.sql"),
            include_str!("../migrations/003_package_evidence.sql"),
            include_str!("../migrations/004_user_scopes.sql"),
        ] {
            db.execute_batch(sql).unwrap();
        }
        db.execute_batch(
            "INSERT INTO app_meta(key,value) VALUES('mode','local');
             INSERT INTO resources(name,path,kind,evidence,origin) VALUES('tmp','C:\\fixture\\tmp','temporary','x','registered');
             INSERT INTO sessions(scratch_id,purpose,state,origin,started_at) VALUES(1,'fixture','running','managed',0);
             INSERT INTO cleanup_plans(state,created_at,rule_version,fingerprint) VALUES('approved',0,'cleanup-rules-1','f');
             INSERT INTO cleanup_plans(state,created_at,rule_version,fingerprint) VALUES('completed',0,'cleanup-rules-1','g');
             INSERT INTO cleanup_targets(plan_id,resource_id,path,identity,entries,bytes) VALUES(1,1,'C:\\fixture\\tmp','x',1,1);
             INSERT INTO action_journal(plan_id,target_id,rel_path,identity,is_dir,depth,state,updated_at)
                 VALUES(1,1,'a.txt','x',0,1,'planned',0);",
        )
        .unwrap();
        db.pragma_update(None, "user_version", 4).unwrap();
    }
    let mut repo = Repository::open_path(&path, false, Arc::new(FixedClock::new(1_000))).unwrap();
    assert_eq!(repo.plan(1).unwrap().state, PlanState::Invalidated, "an approved old plan cannot run");
    assert_eq!(repo.plan(2).unwrap().state, PlanState::Completed, "history stays");
    assert!(repo.plan(1).unwrap().targets.is_empty());
    assert_eq!(repo.resources().unwrap().len(), 1, "resources stay");
    let db = Connection::open(&path).unwrap();
    for gone in ["sessions", "cleanup_targets", "cleanup_manifest", "action_journal"] {
        let found: i64 =
            db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = ?1", [gone], |r| r.get(0)).unwrap();
        assert_eq!(found, 0, "{gone}");
    }
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), SCHEMA_VERSION);
}

#[test]
fn search_spans_areas_and_needs_two_characters() {
    let temp = tempfile::tempdir().unwrap();
    let mut core = core_at(temp.path(), Arc::new(FixedClock::new(1_800_000_000)));
    let project = core.register_project("billing-api", r"C:\fixture\billing").unwrap();
    core.register_scratch(project, "billing-spike", r"C:\fixture\billing\spike").unwrap();
    assert!(core.search("b").unwrap().is_empty());
    let hits = core.search("BILLING").unwrap();
    assert!(hits.iter().any(|h| h.area == SearchArea::Project && h.title == "billing-api"));
    assert!(hits.iter().any(|h| h.area == SearchArea::Resource && h.title == "billing-spike"));
    assert!(core.search("%").unwrap().is_empty(), "LIKE wildcards are literal");
}

#[test]
fn settings_are_validated_and_kept() {
    let temp = tempfile::tempdir().unwrap();
    let mut core = core_at(temp.path(), Arc::new(FixedClock::new(1_800_000_000)));
    let mut settings = core.settings().unwrap();
    assert_eq!(settings.scratch_review_days, 7);
    settings.scratch_review_days = 0;
    assert!(matches!(core.save_settings(&settings), Err(Error::Invalid(_))));
    settings.scratch_review_days = 14;
    core.save_settings(&settings).unwrap();
    assert_eq!(core.settings().unwrap().scratch_review_days, 14);
}

#[test]
fn approvals_for_installed_program_lists_are_retired_once() {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(FixedClock::new(1_800_000_000));
    let scope = {
        let mut core = core_at(temp.path(), clock.clone());
        core.repository().approve_scope(ScopeKind::Scoop, None, r"C:\fixture\scoop", None).unwrap()
    };
    let mut core = core_at(temp.path(), clock);
    assert_eq!(core.repository().scope(scope).unwrap().status, ScopeStatus::Revoked);
    assert!(core.repository().activity(20).unwrap().iter().any(|e| e.title.contains("installierte Programme")));
    for kind in [ScopeKind::Winget, ScopeKind::Scoop, ScopeKind::Chocolatey] {
        assert!(matches!(core.approve_system(kind, None), Err(Error::Blocked(_))), "{kind:?}");
    }
    assert!(core.scan_all().unwrap().is_empty(), "a full pass skips revoked scopes");
}
