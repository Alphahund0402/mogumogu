//! AI access per folder from settings files; nothing is started or changed.
#![cfg(windows)]
mod common;

use common::{path_text, write};
use mogumogu::clock::FixedClock;
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::Core;
use std::sync::Arc;

#[test]
fn settings_decide_how_far_ai_tools_reach_per_project() {
    let temp = tempfile::tempdir().unwrap();
    let wide = temp.path().join("wide");
    write(
        &wide,
        ".claude/settings.json",
        r#"{"permissions":{"defaultMode":"bypassPermissions","additionalDirectories":["C:\\Users\\x\\Documents"]}}"#,
    );
    write(&wide, "CLAUDE.md", "# Projekt");
    write(&wide, "package.json", "{}");
    write(&wide, "node_modules/dep/index.js", "1");
    let open = temp.path().join("open");
    write(&open, "AGENTS.md", "# Regeln");
    let limited = temp.path().join("limited");
    write(&limited, ".cursorignore", "node_modules/\n.env\n");
    write(&limited, ".cursor/rules/style.mdc", "---\nalwaysApply: true\n---\nKurz.");
    let mut core =
        Core::open_with(Config::new(Some(temp.path().join("data")), false).unwrap(), Arc::new(FixedClock::new(10)))
            .unwrap();
    for (name, path) in [("wide", &wide), ("open", &open), ("limited", &limited)] {
        let id = core.register_project(name, &path_text(path)).unwrap();
        let scope = core.approve_project(id).unwrap();
        core.scan(scope).unwrap();
    }
    let access = core.snapshot().unwrap().ai_access;
    let row = |name: &str| access.iter().find(|a| a.name == name).unwrap_or_else(|| panic!("{name}: {access:#?}"));
    let w = row("wide");
    assert_eq!(w.level, AccessLevel::Wide);
    assert!(w.findings.iter().any(|f| f.contains("bypassPermissions")), "{:?}", w.findings);
    assert!(w.findings.iter().any(|f| f.contains("außerhalb des Projekts")), "{:?}", w.findings);
    assert!(w.keep_out.iter().any(|k| k == "node_modules"), "{:?}", w.keep_out);
    assert!(!format!("{access:?}").contains("Documents"), "granted paths are not kept");
    assert_eq!(row("open").level, AccessLevel::Open);
    let l = row("limited");
    assert_eq!(l.level, AccessLevel::Limited);
    assert!(l.findings.iter().any(|f| f.contains(".cursorignore")), "{:?}", l.findings);
    assert_eq!(access[0].name, "wide", "wide reach comes first");
}

fn core_in(temp: &std::path::Path) -> Core {
    Core::open_with(Config::new(Some(temp.join("data")), false).unwrap(), Arc::new(FixedClock::new(10))).unwrap()
}

fn approved(core: &mut Core, name: &str, path: &std::path::Path) -> i64 {
    let id = core.register_project(name, &path_text(path)).unwrap();
    let scope = core.approve_project(id).unwrap();
    core.scan(scope).unwrap();
    scope
}

#[test]
fn a_context_filter_goes_into_the_personal_file_and_keeps_the_shared_one() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    write(&app, "package.json", "{}");
    write(&app, "node_modules/dep/index.js", "1");
    let shared = r#"{"permissions":{"deny":["Read(.env)"]}}"#;
    write(&app, ".claude/settings.json", shared);
    write(&app, ".claude/settings.local.json", r#"{"model":"opus","permissions":{"allow":["Bash(ls)"]}}"#);
    let mut core = core_in(temp.path());
    let scope = approved(&mut core, "app", &app);
    let plan = core.context_filter(scope).unwrap();
    let file = &plan.files[0];
    assert_eq!(file.rel_path, ".claude/settings.local.json");
    assert!(file.exists && file.kept == 0, "{file:?}");
    assert!(file.add.iter().any(|r| r == "Read(/node_modules/**)"), "{:?}", file.add);
    assert!(!file.add.iter().any(|r| r == "Read(.env)"), "a rule of the shared file is not repeated");
    assert!(!file.add.iter().any(|r| r.starts_with("Read(!")), "no exception that would lift earlier rules");
    assert!(plan.hints.iter().any(|h| h.contains("1 Verbote aus .claude/settings.json")), "{:?}", plan.hints);
    let local = app.join(".claude/settings.local.json");
    let before = std::fs::read_to_string(&local).unwrap();
    assert!(matches!(core.apply_context_filter(scope, "0000"), Err(mogumogu::Error::Conflict(_))));
    assert_eq!(std::fs::read_to_string(&local).unwrap(), before, "nothing without the preview");
    assert!(core.apply_context_filter(scope, &plan.fingerprint).unwrap().written);
    assert_eq!(std::fs::read_to_string(app.join(".claude/settings.json")).unwrap(), shared, "the team file stays");
    let after: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&local).unwrap()).unwrap();
    assert_eq!(after["model"], "opus");
    assert_eq!(after["permissions"]["allow"][0], "Bash(ls)");
    let deny: Vec<&str> = after["permissions"]["deny"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(deny.contains(&"Read(/node_modules/**)") && deny.contains(&"Read(*.pem)"), "{deny:?}");
    assert!(core.context_filter(scope).unwrap().files[0].add.is_empty(), "a second preview adds nothing");
}

#[test]
fn a_git_project_that_does_not_ignore_the_personal_file_gets_a_hint() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    write(&app, "package.json", "{}");
    write(&app, ".git/HEAD", "ref: refs/heads/main");
    let mut core = core_in(temp.path());
    let scope = approved(&mut core, "app", &app);
    let hinted =
        |core: &mut Core| core.context_filter(scope).unwrap().hints.iter().any(|h| h.starts_with("Git schließt"));
    assert!(hinted(&mut core));
    write(&app, ".gitignore", "node_modules/\n.claude/settings.local.json\n");
    assert!(!hinted(&mut core), "an ignore entry ends the hint");
    assert!(!app.join(".claude").exists(), "a preview writes nothing");
}

#[test]
fn every_project_gets_its_filter_in_one_confirmed_step() {
    let temp = tempfile::tempdir().unwrap();
    let web = temp.path().join("web");
    write(&web, "package.json", "{}");
    write(&web, "node_modules/dep/index.js", "1");
    let api = temp.path().join("api");
    write(&api, "pyproject.toml", "[project]\nname='api'\n");
    let broken = temp.path().join("broken");
    write(&broken, "package.json", "{}");
    write(&broken, ".claude/settings.local.json", "{ kein json");
    let mut core = core_in(temp.path());
    for (name, path) in [("web", &web), ("api", &api), ("broken", &broken)] {
        approved(&mut core, name, path);
    }
    let batch = core.context_filter_all().unwrap();
    assert_eq!(batch.plans.len(), 2, "{batch:#?}");
    assert_ne!(batch.plans[0].fingerprint, batch.plans[1].fingerprint, "a preview fits only its own project");
    assert_eq!(batch.skipped.len(), 1);
    assert!(batch.skipped[0].project == "broken" && batch.skipped[0].reason.contains("kein gültiges JSON"));
    assert!(matches!(core.apply_context_filter_all("0000"), Err(mogumogu::Error::Conflict(_))));
    assert!(!web.join(".claude").exists(), "nothing without the preview");
    let done = core.apply_context_filter_all(&batch.fingerprint).unwrap();
    assert_eq!(done.written, 2);
    for project in [&web, &api] {
        assert!(project.join(".claude/settings.local.json").is_file());
    }
    assert_eq!(std::fs::read_to_string(broken.join(".claude/settings.local.json")).unwrap(), "{ kein json");
    let again = core.context_filter_all().unwrap();
    assert!(again.plans.is_empty() && again.complete == 2, "{again:#?}");
}

#[test]
fn a_context_filter_never_writes_through_a_link() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    write(&app, "package.json", "{}");
    let elsewhere = temp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    common::junction(&app.join(".claude"), &elsewhere);
    let mut core =
        Core::open_with(Config::new(Some(temp.path().join("data")), false).unwrap(), Arc::new(FixedClock::new(10)))
            .unwrap();
    let id = core.register_project("app", &path_text(&app)).unwrap();
    let scope = core.approve_project(id).unwrap();
    core.scan(scope).unwrap();
    let plan = core.context_filter(scope).unwrap();
    assert!(matches!(core.apply_context_filter(scope, &plan.fingerprint), Err(mogumogu::Error::Blocked(_))));
    assert!(std::fs::read_dir(&elsewhere).unwrap().next().is_none(), "nothing written behind the junction");
}

#[test]
fn a_context_filter_never_writes_through_a_hard_link_and_leaves_nothing_behind() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    write(&app, "package.json", "{}");
    let outside = temp.path().join("outside.json");
    std::fs::write(&outside, "{}").unwrap();
    std::fs::create_dir_all(app.join(".claude")).unwrap();
    std::fs::hard_link(&outside, app.join(".claude/settings.local.json")).unwrap();
    let mut core = core_in(temp.path());
    let scope = approved(&mut core, "app", &app);
    let plan = core.context_filter(scope).unwrap();
    let refused = core.apply_context_filter(scope, &plan.fingerprint);
    assert!(matches!(&refused, Err(mogumogu::Error::Blocked(text)) if text.contains("Hardlinks")), "{refused:?}");
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "{}", "the file the link points to stays");

    std::fs::remove_file(app.join(".claude/settings.local.json")).unwrap();
    write(&app, ".claude/settings.local.json", "{}");
    let local = app.join(".claude/settings.local.json");
    let mut permissions = std::fs::metadata(&local).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&local, permissions).unwrap();
    let plan = core.context_filter(scope).unwrap();
    let refused = core.apply_context_filter(scope, &plan.fingerprint);
    assert!(
        matches!(&refused, Err(mogumogu::Error::Blocked(text)) if text.contains("Zugriff verweigert")),
        "{refused:?}"
    );
    let names: Vec<String> = std::fs::read_dir(app.join(".claude"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["settings.local.json".to_string()], "no temporary file is left behind");
}
