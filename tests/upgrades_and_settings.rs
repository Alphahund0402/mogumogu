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
fn v6_storage_analysis_is_a_separate_approval_and_upgrade_is_backed_up() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inventory.sqlite3");
    {
        let db = Connection::open(&path).unwrap();
        for sql in [
            include_str!("../migrations/001_initial.sql"),
            include_str!("../migrations/002_inventory.sql"),
            include_str!("../migrations/003_package_evidence.sql"),
            include_str!("../migrations/004_user_scopes.sql"),
            include_str!("../migrations/005_recycle.sql"),
            include_str!("../migrations/006_cache_scopes.sql"),
        ] {
            db.execute_batch(sql).unwrap();
        }
        db.execute_batch(
            "INSERT INTO app_meta(key,value) VALUES('mode','local');
            INSERT INTO scopes(kind,path,status,approved_at,profile,realm,cache) VALUES
            ('project','C:\\fixture\\same','approved',0,'','',''),
            ('project','C:\\fixture\\same','approved',0,'projects','',''),
            ('project','C:\\fixture\\same','approved',0,'','user',''),
            ('project','C:\\fixture\\same','approved',0,'','','npm-cache');",
        )
        .unwrap();
        db.pragma_update(None, "user_version", 6).unwrap();
    }
    let mut repo = Repository::open_path(&path, false, Arc::new(FixedClock::new(1_000))).unwrap();
    let original: Vec<_> = repo.scopes().unwrap().into_iter().map(|s| s.kind).collect();
    assert_eq!(original, vec![ScopeKind::Project, ScopeKind::ProjectSearch, ScopeKind::UserAi, ScopeKind::NpmCache]);
    let storage = repo.approve_scope(ScopeKind::Storage, None, r"C:\fixture\same", None).unwrap();
    assert_eq!(repo.scope(storage).unwrap().kind, ScopeKind::Storage);
    assert_eq!(repo.scopes().unwrap().len(), 5);
    assert_eq!(repo.approve_scope(ScopeKind::Storage, None, r"C:\fixture\same", None).unwrap(), storage);
    let db = Connection::open(&path).unwrap();
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), SCHEMA_VERSION);
    assert_eq!(db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
    let backups: Vec<_> = std::fs::read_dir(temp.path().join("backups")).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(backups.len(), 1);
    let before = Connection::open(&backups[0]).unwrap();
    assert_eq!(before.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), 6);
    assert_eq!(before.query_row("SELECT COUNT(*) FROM scopes", [], |r| r.get::<_, i64>(0)).unwrap(), 4);
}

#[test]
fn v7_plans_keep_their_state_and_keeps_belong_to_a_known_project() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inventory.sqlite3");
    {
        let db = Connection::open(&path).unwrap();
        for sql in [
            include_str!("../migrations/001_initial.sql"),
            include_str!("../migrations/002_inventory.sql"),
            include_str!("../migrations/003_package_evidence.sql"),
            include_str!("../migrations/004_user_scopes.sql"),
            include_str!("../migrations/005_recycle.sql"),
            include_str!("../migrations/006_cache_scopes.sql"),
            include_str!("../migrations/007_storage_analysis.sql"),
        ] {
            db.execute_batch(sql).unwrap();
        }
        db.execute_batch(
            "INSERT INTO app_meta(key,value) VALUES('mode','local');
            INSERT INTO projects(name,path,origin) VALUES('web','C:\\fixture\\web','registered');
            INSERT INTO cleanup_plans(state,created_at,rule_version,fingerprint) VALUES('completed',5,'recycle-rules-2','ab');",
        )
        .unwrap();
        db.pragma_update(None, "user_version", 7).unwrap();
    }
    let mut repo = Repository::open_path(&path, false, Arc::new(FixedClock::new(1_000))).unwrap();
    let plan = repo.plan(1).unwrap();
    assert_eq!((plan.state, plan.free_before, plan.free_after), (PlanState::Completed, None, None));
    assert!(repo.kept_folders(1).unwrap().is_empty());
    repo.keep_folder(1, "web/dist", r"C:\fixture\web\web\dist").unwrap();
    repo.keep_folder(1, "Web/Dist", r"C:\fixture\web\web\dist").unwrap();
    assert_eq!(repo.kept_folders(1).unwrap(), ["web/dist"], "one keep per folder, whatever the case");
    assert_eq!(repo.kept_locations().unwrap(), [r"C:\fixture\web\web\dist"]);
    assert!(matches!(repo.keep_folder(99, "dist", r"C:\x\dist"), Err(Error::NotFound(_))));
    // SQLite's lower() folds ASCII only; the comparison happens in Rust.
    repo.keep_folder(1, "Über/dist", r"C:\fixture\web\Über\dist").unwrap();
    repo.unkeep_folder(1, "über/DIST").unwrap();
    repo.unkeep_folder(1, "WEB/dist").unwrap();
    assert!(repo.kept_folders(1).unwrap().is_empty());
    assert!(matches!(repo.unkeep_folder(1, "web/dist"), Err(Error::NotFound(_))), "nothing silently acknowledged");
    let db = Connection::open(&path).unwrap();
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), SCHEMA_VERSION);
    assert_eq!(db.query_row("PRAGMA foreign_key_check", [], |r| r.get::<_, i64>(0)).ok(), None);
}

#[test]
fn v8_upgrades_with_runs_off_and_nothing_approved_for_docker() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inventory.sqlite3");
    {
        let db = Connection::open(&path).unwrap();
        for sql in [
            include_str!("../migrations/001_initial.sql"),
            include_str!("../migrations/002_inventory.sql"),
            include_str!("../migrations/003_package_evidence.sql"),
            include_str!("../migrations/004_user_scopes.sql"),
            include_str!("../migrations/005_recycle.sql"),
            include_str!("../migrations/006_cache_scopes.sql"),
            include_str!("../migrations/007_storage_analysis.sql"),
            include_str!("../migrations/008_cleanup_keeps.sql"),
        ] {
            db.execute_batch(sql).unwrap();
        }
        db.execute_batch(
            "INSERT INTO app_meta(key,value) VALUES('mode','local');
            INSERT INTO projects(name,path,origin) VALUES('web','C:\\fixture\\web','registered');
            INSERT INTO settings(key,value) VALUES('settings','{\"scratch_review_days\":9}');
            INSERT INTO cleanup_keeps(project_id,rel_path,location,created_at) VALUES(1,'dist','C:\\fixture\\web\\dist',1);",
        )
        .unwrap();
        db.pragma_update(None, "user_version", 8).unwrap();
    }
    let mut repo = Repository::open_path(&path, false, Arc::new(FixedClock::new(1_000))).unwrap();
    let settings = repo.settings().unwrap();
    assert_eq!((settings.scratch_review_days, settings.agent_runs), (9, false), "runs stay off after the upgrade");
    assert!(repo.runs(None, 10, 10).unwrap().is_empty() && repo.docker_approvals().unwrap().is_empty());
    assert!(repo.tidy_history(1).unwrap().is_empty());
    assert_eq!(repo.kept_locations().unwrap(), [r"C:\fixture\web\dist"], "keeps survive");
    let db = Connection::open(&path).unwrap();
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), SCHEMA_VERSION);
    for bad in [
        "INSERT INTO agent_runs(project_id,harness,state,started_at,updated_at) VALUES(1,'x','expired',0,0)",
        "INSERT INTO run_artifacts(run_id,kind,location,created_at) VALUES(1,'docker','',0)",
    ] {
        assert!(db.execute(bad, []).is_err(), "{bad}");
    }
}

#[test]
fn v9_runs_stay_held_and_no_database_is_monitored_after_the_upgrade() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inventory.sqlite3");
    {
        let db = Connection::open(&path).unwrap();
        for sql in [
            include_str!("../migrations/001_initial.sql"),
            include_str!("../migrations/002_inventory.sql"),
            include_str!("../migrations/003_package_evidence.sql"),
            include_str!("../migrations/004_user_scopes.sql"),
            include_str!("../migrations/005_recycle.sql"),
            include_str!("../migrations/006_cache_scopes.sql"),
            include_str!("../migrations/007_storage_analysis.sql"),
            include_str!("../migrations/008_cleanup_keeps.sql"),
            include_str!("../migrations/009_runs_docker_history.sql"),
        ] {
            db.execute_batch(sql).unwrap();
        }
        db.execute_batch(
            "INSERT INTO app_meta(key,value) VALUES('mode','local');
            INSERT INTO projects(name,path,origin) VALUES('web','C:\\fixture\\web','registered');
            INSERT INTO agent_runs(project_id,harness,state,started_at,updated_at,finished_at)
                VALUES(1,'claude-code','succeeded',1,2,2);
            INSERT INTO run_artifacts(run_id,kind,location,created_at) VALUES(1,'scratch','C:\\fixture\\web\\tmp',1);",
        )
        .unwrap();
        db.pragma_update(None, "user_version", 9).unwrap();
    }
    let mut repo = Repository::open_path(&path, false, Arc::new(FixedClock::new(1_000))).unwrap();
    let runs = repo.runs(None, 10, 10).unwrap();
    assert_eq!((runs.len(), runs[0].released_at), (1, None), "an old run is not released by the upgrade");
    assert_eq!(repo.artifact_locations().unwrap().len(), 1, "its artifact stays held");
    assert!(repo.databases().unwrap().is_empty());
    let db = Connection::open(&path).unwrap();
    db.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), SCHEMA_VERSION);
    for bad in [
        "INSERT INTO monitored_databases(name,engine,location,registered_at) VALUES('x','oracle','C:\\x',0)",
        "INSERT INTO monitored_databases(name,engine,location,registered_at) VALUES('x','sqlite','',0)",
        "INSERT INTO monitored_databases(name,engine,docker_volume,registered_at) VALUES('x','sqlite','v',0)",
        "INSERT INTO monitored_databases(name,engine,location,registered_at,project_id) VALUES('x','sqlite','C:\\x',0,9)",
    ] {
        assert!(db.execute(bad, []).is_err(), "{bad}");
    }
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
