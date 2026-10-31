//! End-to-end pilot (G2/G3 scenarios): owner process + real CLI over the
//! named pipe, cleanup plans in a disposable test root. Everything happens
//! in temporary directories; the owner's recycle bin is a test directory.
#![cfg(windows)]

mod common;

use common::owner::{cli, err, ok, start_dashboard, start_owner};
use common::*;
use mogumogu::cleanup::{DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT};
use mogumogu::service::Request;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    _temp: tempfile::TempDir,
    data: PathBuf,
    root: PathBuf,
}

/// Data dir and a disposable root; the owner is started by each test.
fn fixture(data_suffix: &str) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join(format!("data-{data_suffix}"));
    let root = temp.path().join("disposable");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(DISPOSABLE_MARKER), DISPOSABLE_MARKER_TEXT).unwrap();
    Fixture { _temp: temp, data, root }
}

fn id(value: &Value, key: &str) -> i64 {
    value[key].as_i64().unwrap_or_else(|| panic!("{key} missing in {value}"))
}

/// Registers, approves and returns the project id.
fn approved_project(data: &Path, dir: &Path) -> String {
    let project = ok(cli(data, &["project", "register", "--name", "web", "--path", &path_text(dir)]));
    let project_id = id(&project, "project_id").to_string();
    ok(cli(data, &["project", "approve", "--project", &project_id]));
    project_id
}

#[test]
fn pilot_registration_scan_and_a_recycle_plan() {
    let f = fixture("pilot");
    let (dashboard, bin) = start_dashboard(&f.data);
    let data = f.data.as_path();
    ok(cli(data, &["cleanup", "test-root", "--path", &path_text(&f.root)]));
    let project_dir = sample_project(&f.root);

    // Registration is not a read approval.
    let project = ok(cli(data, &["project", "register", "--name", "web", "--path", &path_text(&project_dir)]));
    let project_id = id(&project, "project_id");
    assert_eq!(project["scanned"], false);
    let listed = ok(cli(data, &["list"]));
    assert_eq!(listed["projects"][0]["read_approved"], false);
    assert_eq!(listed["projects"][0]["packages"], Value::Null, "unknown, not zero");
    assert_eq!(err(cli(data, &["cleanup", "plan", "--project", &project_id.to_string()])).0, "blocked");

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
    write(
        &project_dir,
        "requirements.txt",
        "requests==2.32.3
flask==3.0.0
",
    );
    let changed = ok(cli(data, &["scan"]));
    assert_eq!(changed[0]["added"], 1, "{changed}");

    // The plan lists only recoverable folders with their evidence.
    let plan = ok(cli(data, &["cleanup", "plan", "--project", &project_id.to_string()]));
    let paths: Vec<&str> =
        plan["targets"].as_array().unwrap().iter().map(|t| t["rel_path"].as_str().unwrap()).collect();
    assert_eq!(paths.len(), 2, "{plan}");
    assert!(paths.contains(&"node_modules") && paths.contains(&"tool/target"), "{plan}");
    let plan_id = id(&plan, "id");
    let fingerprint = plan["fingerprint"].as_str().unwrap()[..12].to_string();

    // Binding and moving are the person's: neither the CLI nor anything else
    // on the pipe approves or runs a plan, even with the right fingerprint.
    let text = plan_id.to_string();
    let refused: [&[&str]; 2] =
        [&["cleanup", "approve", "--plan", &text, "--confirm", &fingerprint], &["cleanup", "apply", "--plan", &text]];
    for args in refused {
        assert_eq!(err(cli(data, args)), ("blocked".to_string(), 3), "{args:?}");
    }
    let config = mogumogu::config::Config::new(Some(f.data.clone()), false).unwrap();
    let client = mogumogu::ipc::Client::new(&config).unwrap();
    for request in
        [Request::CleanupApprove { plan_id, confirmation: fingerprint.clone() }, Request::CleanupApply { plan_id }]
    {
        let refused = client.call(request.clone());
        assert!(matches!(refused, Err(mogumogu::Error::Blocked(_))), "{request:?}: {refused:?}");
    }
    let listed = ok(cli(data, &["list"]));
    let stored = listed["plans"].as_array().unwrap().iter().find(|p| p["id"] == plan_id).unwrap().clone();
    assert_eq!(stored["state"], "draft", "{stored}");

    // In the dashboard a wrong confirmation is refused; the exact fingerprint approves.
    let approve = |plan_id, confirmation: &str| {
        dashboard.send(Request::CleanupApprove { plan_id, confirmation: confirmation.into() })
    };
    let apply = |plan_id| dashboard.send(Request::CleanupApply { plan_id });
    assert_eq!(approve(plan_id, "000000000000").unwrap_err(), "invalid");
    approve(plan_id, &fingerprint).unwrap();

    // The lockfile goes away: the evidence no longer holds, nothing moves.
    fs::remove_file(project_dir.join("package-lock.json")).unwrap();
    assert_eq!(apply(plan_id).unwrap_err(), "blocked");
    let after = ok(cli(data, &["list"]));
    let stored = after["plans"].as_array().unwrap().iter().find(|p| p["id"] == plan_id).unwrap().clone();
    assert_eq!(stored["state"], "invalidated", "{stored}");
    assert!(project_dir.join("node_modules/left-pad/index.js").is_file());
    assert!(project_dir.join("tool/target/debug/tool.exe").is_file());
    // A plan runs at most once.
    assert_eq!(apply(plan_id).unwrap_err(), "conflict");

    // With the lockfile back, a new plan moves both folders as a whole.
    write(&project_dir, "package-lock.json", "{}");
    let plan = ok(cli(data, &["cleanup", "plan", "--project", &project_id.to_string()]));
    let plan_id = id(&plan, "id");
    let fingerprint = plan["fingerprint"].as_str().unwrap()[..12].to_string();
    approve(plan_id, &fingerprint).unwrap();
    let applied = apply(plan_id).unwrap();
    assert_eq!(applied["state"], "completed", "{applied}");
    assert_eq!(bin.count(), 2);
    assert!(!project_dir.join("node_modules").exists() && !project_dir.join("tool/target").exists());
    assert!(project_dir.join("package.json").is_file() && project_dir.join("tool/Cargo.toml").is_file());
}

#[test]
fn folders_to_review_and_projects_outside_a_test_root_stay_out_of_reach() {
    let f = fixture("gate");
    let (dashboard, _) = start_dashboard(&f.data);
    let data = f.data.as_path();
    let outside = f.root.parent().unwrap().join("real");
    let project_dir = sample_project(&outside);
    let project_id = approved_project(data, &project_dir);
    let plan = ok(cli(data, &["cleanup", "plan", "--project", &project_id]));
    assert!(plan["targets"].to_string().contains("G5"), "{plan}");
    let plan_id = id(&plan, "id");
    let confirmation = plan["fingerprint"].as_str().unwrap()[..12].to_string();
    // Not even the dashboard gets past the gate.
    assert_eq!(dashboard.send(Request::CleanupApprove { plan_id, confirmation }).unwrap_err(), "blocked");
    let applied = dashboard.send(Request::CleanupApply { plan_id });
    assert_eq!(applied.unwrap_err(), "conflict", "a draft never runs");
    for path in [".venv", "package.json", ".claude"] {
        assert_eq!(
            err(cli(data, &["cleanup", "plan", "--project", &project_id, "--path", path])).0,
            "blocked",
            "{path}"
        );
    }
    assert!(project_dir.join("node_modules").is_dir() && project_dir.join(".venv").is_dir());
}

#[test]
fn second_owner_and_foreign_protocols_are_refused() {
    let f = fixture("owner");
    let owner = start_owner(&f.data);
    let config = mogumogu::config::Config::new(Some(f.data.clone()), false).unwrap();
    assert!(mogumogu::owner::Owner::start(config, common::owner::Quiet).is_err(), "no second writer");
    let (code, exit) = err(cli(&f.data, &["cleanup", "cancel", "--plan", "1", "--force"]));
    assert_eq!((code.as_str(), exit), ("invalid", 2));
    drop(owner);
    let (code, exit) = err(cli(&f.data, &["status"]));
    assert_eq!((code.as_str(), exit), ("unsupported", 6), "--no-start never spawns an owner");
}
