//! End-to-end pilot (G2/G3 scenarios): owner process + real CLI over the
//! named pipe, real managed runs in job objects, real cleanup in a
//! disposable test root. Everything happens in temporary directories.
#![cfg(windows)]

mod common;

use common::owner::{cli, cli_to_file, err, ok, start_owner};
use common::*;
use mogumogu::cleanup::{DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT, KEEP_MARKER};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

struct Fixture {
    _temp: tempfile::TempDir,
    data: std::path::PathBuf,
    root: std::path::PathBuf,
    project: std::path::PathBuf,
}

/// Data dir, a sample project and a disposable root that will hold the
/// scratch root. The owner is started by each test.
fn fixture(data_suffix: &str) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join(format!("data-{data_suffix}"));
    let root = temp.path().join("disposable");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(DISPOSABLE_MARKER), DISPOSABLE_MARKER_TEXT).unwrap();
    let project = sample_project(temp.path());
    Fixture { _temp: temp, data, root, project }
}

fn id(value: &Value, key: &str) -> i64 {
    value[key].as_i64().unwrap_or_else(|| panic!("{key} missing in {value}"))
}

fn wait_for_session(data: &Path, session_id: i64, state: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let sessions = ok(cli(data, &["sessions"]));
        let session = sessions.as_array().unwrap().iter().find(|s| s["id"] == session_id).cloned().unwrap();
        if session["state"] == state || Instant::now() > deadline {
            return session;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[test]
fn pilot_registration_scan_run_and_test_cleanup() {
    let f = fixture("pilot");
    let project_dir = f.project.clone();
    let _owner = start_owner(&f.data);
    let data = f.data.as_path();

    // Registration is not a read approval.
    let project = ok(cli(data, &["project", "register", "--name", "web", "--path", &path_text(&project_dir)]));
    let project_id = id(&project, "project_id");
    assert_eq!(project["scanned"], false);
    let listed = ok(cli(data, &["list"]));
    assert_eq!(listed["projects"][0]["read_approved"], false);
    assert_eq!(listed["projects"][0]["packages"], Value::Null, "unknown, not zero");

    // Approve, scan: npm project, Python environment and a skill are visible.
    let scope = ok(cli(data, &["project", "approve", "--project", &project_id.to_string()]));
    let reports = ok(cli(data, &["scan", "--scope", &id(&scope, "scope_id").to_string()]));
    assert_eq!(reports[0]["state"], "complete", "{reports}");
    assert_eq!(reports[0]["baseline"], true);
    let snapshot = ok(cli(data, &["list"]));
    assert!(snapshot["projects"][0]["packages"].as_i64().unwrap() >= 2);
    assert!(snapshot["ai"].as_array().unwrap().iter().any(|a| a["client"] == "Claude Code" && a["kind"] == "Skill"));
    assert!(snapshot["ecosystems"].as_array().unwrap().iter().any(|e| e["ecosystem"] == "npm" && e["installed"] == 1));

    // A second scan without changes: no event flood, no fake installs.
    let again = ok(cli(data, &["scan", "--scope", &id(&scope, "scope_id").to_string()]));
    assert_eq!((again[0]["baseline"].as_bool(), again[0]["added"].as_i64()), (Some(false), Some(0)));

    // A new dependency is a grouped change.
    write(&project_dir, "requirements.txt", "requests==2.32.3\nflask==3.0.0\n");
    let changed = ok(cli(data, &["scan"]));
    assert_eq!(changed[0]["added"], 1, "{changed}");

    // Managed scratchpad and a run whose output lands in temporary\.
    ok(cli(data, &["settings", "set", "scratch-root", &path_text(&f.root.join("scratch"))]));
    let scratch = ok(cli(data, &["scratch", "create", "--project", &project_id.to_string(), "--name", "parser spike"]));
    let scratch_id = id(&scratch, "resource_id");
    let run =
        ok(cli(data, &["run", "--scratch", &scratch_id.to_string(), "--", "cmd", "/C", "echo out>%TEMP%\\out.txt"]));
    assert_eq!(run["exit_code"], 0);
    let session = wait_for_session(data, id(&run, "session_id"), "completed_verified");
    assert_eq!(session["state"], "completed_verified", "{session}");
    let why = ok(cli(data, &["why", "--resource", &scratch_id.to_string()]));
    let temp_id =
        why["children"].as_array().unwrap().iter().find(|c| c["kind"] == "temporary").map(|c| id(c, "id")).unwrap();
    let temp_path = why["children"].as_array().unwrap().iter().find(|c| c["kind"] == "temporary").unwrap()["path"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(Path::new(&temp_path).join("out.txt").is_file());
    let results_path = why["children"].as_array().unwrap().iter().find(|c| c["kind"] == "results").unwrap()["path"]
        .as_str()
        .unwrap()
        .to_string();
    fs::write(Path::new(&results_path).join("report.txt"), "keep").unwrap();

    // Without explicit expendability or test root registration: blocked plan.
    let blocked = ok(cli(data, &["cleanup", "plan", "--resource", &temp_id.to_string()]));
    assert!(!blocked["targets"][0]["blockers"].as_array().unwrap().is_empty());
    assert_eq!(
        err(cli(data, &["cleanup", "approve", "--plan", &id(&blocked, "id").to_string(), "--confirm", "x"])).0,
        "blocked"
    );

    ok(cli(data, &["scratch", "expendable", "--resource", &temp_id.to_string()]));
    ok(cli(data, &["cleanup", "test-root", "--path", &path_text(&f.root)]));
    let plan = ok(cli(data, &["cleanup", "plan", "--resource", &temp_id.to_string()]));
    assert!(plan["targets"][0]["blockers"].as_array().unwrap().is_empty(), "{plan}");
    let plan_id = id(&plan, "id").to_string();
    let fingerprint = plan["fingerprint"].as_str().unwrap()[..12].to_string();

    // Wrong confirmation is refused; the exact fingerprint approves.
    assert_eq!(err(cli(data, &["cleanup", "approve", "--plan", &plan_id, "--confirm", "000000000000"])).0, "invalid");
    ok(cli(data, &["cleanup", "approve", "--plan", &plan_id, "--confirm", &fingerprint]));
    let applied = ok(cli(data, &["cleanup", "apply", "--plan", &plan_id]));
    assert_eq!(applied["state"], "completed", "{applied}");
    assert!(!Path::new(&temp_path).join("out.txt").exists());
    assert!(Path::new(&temp_path).is_dir(), "the temporary folder itself stays");
    assert!(Path::new(&results_path).join("report.txt").is_file(), "results are untouched");
    assert!(project_dir.join("package.json").is_file(), "sources are untouched");
    // A plan runs at most once.
    assert_eq!(err(cli(data, &["cleanup", "apply", "--plan", &plan_id])).0, "conflict");
}

/// Shared setup for cleanup negatives: managed scratch with completed run,
/// marked expendable, test root registered. Returns (temp path, temp id).
fn prepared(data: &Path, root: &Path, project_dir: &Path) -> (String, i64) {
    let project = ok(cli(data, &["project", "register", "--name", "p", "--path", &path_text(project_dir)]));
    ok(cli(data, &["settings", "set", "scratch-root", &path_text(&root.join("scratch"))]));
    let scratch =
        ok(cli(data, &["scratch", "create", "--project", &id(&project, "project_id").to_string(), "--name", "neg"]));
    let scratch_id = id(&scratch, "resource_id").to_string();
    let run = ok(cli(data, &["run", "--scratch", &scratch_id, "--", "cmd", "/C", "echo a>%TEMP%\\a.txt"]));
    wait_for_session(data, id(&run, "session_id"), "completed_verified");
    let why = ok(cli(data, &["why", "--resource", &scratch_id]));
    let temp = why["children"].as_array().unwrap().iter().find(|c| c["kind"] == "temporary").unwrap().clone();
    ok(cli(data, &["scratch", "expendable", "--resource", &id(&temp, "id").to_string()]));
    ok(cli(data, &["cleanup", "test-root", "--path", &path_text(root)]));
    (temp["path"].as_str().unwrap().to_string(), id(&temp, "id"))
}

fn approved_plan(data: &Path, temp_id: i64) -> String {
    let plan = ok(cli(data, &["cleanup", "plan", "--resource", &temp_id.to_string()]));
    assert!(plan["targets"][0]["blockers"].as_array().unwrap().is_empty(), "{plan}");
    let plan_id = id(&plan, "id").to_string();
    ok(cli(
        data,
        &["cleanup", "approve", "--plan", &plan_id, "--confirm", &plan["fingerprint"].as_str().unwrap()[..12]],
    ));
    plan_id
}

#[test]
fn new_file_after_approval_invalidates_the_plan() {
    let f = fixture("invalidate");
    let project_dir = f.project.clone();
    let _owner = start_owner(&f.data);
    let (temp, temp_id) = prepared(&f.data, &f.root, &project_dir);
    let plan_id = approved_plan(&f.data, temp_id);
    fs::write(Path::new(&temp).join("late.txt"), "new after approval").unwrap();
    assert_eq!(err(cli(&f.data, &["cleanup", "apply", "--plan", &plan_id])).0, "blocked");
    assert!(Path::new(&temp).join("a.txt").is_file(), "nothing removed");
    assert!(Path::new(&temp).join("late.txt").is_file());
}

#[test]
fn protected_child_and_links_block_the_whole_target() {
    let f = fixture("protected");
    let project_dir = f.project.clone();
    let _owner = start_owner(&f.data);
    let (temp, temp_id) = prepared(&f.data, &f.root, &project_dir);
    write(Path::new(&temp), &format!("keep\\{KEEP_MARKER}"), "");
    let plan = ok(cli(&f.data, &["cleanup", "plan", "--resource", &temp_id.to_string()]));
    assert!(plan["targets"][0]["blockers"].to_string().contains("geschütztes Kind"), "{plan}");
    fs::remove_dir_all(Path::new(&temp).join("keep")).unwrap();
    junction(&Path::new(&temp).join("link"), &project_dir);
    let plan = ok(cli(&f.data, &["cleanup", "plan", "--resource", &temp_id.to_string()]));
    assert!(plan["targets"][0]["blockers"].to_string().contains("Verknüpfung"), "{plan}");
    assert!(project_dir.join("package.json").is_file());
}

#[test]
fn outside_a_disposable_root_cleanup_stays_disabled() {
    let f = fixture("gate");
    let project_dir = f.project.clone();
    let _owner = start_owner(&f.data);
    let data = f.data.as_path();
    let elsewhere = f.root.parent().unwrap().join("real-scratch");
    let project = ok(cli(data, &["project", "register", "--name", "p", "--path", &path_text(&project_dir)]));
    ok(cli(data, &["settings", "set", "scratch-root", &path_text(&elsewhere)]));
    let scratch =
        ok(cli(data, &["scratch", "create", "--project", &id(&project, "project_id").to_string(), "--name", "prod"]));
    let scratch_id = id(&scratch, "resource_id").to_string();
    let run = ok(cli(data, &["run", "--scratch", &scratch_id, "--", "cmd", "/C", "echo a>%TEMP%\\a.txt"]));
    wait_for_session(data, id(&run, "session_id"), "completed_verified");
    let why = ok(cli(data, &["why", "--resource", &scratch_id]));
    let temp_id =
        why["children"].as_array().unwrap().iter().find(|c| c["kind"] == "temporary").map(|c| id(c, "id")).unwrap();
    ok(cli(data, &["scratch", "expendable", "--resource", &temp_id.to_string()]));
    let plan = ok(cli(data, &["cleanup", "plan", "--resource", &temp_id.to_string()]));
    assert!(plan["targets"][0]["blockers"].to_string().contains("G5"), "{plan}");
}

#[test]
fn starter_ends_but_worker_continues_keeps_the_lock() {
    let f = fixture("worker");
    let project_dir = f.project.clone();
    let _owner = start_owner(&f.data);
    let data = f.data.as_path();
    let project = ok(cli(data, &["project", "register", "--name", "p", "--path", &path_text(&project_dir)]));
    ok(cli(data, &["settings", "set", "scratch-root", &path_text(&f.root.join("scratch"))]));
    let scratch =
        ok(cli(data, &["scratch", "create", "--project", &id(&project, "project_id").to_string(), "--name", "bg"]));
    // cmd exits at once, the detached ping keeps running inside the job.
    // Output goes to a file: inherited pipe handles would otherwise make the
    // test wait for ping itself.
    let scratch_id = id(&scratch, "resource_id").to_string();
    let run = cli_to_file(
        data,
        &["run", "--scratch", &scratch_id, "--", "cmd", "/C", "start", "/B", "ping", "-n", "6", "127.0.0.1", ">NUL"],
    );
    assert!(run["remaining_processes"].as_i64().unwrap() >= 1, "{run}");
    let session_id = id(&run, "session_id");
    let early = wait_for_session(data, session_id, "completion_requested");
    assert_eq!(early["state"], "completion_requested", "worker still writing (AT-12)");
    assert_eq!(early["remaining_processes"], run["remaining_processes"]);
    let late = wait_for_session(data, session_id, "completed_verified");
    assert_eq!(late["state"], "completed_verified");
}

#[test]
fn second_owner_and_foreign_protocols_are_refused() {
    let f = fixture("owner");
    let owner = start_owner(&f.data);
    let config = mogumogu::config::Config::new(Some(f.data.clone()), false).unwrap();
    assert!(mogumogu::owner::Owner::start(config, common::owner::Quiet).is_err(), "no second writer");
    let (code, exit) = err(cli(&f.data, &["cleanup", "apply", "--plan", "1", "--force"]));
    assert_eq!((code.as_str(), exit), ("invalid", 2));
    drop(owner);
    let (code, exit) = err(cli(&f.data, &["status"]));
    assert_eq!((code.as_str(), exit), ("unsupported", 6), "--no-start never spawns an owner");
}
