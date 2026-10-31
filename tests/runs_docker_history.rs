//! Agent runs registered on request, the read-only Docker inventory against
//! a fake engine on a named pipe of this test, and the saved tidy-up
//! history. No real Docker installation and no recycle bin are touched.
#![cfg(windows)]
mod common;

use common::{TestBin, fake_engine, path_text, write};
use mogumogu::Error;
use mogumogu::cleanup::{DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT};
use mogumogu::clock::{DAY, FixedClock};
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::{ArtifactRequest, Core, ProjectRef};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const NOW: i64 = 1_800_000_000;
struct Env {
    temp: tempfile::TempDir,
    core: Core,
    clock: Arc<FixedClock>,
    project: PathBuf,
    project_id: i64,
}

/// An approved project, inside a registered disposable test root.
fn env() -> Env {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("disposable");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(DISPOSABLE_MARKER), DISPOSABLE_MARKER_TEXT).unwrap();
    let project = root.join("app");
    write(&project, "package.json", "{}");
    write(&project, "package-lock.json", "{}");
    write(&project, "node_modules/x/index.js", &"x".repeat(1_000));
    write(&project, "tool/Cargo.toml", "[package]");
    write(&project, "tool/target/debug/tool.exe", "binary");
    let clock = Arc::new(FixedClock::new(NOW));
    let config = Config::new(Some(temp.path().join("data")), false).unwrap();
    let mut core = Core::open_with(config, clock.clone()).unwrap();
    core.use_recycler(Box::new(TestBin::new(temp.path().join("papierkorb"))));
    core.register_disposable_root(&path_text(&root)).unwrap();
    let project_id = core.register_project("app", &path_text(&project)).unwrap();
    core.approve_project(project_id).unwrap();
    Env { temp, core, clock, project, project_id }
}

/// The client configuration points at `pipe`; no contexts besides `default`
/// unless the test writes them below the fake profile.
fn point_docker_at(env: &mut Env, pipe: &str) {
    let host = format!("npipe:////./pipe/{pipe}");
    let home = path_text(&env.temp.path().join("home"));
    env.core.use_env(move |key| match key {
        "DOCKER_HOST" => Some(host.clone()),
        "USERPROFILE" => Some(home.clone()),
        _ => None,
    });
}

/// The real place of a folder, as Docker would report a bind source.
fn real(path: &Path) -> String {
    path_text(&std::fs::canonicalize(path).unwrap())
}

#[test]
fn runs_are_refused_until_switched_on_and_only_add_protection() {
    let mut env = env();
    write(&env.project, "agent-work/package.json", "{}");
    write(&env.project, "agent-work/package-lock.json", "{}");
    write(&env.project, "agent-work/node_modules/y/index.js", "1");
    write(&env.project, ".playwright-mcp/page.png", "png");
    let here = ProjectRef::Path(path_text(&env.project.join("agent-work")));
    let refused = env.core.start_run(&here, "claude-code", None);
    assert!(matches!(refused, Err(Error::Blocked(_))), "off until the person asks: {refused:?}");
    assert!(env.core.allow_agent_runs(true).unwrap().allowed);
    let run = env.core.start_run(&here, "claude-code", Some("Fehler suchen")).unwrap();
    assert_eq!((run.project_id, run.state), (env.project_id, RunState::Running), "innermost project of the folder");
    let artifact =
        |kind, path: &Path| ArtifactRequest { kind, path: Some(path_text(path)), docker: None, purpose: None };
    env.core.add_run_artifact(run.id, &artifact(ArtifactKind::Workspace, &env.project.join("agent-work"))).unwrap();
    env.core
        .add_run_artifact(run.id, &artifact(ArtifactKind::Diagnostic, &env.project.join(".playwright-mcp")))
        .unwrap();
    let volume = ArtifactRequest {
        kind: ArtifactKind::Docker,
        path: None,
        docker: Some(("default".into(), DockerObject::Volume, "pgdata".into())),
        purpose: Some("Testdatenbank".into()),
    };
    let run = env.core.add_run_artifact(run.id, &volume).unwrap();
    assert_eq!(run.artifacts.len(), 3);
    for outside in [env.temp.path().join("elsewhere"), env.project.clone()] {
        let result = env.core.add_run_artifact(run.id, &artifact(ArtifactKind::Scratch, &outside));
        assert!(matches!(result, Err(Error::Invalid(_))), "{outside:?}: {result:?}");
    }
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    let items = &report.projects[0].items;
    let item = |rel: &str| items.iter().find(|i| i.rel_path == rel).unwrap_or_else(|| panic!("{rel}: {items:#?}"));
    let work = item("agent-work/node_modules");
    assert_eq!(work.class, TidyClass::Review);
    assert!(work.evidence.contains("gehört zu Lauf #1 claude-code (läuft) (Arbeitsordner)"), "{}", work.evidence);
    let recording = item(".playwright-mcp");
    assert!(recording.evidence.contains("registriert, gehört zu Lauf #1"), "provenance: {}", recording.evidence);
    assert!(!recording.evidence.contains("nur am Namen"), "{}", recording.evidence);
    assert_eq!(item("node_modules").class, TidyClass::Regenerable, "the project's own output stays recoverable");
    assert_eq!(report.projects[0].runs.len(), 1);
    let asked = env.core.cleanup_plan(env.project_id, &["agent-work/node_modules".into()]);
    assert!(matches!(asked, Err(Error::Blocked(_))), "{asked:?}");
    let finished = env.core.set_run_state(run.id, RunState::Failed, Some("Test rot")).unwrap();
    assert!(finished.finished_at.is_some());
    assert!(matches!(env.core.set_run_state(run.id, RunState::Running, None), Err(Error::Conflict(_))));
    let overview = env.core.run_overview(Some(env.project_id)).unwrap();
    assert_eq!((overview.runs.len(), overview.runs[0].state), (1, RunState::Failed));
    env.core.allow_agent_runs(false).unwrap();
    let late = env.core.add_run_artifact(run.id, &artifact(ArtifactKind::Output, &env.project.join("out")));
    assert!(matches!(late, Err(Error::Blocked(_))));
    assert!(env.project.join("agent-work/node_modules/y/index.js").exists(), "nothing was removed");
}

fn container(name: &str, mounts: Value) -> Value {
    json!({ "Id": "abcdef0123456789", "Names": [format!("/{name}")], "Image": "node:22", "State": "running",
            "Status": "Up 2 hours", "Labels": {}, "Mounts": mounts })
}

fn bind(source: &str) -> Value {
    json!([{ "Type": "bind", "Source": source, "Destination": "/data" }])
}

fn no_volumes() -> (&'static str, Value) {
    ("/volumes", json!({ "Volumes": [], "Warnings": null }))
}

#[test]
fn folders_a_container_binds_in_stay_and_an_unreachable_engine_blocks() {
    let mut env = env();
    let modules = real(&env.project.join("node_modules"));
    let engine =
        fake_engine(vec![("/containers/json?all=1", json!([container("web-1", bind(&modules))])), no_volumes()]);
    point_docker_at(&mut env, &engine.pipe);
    env.core.approve_docker("default").unwrap();
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    let held = report.projects[0].items.iter().find(|i| i.rel_path == "node_modules").unwrap();
    assert_eq!(held.class, TidyClass::Review);
    assert!(held.evidence.contains("Container „web-1“ (running) bindet ihn ein"), "{}", held.evidence);
    let plan = env.core.cleanup_plan(env.project_id, &[]).unwrap();
    assert_eq!(plan.targets.iter().map(|t| t.rel_path.as_str()).collect::<Vec<_>>(), ["tool/target"]);
    let plan = env.core.cleanup_approve(plan.id, plan.short_fingerprint()).unwrap();
    // The engine goes away before the move: its bind mounts are unknown now.
    point_docker_at(&mut env, "mogumogu-test-engine-absent");
    env.core.approve_docker("default").unwrap();
    let result = env.core.cleanup_apply(plan.id);
    assert!(
        matches!(&result, Err(Error::Blocked(note)) if note.contains("Docker-Einbindungen nicht prüfbar")),
        "{result:?}"
    );
    assert!(env.project.join("tool/target").is_dir() && env.project.join("node_modules").is_dir());
    // The report says the same as the plan: nothing is recoverable while it is unknown.
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    let target = report.projects[0].items.iter().find(|i| i.rel_path == "tool/target").unwrap();
    assert_eq!(target.class, TidyClass::Review);
    assert!(target.evidence.contains("Docker läuft nicht"), "{}", target.evidence);
    assert!(matches!(env.core.cleanup_plan(env.project_id, &[]), Err(Error::Blocked(_))));
}

#[test]
fn a_bind_counts_under_the_spelling_docker_reports_and_through_volumes() {
    let mut env = env();
    // As registered: %TEMP% is often an 8.3 alias of the profile (`LONGNA~1`).
    let alias = path_text(&env.project.join("node_modules"));
    let engine = fake_engine(vec![
        ("/containers/json?all=1", json!([container("web-1", bind(&alias.replace('\\', "/")))])),
        // A named volume whose driver binds a host folder.
        (
            "/volumes",
            json!({ "Volumes": [{ "Name": "tooldata", "Driver": "local",
            "Options": { "type": "none", "o": "bind", "device": path_text(&env.project.join("tool")) } }] }),
        ),
    ]);
    point_docker_at(&mut env, &engine.pipe);
    env.core.approve_docker("default").unwrap();
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    for (rel, reason) in [("node_modules", "Container „web-1“"), ("tool/target", "Volume „tooldata“")] {
        let item = report.projects[0].items.iter().find(|i| i.rel_path == rel).unwrap();
        assert_eq!(item.class, TidyClass::Review, "{rel}: {}", item.evidence);
        assert!(item.evidence.contains(reason), "{rel}: {}", item.evidence);
    }
    assert!(matches!(env.core.cleanup_plan(env.project_id, &[]), Err(Error::Blocked(_))), "nothing recoverable");
}

#[test]
fn an_engine_that_answers_too_much_or_oddly_leaves_binds_unknown() {
    let mut env = env();
    let many: Vec<Value> = (0..5_001).map(|i| container(&format!("c{i}"), json!([]))).collect();
    let crowded = fake_engine(vec![("/containers/json?all=1", Value::Array(many)), no_volumes()]);
    point_docker_at(&mut env, &crowded.pipe);
    env.core.approve_docker("default").unwrap();
    let plan = env.core.cleanup_plan(env.project_id, &[]);
    assert!(matches!(&plan, Err(Error::Blocked(_))), "a cut list proves nothing: {plan:?}");
    // An answer of the wrong shape must neither panic nor count as "no binds".
    let odd = fake_engine(vec![
        ("/version", json!({ "Version": "1" })),
        ("/containers/json?all=1", json!([])),
        ("/volumes", json!([])),
        ("/images/json", json!([])),
        ("/networks", json!([])),
    ]);
    point_docker_at(&mut env, &odd.pipe);
    env.core.approve_docker("default").unwrap();
    let inventory = env.core.docker_inventory(None, false).unwrap();
    assert!(inventory.volumes.is_empty(), "an odd volume list shows as empty, no panic");
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    assert!(report.projects[0].note.contains("unerwartete Antwort"), "{}", report.projects[0].note);
    assert_eq!(report.projects[0].regenerable, 0);
}

#[test]
fn a_context_that_changed_is_neither_asked_nor_trusted_for_cleanup() {
    let mut env = env();
    let engine = fake_engine(vec![("/containers/json?all=1", json!([])), no_volumes()]);
    point_docker_at(&mut env, &engine.pipe);
    env.core.approve_docker("default").unwrap();
    assert!(env.core.cleanup_plan(env.project_id, &[]).unwrap().executable());
    let home = path_text(&env.temp.path().join("home"));
    env.core.use_env(move |key| match key {
        "DOCKER_HOST" => Some("tcp://10.0.0.5:2376".into()),
        "USERPROFILE" => Some(home.clone()),
        _ => None,
    });
    let asked = engine.requests.lock().unwrap().len();
    let overview = env.core.docker_overview().unwrap();
    let default = overview.contexts.iter().find(|c| c.name == "default").unwrap();
    assert!(default.approved && default.changed, "{default:?}");
    assert!(matches!(env.core.docker_inventory(None, false), Err(Error::Blocked(_))));
    assert!(matches!(env.core.cleanup_plan(env.project_id, &[]), Err(Error::Blocked(_))));
    assert_eq!(engine.requests.lock().unwrap().len(), asked, "the old pipe is not asked any more");
}

#[test]
fn a_finished_run_is_released_once_and_its_artifacts_are_no_longer_held() {
    let mut env = env();
    write(&env.project, "agent-work/package.json", "{}");
    write(&env.project, "agent-work/package-lock.json", "{}");
    write(&env.project, "agent-work/node_modules/y/index.js", "1");
    env.core.allow_agent_runs(true).unwrap();
    let run = env.core.start_run(&ProjectRef::Id(env.project_id), "claude-code", None).unwrap();
    let work = ArtifactRequest {
        kind: ArtifactKind::Workspace,
        path: Some(path_text(&env.project.join("agent-work"))),
        docker: None,
        purpose: None,
    };
    env.core.add_run_artifact(run.id, &work).unwrap();
    assert!(matches!(env.core.release_run(run.id), Err(Error::Conflict(_))), "a running run stays held");
    env.core.set_run_state(run.id, RunState::Succeeded, None).unwrap();
    let held = |env: &mut Env| {
        let report = env.core.tidy_report(Some(env.project_id)).unwrap();
        let item = report.projects[0].items.iter().find(|i| i.rel_path == "agent-work/node_modules").cloned().unwrap();
        (item.class, item.evidence.contains("gehört zu Lauf"))
    };
    assert_eq!(held(&mut env), (TidyClass::Review, true), "finished, but not released yet");
    env.clock.advance(DAY);
    let overview = env.core.release_run(run.id).unwrap();
    assert_eq!(overview.runs[0].released_at, Some(NOW + DAY));
    assert!(matches!(env.core.release_run(run.id), Err(Error::Conflict(_))), "released once");
    let late = env.core.add_run_artifact(run.id, &work);
    assert!(matches!(late, Err(Error::Conflict(_))), "a released run holds nothing new either: {late:?}");
    assert_eq!(held(&mut env), (TidyClass::Regenerable, false));
    let plan = env.core.cleanup_plan(env.project_id, &["agent-work/node_modules".into()]).unwrap();
    assert_eq!(plan.targets.len(), 1, "the person can now choose it like any other output");
    assert!(env.project.join("agent-work/node_modules/y/index.js").exists(), "releasing removes nothing");
}

#[test]
fn artifacts_registered_before_the_approval_stay_held_after_it() {
    let mut env = env();
    let other = env.temp.path().join("disposable").join("second");
    write(&other, "package.json", "{}");
    write(&other, "package-lock.json", "{}");
    write(&other, "agent-work/package.json", "{}");
    write(&other, "agent-work/package-lock.json", "{}");
    write(&other, "agent-work/node_modules/y/index.js", "1");
    let id = env.core.register_project("second", &path_text(&other)).unwrap();
    env.core.allow_agent_runs(true).unwrap();
    let run = env.core.start_run(&ProjectRef::Id(id), "claude-code", None).unwrap();
    let work = ArtifactRequest {
        kind: ArtifactKind::Workspace,
        path: Some(path_text(&other.join("agent-work"))),
        docker: None,
        purpose: None,
    };
    env.core.add_run_artifact(run.id, &work).unwrap();
    env.core.approve_project(id).unwrap();
    let report = env.core.tidy_report(Some(id)).unwrap();
    let item = report.projects[0].items.iter().find(|i| i.rel_path == "agent-work/node_modules").unwrap();
    assert_eq!(item.class, TidyClass::Review, "{}", item.evidence);
    assert!(item.evidence.contains("gehört zu Lauf"), "{}", item.evidence);
}

#[test]
fn only_the_person_switches_runs_on() {
    let mut env = env();
    let mut settings = env.core.settings().unwrap();
    settings.agent_runs = true;
    env.core.save_settings(&settings).unwrap();
    assert!(!env.core.settings().unwrap().agent_runs, "a settings write does not switch it");
    let data = env.temp.path().join("owner-data");
    let _owner = common::owner::start_owner(&data);
    let on = common::owner::cli(&data, &["run", "allow", "on"]);
    assert!(!on.status.success(), "{}", String::from_utf8_lossy(&on.stdout));
    let client = mogumogu::ipc::Client::new(&Config::new(Some(data.clone()), false).unwrap()).unwrap();
    let raw = client.call(mogumogu::service::Request::RunsAllow { allowed: true });
    assert!(matches!(raw, Err(Error::Blocked(_))), "not over the pipe either: {raw:?}");
    let off = common::owner::cli(&data, &["run", "allow", "off"]);
    assert!(off.status.success(), "{}", String::from_utf8_lossy(&off.stderr));
}

#[test]
fn the_inventory_asks_only_fixed_read_only_questions() {
    let mut env = env();
    env.core.allow_agent_runs(true).unwrap();
    let run = env.core.start_run(&ProjectRef::Id(env.project_id), "test-harness", None).unwrap();
    let db = real(&env.project.join("tool"));
    let engine = fake_engine(vec![
        (
            "/version",
            json!({ "Version": "29.0.0", "ApiVersion": "1.52", "Os": "linux", "Arch": "amd64",
                             "Platform": { "Name": "Docker Engine - Community" } }),
        ),
        (
            "/containers/json?all=1",
            json!([{ "Id": "0123456789abcdef", "Names": ["/app-db-1"], "Image": "postgres:17",
            "State": "exited", "Status": "Exited (0) 3 days ago",
            "Labels": { "com.docker.compose.project": "app", "io.mogumogu.run": run.id.to_string() },
            "Mounts": [{ "Type": "bind", "Source": db, "Destination": "/src" },
                       { "Type": "volume", "Name": "pgdata", "Source": "/var/lib/docker/volumes/pgdata/_data",
                         "Destination": "/var/lib/postgresql/data" }] }]),
        ),
        (
            "/images/json",
            json!([{ "Id": "sha256:feedface00112233", "RepoTags": ["postgres:17"], "Size": 450_000_000,
                                  "Containers": 1, "Labels": null }]),
        ),
        ("/volumes", json!({ "Volumes": [{ "Name": "pgdata", "Driver": "local", "Labels": null }], "Warnings": null })),
        (
            "/networks",
            json!([{ "Name": "bridge", "Id": "1", "Driver": "bridge", "Labels": {} },
                             { "Name": "app_default", "Id": "2", "Driver": "bridge", "Labels": {} }]),
        ),
        (
            "/system/df",
            json!({ "LayersSize": 900_000_000, "Containers": [{ "SizeRw": 1_000 }],
            "Volumes": [{ "Name": "pgdata", "UsageData": { "Size": 52_000_000, "RefCount": 1 } }],
            "BuildCache": [{ "Size": 3_000 }, { "Size": 4_000 }] }),
        ),
    ]);
    point_docker_at(&mut env, &engine.pipe);
    assert!(matches!(env.core.docker_inventory(None, false), Err(Error::Blocked(_))), "nothing approved yet");
    let overview = env.core.approve_docker("default").unwrap();
    assert!(overview.contexts.iter().any(|c| c.name == "default" && c.approved));
    let inventory = env.core.docker_inventory(None, true).unwrap();
    assert!(inventory.engine.contains("29.0.0") && inventory.engine.contains("API 1.52"), "{}", inventory.engine);
    let container = &inventory.containers[0];
    assert_eq!(
        (container.name.as_str(), container.run, container.compose.as_deref()),
        ("app-db-1", Some(run.id), Some("app"))
    );
    assert_eq!(container.mounts[0].project.as_deref(), Some("app"), "{:#?}", container.mounts);
    assert_eq!((inventory.volumes[0].bytes, inventory.volumes[0].containers), (Some(52_000_000), Some(1)));
    assert_eq!(
        inventory.usage,
        Some(DockerUsage { layers: 900_000_000, containers: 1_000, volumes: 52_000_000, build_cache: 7_000 })
    );
    assert_eq!(inventory.networks.iter().filter(|n| !n.builtin).count(), 1);
    let allowed = ["/version", "/containers/json?all=1", "/images/json", "/volumes", "/networks", "/system/df"];
    for line in engine.requests.lock().unwrap().iter() {
        let (method, rest) = line.split_once(' ').unwrap();
        let path = rest.split(' ').next().unwrap();
        assert!(method == "GET" && allowed.contains(&path), "{line}");
    }
}

#[test]
fn only_local_contexts_can_be_approved_and_a_moved_one_must_be_approved_again() {
    let mut env = env();
    let home = env.temp.path().join("home");
    write(
        &home,
        ".docker/contexts/meta/0f/meta.json",
        r#"{"Name":"build","Endpoints":{"docker":{"Host":"tcp://10.0.0.5:2376"}}}"#,
    );
    write(&home, ".docker/config.json", r#"{"auths":{"registry.example":{"auth":"c2VjcmV0"}}}"#);
    let first = fake_engine(vec![]);
    point_docker_at(&mut env, &first.pipe);
    let overview = env.core.docker_overview().unwrap();
    let build = overview.contexts.iter().find(|c| c.name == "build").unwrap();
    assert_eq!(build.pipe, None);
    assert!(!build.note.is_empty());
    assert!(matches!(env.core.approve_docker("build"), Err(Error::Blocked(_))), "a remote engine is never asked");
    assert!(matches!(env.core.approve_docker("nowhere"), Err(Error::NotFound(_))));
    env.core.approve_docker("default").unwrap();
    let second = fake_engine(vec![]);
    point_docker_at(&mut env, &second.pipe);
    let moved = env.core.docker_inventory(Some("default"), false);
    assert!(matches!(&moved, Err(Error::Blocked(note)) if note.contains("erneut freigeben")), "{moved:?}");
    assert!(second.requests.lock().unwrap().is_empty() && first.requests.lock().unwrap().is_empty());
    env.core.revoke_docker("default").unwrap();
    assert!(matches!(env.core.revoke_docker("default"), Err(Error::NotFound(_))));
}

#[test]
fn reports_are_saved_and_show_what_grows() {
    let mut env = env();
    env.core.tidy_report(Some(env.project_id)).unwrap();
    write(&env.project, "node_modules/big/index.js", &"y".repeat(5_000));
    env.clock.advance(2 * DAY);
    let report = env.core.tidy_report(None).unwrap();
    let growth = report.projects[0].growth.clone().unwrap();
    assert_eq!((growth.reports, growth.regenerable), (2, 5_000));
    assert_eq!(growth.items.first().map(|(rel, grown)| (rel.as_str(), *grown)), Some(("node_modules", 5_000)));
    for _ in 0..35 {
        env.clock.advance(DAY);
        env.core.tidy_report(Some(env.project_id)).unwrap();
    }
    let (points, growth) = env.core.tidy_history(env.project_id).unwrap();
    assert_eq!(points.len(), 30, "the newest 30 per project");
    assert!(points.windows(2).all(|w| w[0].checked_at < w[1].checked_at), "oldest first");
    assert_eq!(growth.map(|g| g.regenerable), Some(0), "nothing grew in the last 30 reports");
}

#[test]
fn a_bind_through_a_junction_spelling_holds_the_real_folder() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    let project = real.join("app");
    write(&project, "package.json", "{}");
    write(&project, "package-lock.json", "{}");
    write(&project, "node_modules/x/index.js", "1");
    common::junction(&temp.path().join("link"), &real);
    let through = temp.path().join("link").join("app");
    let config = Config::new(Some(temp.path().join("data")), false).unwrap();
    let mut core = Core::open_with(config, Arc::new(FixedClock::new(NOW))).unwrap();
    let id = core.register_project("app", &path_text(&through)).unwrap();
    core.approve_project(id).unwrap();
    // Docker reports the spelling the client used: through the junction.
    let source = path_text(&through.join("node_modules"));
    let engine =
        fake_engine(vec![("/containers/json?all=1", json!([container("web-1", bind(&source))])), no_volumes()]);
    let host = format!("npipe:////./pipe/{}", engine.pipe);
    core.use_env(move |key| (key == "DOCKER_HOST").then(|| host.clone()));
    core.approve_docker("default").unwrap();
    let report = core.tidy_report(Some(id)).unwrap();
    let item = report.projects[0].items.iter().find(|i| i.rel_path == "node_modules").unwrap();
    assert_eq!(item.class, TidyClass::Review, "{}", item.evidence);
    assert!(item.evidence.contains("Container „web-1“"), "{}", item.evidence);
}

#[test]
fn rbind_volumes_missing_sources_and_empty_lists_of_older_engines() {
    let mut env = env();
    // A source below node_modules that does not exist, spelled as registered (often an 8.3 alias).
    let missing = path_text(&env.project.join("node_modules").join("gone").join("cache"));
    let engine = fake_engine(vec![
        ("/containers/json?all=1", json!([container("cache-1", bind(&missing)), container("idle-1", Value::Null)])),
        (
            "/volumes",
            json!({ "Volumes": [{ "Name": "tooldata", "Driver": "local",
            "Options": { "type": "none", "o": "rw,rbind", "device": path_text(&env.project.join("tool")) } }] }),
        ),
    ]);
    point_docker_at(&mut env, &engine.pipe);
    env.core.approve_docker("default").unwrap();
    let report = env.core.tidy_report(Some(env.project_id)).unwrap();
    assert!(report.projects[0].note.is_empty(), "null mounts are no mounts: {}", report.projects[0].note);
    for (rel, reason) in [("node_modules", "Container „cache-1“"), ("tool/target", "Volume „tooldata“")] {
        let item = report.projects[0].items.iter().find(|i| i.rel_path == rel).unwrap();
        assert_eq!(item.class, TidyClass::Review, "{rel}: {}", item.evidence);
        assert!(item.evidence.contains(reason), "{rel}: {}", item.evidence);
    }
    let empty = fake_engine(vec![("/containers/json?all=1", json!([])), ("/volumes", json!({ "Volumes": null }))]);
    point_docker_at(&mut env, &empty.pipe);
    env.core.approve_docker("default").unwrap();
    assert!(env.core.cleanup_plan(env.project_id, &[]).unwrap().executable(), "an engine with nothing blocks nothing");
}

/// A real engine: `MOGU_DOCKER_LIVE` names an empty folder, and a stopped
/// container binds its `disposable\app\node_modules` (e.g. `docker create
/// --pull never -v <folder>\disposable\app\node_modules:/work <local image>`).
/// `MOGU_DOCKER_CONTEXT` names the context (default `desktop-linux`). Only
/// reads the engine; the test's own recycle bin stays empty.
#[test]
#[ignore = "needs a local Docker engine and a prepared container; run explicitly"]
fn a_real_container_bind_holds_its_folder() {
    let Some(live) = std::env::var_os("MOGU_DOCKER_LIVE") else {
        println!("MOGU_DOCKER_LIVE nicht gesetzt; nichts geprüft.");
        return;
    };
    let context = std::env::var("MOGU_DOCKER_CONTEXT").unwrap_or_else(|_| "desktop-linux".into());
    let (temp, root) = (tempfile::tempdir().unwrap(), PathBuf::from(live).join("disposable"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(DISPOSABLE_MARKER), DISPOSABLE_MARKER_TEXT).unwrap();
    let project = root.join("app");
    write(&project, "package.json", "{}");
    write(&project, "package-lock.json", "{}");
    write(&project, "node_modules/x/index.js", "1");
    let mut core =
        Core::open_with(Config::new(Some(temp.path().join("data")), false).unwrap(), Arc::new(FixedClock::new(NOW)))
            .unwrap();
    let bin = TestBin::new(temp.path().join("papierkorb"));
    core.use_recycler(Box::new(bin.clone()));
    core.register_disposable_root(&path_text(&root)).unwrap();
    let project_id = core.register_project("app", &path_text(&project)).unwrap();
    core.approve_project(project_id).unwrap();
    let before = core.tidy_report(Some(project_id)).unwrap();
    let modules = before.projects[0].items.iter().find(|i| i.rel_path == "node_modules").unwrap();
    assert_eq!(modules.class, TidyClass::Regenerable, "before the approval: {}", modules.evidence);
    core.approve_docker(&context).unwrap();
    let inventory = core.docker_inventory(Some(&context), false).unwrap();
    let mounts: Vec<&DockerMount> = inventory.containers.iter().flat_map(|c| &c.mounts).collect();
    let bound = mounts.iter().find(|m| m.project.as_deref() == Some("app"));
    let bound = bound.unwrap_or_else(|| panic!("no bind in the project among {} mounts", mounts.len()));
    println!("{} → {:?}", bound.source, bound.host);
    assert_eq!(bound.kind, "bind");
    let after = core.tidy_report(Some(project_id)).unwrap();
    let modules = after.projects[0].items.iter().find(|i| i.rel_path == "node_modules").unwrap();
    println!("{:?}: {}", modules.class, modules.evidence);
    assert_eq!(modules.class, TidyClass::Review);
    assert!(modules.evidence.contains("bindet ihn ein"), "{}", modules.evidence);
    let asked = core.cleanup_plan(project_id, &["node_modules".into()]);
    assert!(matches!(asked, Err(Error::Blocked(_))), "{asked:?}");
    assert_eq!(bin.count(), 0);
}
