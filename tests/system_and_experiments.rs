//! Schema v4 user realm, stuck experiments, search, export and settings.
//! Synthetic metadata in temporary folders only; nothing is started.
#![cfg(windows)]

mod common;

use common::*;
use mogumogu::Error;
use mogumogu::clock::{DAY, FixedClock};
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::Core;
use mogumogu::storage::{Repository, SCHEMA_VERSION};
use mogumogu::updates::NoNetwork;
use rusqlite::Connection;
use std::sync::Arc;

fn core_at(temp: &std::path::Path, clock: Arc<FixedClock>) -> Core {
    let config = Config::new(Some(temp.join("data")), false).unwrap();
    Core::open_with(config, clock, Box::new(NoNetwork)).unwrap()
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
fn stuck_sessions_are_named_and_released_only_without_live_processes() {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(FixedClock::new(1_800_000_000));
    let mut core = core_at(temp.path(), clock.clone());
    let project = core.register_project("app", r"C:\fixture\app").unwrap();
    let scratch = core.register_scratch(project, "spike", r"C:\fixture\app\spike").unwrap();
    let silent = core.session_register(scratch, "Integration ohne Prozess", None).unwrap();
    assert!(core.review().unwrap().iter().all(|h| !h.trigger.contains("Lebenszeichen")));
    assert!(matches!(core.session_release(silent.id, "noch zu früh geprüft"), Err(Error::Blocked(_))));
    clock.advance(SESSION_SILENT_AFTER + 1);
    let hints = core.review().unwrap();
    let hint = hints.iter().find(|h| h.resource_id == scratch).expect("stuck session becomes a hint");
    assert!(hint.trigger.contains("ohne Lebenszeichen"), "{}", hint.trigger);
    assert!(hint.uncertainty.contains("beendet keine Prozesse"));
    assert!(matches!(core.session_release(silent.id, "kurz"), Err(Error::Invalid(_))));
    let released = core.session_release(silent.id, "Integration abgestürzt, results geprüft").unwrap();
    assert_eq!(released.state, SessionState::ReleasedAfterReview);
    assert!(released.note.unwrap_or_default().contains("results geprüft"));

    // The test process itself is alive: quiet or not, its session stays locked.
    let alive = core.session_register(scratch, "Eigener Prozess", Some(std::process::id())).unwrap();
    clock.advance(2 * DAY);
    assert_eq!(core.repository().session(alive.id).unwrap().stall(clock_now(&clock)), Some(Stall::LongRunning));
    assert!(matches!(core.session_release(alive.id, "darf nicht freigegeben werden"), Err(Error::Blocked(_))));
    assert!(core.repository().session(alive.id).unwrap().state.locks_resources());
}

fn clock_now(clock: &FixedClock) -> i64 {
    use mogumogu::clock::Clock;
    clock.now()
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
fn export_writes_a_new_redacted_file_and_never_overwrites() {
    let temp = tempfile::tempdir().unwrap();
    let mut core = core_at(temp.path(), Arc::new(FixedClock::new(1_800_000_000)));
    core.register_project("geheimprojekt", r"C:\Users\alice\geheim").unwrap();
    let target = temp.path().join("export.json");
    core.export_to(&path_text(&target), false).unwrap();
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(text.contains("mogumogu-export"));
    for secret in ["geheimprojekt", "alice"] {
        assert!(!text.contains(secret), "{secret}");
    }
    assert!(matches!(core.export_to(&path_text(&target), true), Err(Error::Conflict(_))));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), text, "unchanged");
}

#[test]
fn new_settings_are_validated_and_a_pause_keeps_the_choice() {
    let temp = tempfile::tempdir().unwrap();
    let clock = Arc::new(FixedClock::new(1_800_000_000));
    let mut core = core_at(temp.path(), clock.clone());
    let mut settings = core.settings().unwrap();
    assert!(settings.guide_enabled, "guide is on for a new inventory");
    settings.quiet_hours = Some(QuietHours { from: 8, to: 8 });
    assert!(matches!(core.save_settings(&settings), Err(Error::Invalid(_))));
    settings.quiet_hours = Some(QuietHours { from: 19, to: 8 });
    settings.summary_hour = 24;
    assert!(matches!(core.save_settings(&settings), Err(Error::Invalid(_))));
    settings.summary_hour = 17;
    settings.monitoring_enabled = true;
    settings.monitoring_paused_until = Some(1_800_000_000 + 3_600);
    core.save_settings(&settings).unwrap();
    let stored = core.settings().unwrap();
    assert!(!stored.monitoring_active(1_800_000_000));
    assert!(stored.monitoring_active(1_800_000_000 + 3_600) && stored.monitoring_enabled);
}
