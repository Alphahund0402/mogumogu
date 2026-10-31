//! Databases registered for monitoring: measured from metadata only, held
//! out of every cleanup plan, suggested from known places, project files and
//! Docker volumes. No database is opened or contacted; Docker is a fake
//! engine on a pipe of this test.
#![cfg(windows)]
mod common;

use common::{TestBin, fake_engine, path_text, write};
use mogumogu::Error;
use mogumogu::cleanup::{DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT};
use mogumogu::clock::{DAY, FixedClock};
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::{Core, Request};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

const NOW: i64 = 1_800_000_000;

struct Env {
    temp: tempfile::TempDir,
    core: Core,
    clock: Arc<FixedClock>,
    project: PathBuf,
    project_id: i64,
}

fn env() -> Env {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("disposable");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(DISPOSABLE_MARKER), DISPOSABLE_MARKER_TEXT).unwrap();
    let project = root.join("app");
    write(&project, "package.json", "{}");
    write(&project, "package-lock.json", "{}");
    write(&project, "node_modules/x/index.js", "1");
    write(&project, "tool/Cargo.toml", "[package]");
    write(&project, "tool/target/debug/tool.exe", "binary");
    let clock = Arc::new(FixedClock::new(NOW));
    let mut core = Core::open_with(Config::new(Some(temp.path().join("data")), false).unwrap(), clock.clone()).unwrap();
    core.use_recycler(Box::new(TestBin::new(temp.path().join("papierkorb"))));
    let home = path_text(&temp.path().join("home"));
    core.use_env(move |key| (key == "USERPROFILE").then(|| home.clone()));
    core.register_disposable_root(&path_text(&root)).unwrap();
    let project_id = core.register_project("app", &path_text(&project)).unwrap();
    core.approve_project(project_id).unwrap();
    Env { temp, core, clock, project, project_id }
}

fn path(place: &std::path::Path) -> DatabasePlace {
    DatabasePlace::Path(path_text(place))
}

#[test]
fn a_database_file_is_measured_with_its_companions_held_and_followed() {
    let mut env = env();
    let file = env.project.join("tool/target/state.sqlite");
    write(&env.project, "tool/target/state.sqlite", &"d".repeat(1_000));
    write(&env.project, "tool/target/state.sqlite-wal", &"w".repeat(500));
    write(&env.project, "tool/target/other.txt", &"o".repeat(9_000));
    let database = env.core.register_database(&path(&file), None, None).unwrap();
    assert_eq!((database.engine, database.project.as_deref()), (DatabaseEngine::Sqlite, Some("app")));
    let last = database.last.unwrap();
    assert_eq!((last.bytes, last.files), (Some(1_500), Some(2)), "the file and its WAL, nothing else");
    assert!(matches!(env.core.register_database(&path(&file), None, None), Err(Error::Conflict(_))));
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    let target = report.projects[0].items.iter().find(|i| i.rel_path == "tool/target").unwrap();
    assert_eq!(target.class, TidyClass::Review);
    assert!(target.evidence.contains("überwachte Datenbank „state.sqlite“"), "{}", target.evidence);
    let plan = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    assert_eq!(plan.targets.iter().map(|t| t.rel_path.as_str()).collect::<Vec<_>>(), ["node_modules"]);
    write(&env.project, "tool/target/state.sqlite-wal", &"w".repeat(4_500));
    env.clock.advance(3 * DAY);
    let overview = env.core.measure_databases(Some(database.id)).unwrap();
    let growth = overview.databases[0].growth.clone().unwrap();
    assert_eq!((growth.bytes, growth.measurements), (4_000, 2));
    let (_, sizes) = env.core.database_history(database.id).unwrap();
    assert_eq!(sizes.len(), 2);
    assert!(file.exists(), "nothing was opened or changed");
    env.core.unregister_database(database.id).unwrap();
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    let target = report.projects[0].items.iter().find(|i| i.rel_path == "tool/target").unwrap();
    assert_eq!(target.class, TidyClass::Regenerable, "the hold ends with the monitoring");
    assert!(file.exists(), "ending the monitoring leaves the database alone");
}

#[test]
fn a_data_folder_is_told_by_its_names_and_measured_with_full_reports() {
    let mut env = env();
    let data = env.temp.path().join("pg").join("16").join("data");
    write(&data, "PG_VERSION", "16\n");
    write(&data, "base/1/1259", &"p".repeat(2_000));
    let database = env.core.register_database(&path(&data), None, None).unwrap();
    assert_eq!((database.engine, database.name.as_str()), (DatabaseEngine::Postgres, "PostgreSQL 16"));
    assert_eq!(database.last.as_ref().and_then(|s| s.bytes), Some(2_003));
    assert_eq!(database.project, None, "outside every project");
    env.clock.advance(DAY);
    env.core.tidy_report(None).unwrap();
    assert_eq!(env.core.database_history(database.id).unwrap().1.len(), 2, "a full report measures it too");
    for refused in [env.temp.path().join("nowhere"), PathBuf::from(r"\\server\share\db")] {
        assert!(env.core.register_database(&path(&refused), None, None).is_err(), "{refused:?}");
    }
}

/// A junction on the way could lead the measurement anywhere, a share too:
/// registering refuses it, for the junction, a folder and a file below it.
#[test]
fn a_database_behind_a_junction_is_refused() {
    let mut env = env();
    let real = env.temp.path().join("real");
    write(&real, "16/data/PG_VERSION", "16\n");
    write(&real, "app.db", "sqlite");
    let link = env.temp.path().join("link");
    common::junction(&link, &real);
    for refused in [link.clone(), link.join("16").join("data"), link.join("app.db")] {
        let result = env.core.register_database(&path(&refused), None, None);
        assert!(matches!(result, Err(Error::Blocked(_))), "{refused:?}: {result:?}");
    }
    assert!(env.core.database_overview().unwrap().databases.is_empty());
    let direct = env.core.register_database(&path(&real.join("app.db")), None, None).unwrap();
    assert_eq!(direct.last.and_then(|s| s.bytes), Some(6), "the real place itself is fine");
}

#[test]
fn suggestions_come_from_known_folders_project_files_and_volumes() {
    let mut env = env();
    let program_files = env.temp.path().join("pf");
    write(&program_files, "PostgreSQL/17/data/PG_VERSION", "17\n");
    write(&env.project, "data/app.db", "sqlite");
    env.core.tidy_report(Some(env.project_id)).unwrap();
    let engine = fake_engine(vec![
        ("/containers/json?all=1", json!([])),
        (
            "/volumes",
            json!({ "Volumes": [{ "Name": "web_pgdata", "Driver": "local" }, { "Name": "cache", "Driver": "local" }] }),
        ),
        (
            "/system/df",
            json!({ "LayersSize": 0, "Volumes": [{ "Name": "web_pgdata", "UsageData": { "Size": 52_000_000, "RefCount": 1 } }] }),
        ),
    ]);
    let host = format!("npipe:////./pipe/{}", engine.pipe);
    let (files, home) = (path_text(&program_files), path_text(&env.temp.path().join("home")));
    env.core.use_env(move |key| match key {
        "ProgramFiles" => Some(files.clone()),
        "USERPROFILE" => Some(home.clone()),
        "DOCKER_HOST" => Some(host.clone()),
        _ => None,
    });
    env.core.approve_docker("default").unwrap();
    let overview = env.core.database_overview().unwrap();
    let names: Vec<(&str, DatabaseEngine)> = overview.suggestions.iter().map(|s| (s.name.as_str(), s.engine)).collect();
    assert!(names.contains(&("app.db", DatabaseEngine::Sqlite)), "{names:?}");
    assert!(names.contains(&("web_pgdata", DatabaseEngine::Postgres)), "{names:?}");
    assert!(names.iter().any(|(n, e)| n.starts_with("PostgreSQL") && *e == DatabaseEngine::Postgres), "{names:?}");
    assert!(!names.iter().any(|(n, _)| *n == "cache"), "a volume without a hint is no suggestion");
    let volume = overview.suggestions.iter().find(|s| s.name == "web_pgdata").unwrap().place.clone();
    let database = env.core.register_database(&volume, None, None).unwrap();
    assert_eq!(database.last.and_then(|s| s.bytes), Some(52_000_000));
    assert!(
        env.core.database_overview().unwrap().suggestions.iter().all(|s| s.name != "web_pgdata"),
        "once registered"
    );
    let missing = DatabasePlace::Volume { context: "default".into(), volume: "nope".into() };
    assert!(matches!(env.core.register_database(&missing, None, None), Err(Error::NotFound(_))));
}

#[test]
fn ending_a_monitoring_or_a_runs_protection_is_the_persons_decision() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("owner-data");
    let _owner = common::owner::start_owner(&data);
    let client = mogumogu::ipc::Client::new(&Config::new(Some(data.clone()), false).unwrap()).unwrap();
    for request in [
        Request::DbUnregister { id: 1 },
        Request::RunRelease { run_id: 1 },
        Request::KeepFolder { project_id: 1, rel_path: "dist".into(), keep: false },
        Request::DockerRevoke { context: "default".into() },
    ] {
        let refused = client.call(request.clone());
        assert!(matches!(refused, Err(Error::Blocked(_))), "{request:?}: {refused:?}");
    }
    for args in [
        &["db", "unregister", "--id", "1"][..],
        &["run", "release", "--run", "1"],
        &["cleanup", "unkeep", "--project", "1", "--path", "dist"],
        &["docker", "revoke", "--context", "default"],
    ] {
        let out = common::owner::cli(&data, args);
        assert!(!out.status.success(), "{args:?}");
    }
}
