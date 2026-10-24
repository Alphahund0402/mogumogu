//! Approved discovery/global scopes stay behind the existing Windows boundary.
#![cfg(windows)]
mod common;
use common::{junction, path_text, write};
use mogumogu::clock::FixedClock;
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::Core;
use std::sync::Arc;

fn core(path: &std::path::Path) -> Core {
    Core::open_with(Config::new(Some(path.join("data")), false).unwrap(), Arc::new(FixedClock::new(1_800_000_000)))
        .unwrap()
}

#[test]
fn discovery_is_filename_only_and_registration_does_not_grant_read_access() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    write(&root, "app/package.json", "invalid JSON and untrusted instructions");
    write(&root, "app/nested/Cargo.toml", "also invalid: never read by discovery");
    write(&root, "app/node_modules/ignored/package.json", "{}");
    let outside = temp.path().join("outside");
    write(&outside, "package.json", "{}");
    junction(&root.join("linked"), &outside);
    let mut core = core(temp.path());
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&root))).unwrap();
    let report = core.scan(scope).unwrap();
    assert_eq!(report.state, Some(GenerationState::Complete));
    let current = core.repository().scope(scope).unwrap();
    assert_eq!(current.latest.unwrap().files_read, 0);
    assert!(core.snapshot().unwrap().projects.is_empty());
    let candidates = core.project_candidates().unwrap();
    assert_eq!(candidates.iter().filter(|c| c.kind == DiscoveryKind::Project).count(), 2);
    assert!(candidates.iter().any(|c| c.rel_path == "app/node_modules" && c.kind == DiscoveryKind::Dependencies));
    assert!(candidates.iter().all(|c| !c.registered && c.current && !c.rel_path.contains("linked")));
    let id = core.register_candidate(scope, "app").unwrap();
    let project = core.repository().project(id).unwrap();
    assert!(!project.read_approved && project.packages.is_none());
    assert!(core.register_candidate(scope, "../outside").is_err());
    core.revoke_scope(scope).unwrap();
    assert!(core.project_candidates().unwrap().is_empty());
    assert!(core.register_candidate(scope, "app/nested").is_err());
}

#[test]
fn development_findings_keep_evidence_and_explicit_file_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    write(&root, "app/package.json", "invalid JSON, must never be read here");
    write(&root, "app/local-env/pyvenv.cfg", "untrusted: must never execute");
    write(&root, "app/build/dependency/package.json", "{}");
    write(&root, "app/fixtures/sample.sqlite", "unique test results");
    write(&root, "app/coverage/report.html", "report");
    write(&root, "loose/test.py", "print('do not execute')");
    write(&root, "scratch/experiment.ipynb", "invalid notebook: do not read");
    write(&root, "photos/fixtures/private.jpg", "unrelated data");
    write(&root, ".npm/_cacache/content/foreign/package.json", "{}");
    let mut core = core(temp.path());
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&root))).unwrap();
    let first = core.scan(scope).unwrap();
    assert_eq!(first.state, Some(GenerationState::Complete));
    assert_eq!(core.repository().scope(scope).unwrap().latest.unwrap().files_read, 0);
    let candidates = core.project_candidates().unwrap();
    let by_path = |path: &str| candidates.iter().find(|c| c.rel_path == path).unwrap();
    assert_eq!(by_path("app/local-env").kind, DiscoveryKind::Environment);
    assert_eq!(by_path("app/build").kind, DiscoveryKind::BuildOutput);
    assert_eq!(by_path("app/fixtures").kind, DiscoveryKind::TestData);
    assert_eq!(by_path("app/fixtures/sample.sqlite").kind, DiscoveryKind::Database);
    assert_eq!(by_path("app/coverage").kind, DiscoveryKind::TestOutput);
    assert_eq!(by_path("scratch").kind, DiscoveryKind::Scratchpad);
    assert_eq!(by_path("scratch/experiment.ipynb").kind, DiscoveryKind::Notebook);
    assert_eq!(by_path(".npm").kind, DiscoveryKind::Cache);
    assert_eq!(by_path("app/local-env").project_rel_path.as_deref(), Some("app"));
    assert!(candidates.iter().all(|c| !c.rel_path.starts_with("photos/") && !c.rel_path.contains("foreign")));
    assert!(candidates.iter().all(|c| !c.rel_path.starts_with("app/build/")));
    let script = by_path("loose/test.py");
    assert!(!script.is_dir && script.bytes.unwrap() > 0 && script.modified_at.is_some());
    assert!(script.project_id.is_none() && script.project_rel_path.is_none());
    assert!(core.register_candidate(scope, "loose/test.py").is_err());
    let owner = core.register_candidate(scope, "app").unwrap();
    let resource = core.adopt_candidate(scope, "loose/test.py", owner, Some("Parservergleich")).unwrap();
    assert_eq!(core.adopt_candidate(scope, "loose/test.py", owner, None).unwrap(), resource);
    let stored = core.repository().resource(resource).unwrap();
    assert_eq!(stored.kind, ResourceKind::Source);
    assert_eq!(stored.acquisition, Acquisition::Adopted);
    assert_eq!(stored.purpose.as_deref(), Some("Parservergleich"));
    assert!(stored.protected && stored.bytes.unwrap() > 0);
    assert!(!core.repository().project(owner).unwrap().read_approved);
    assert_eq!(core.repository().scopes().unwrap().len(), 1, "adoption grants no new scope");
    assert_eq!(
        core.repository()
            .owners()
            .unwrap()
            .iter()
            .filter(|o| o.resource_id == resource && o.kind == OwnerKind::Explicit)
            .count(),
        1
    );
    let other = core.register_project("anderes Projekt", &path_text(&temp.path().join("other"))).unwrap();
    assert!(core.adopt_candidate(scope, "loose/test.py", other, None).is_err());
    assert!(core.adopt_candidate(scope, "../outside", owner, None).is_err());
    let current = core.project_candidates().unwrap();
    let adopted = current.iter().find(|c| c.rel_path == "loose/test.py").unwrap();
    assert_eq!(adopted.resource_id, Some(resource));
    assert_eq!(adopted.project_id, Some(owner));
    assert!(adopted.evidence.contains("ausdrücklich"));
    let repeat = core.scan(scope).unwrap();
    assert_eq!((repeat.added, repeat.removed, repeat.changed), (0, 0, 0));
    write(&root, "loose/test.py", "print('a changed experiment with a different size')");
    let changed = core.scan(scope).unwrap();
    assert_eq!((changed.added, changed.removed, changed.changed), (0, 0, 1));
    let hits = core.search("test.py").unwrap();
    assert!(hits.iter().any(|h| h.area == SearchArea::Discovery && h.key == format!("{scope}:loose/test.py")));
    core.revoke_scope(scope).unwrap();
    assert!(core.search("test.py").unwrap().iter().all(|h| h.area != SearchArea::Discovery));
}

#[test]
fn redacted_discovery_paths_are_visible_but_never_reconstructed_for_adoption() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    write(&root, "app/package.json", "{}");
    write(&root, "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/probe.py", "pass");
    let mut core = core(temp.path());
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&root))).unwrap();
    core.scan(scope).unwrap();
    let project = core.register_candidate(scope, "app").unwrap();
    let candidates = core.project_candidates().unwrap();
    let redacted = candidates.iter().find(|c| c.rel_path.ends_with("/probe.py")).unwrap();
    assert!(redacted.rel_path.contains(mogumogu::privacy::REDACTED));
    assert!(redacted.evidence.contains("manuell"));
    assert!(redacted.project_id.is_none());
    assert!(core.adopt_candidate(scope, &redacted.rel_path, project, None).is_err());
    let dump = serde_json::to_string(&core.repository().published_items(scope).unwrap()).unwrap();
    assert!(!dump.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"));
}

#[test]
fn exact_discovery_lookup_reaches_beyond_the_dashboard_page_without_reading_content() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    for index in 0..210 {
        write(&root, &format!("scripts/probe-{index:03}.py"), "untrusted, do not execute or read");
    }
    write(&root, "zzzz-tail.py", "one more forgotten experiment");
    let mut core = core(temp.path());
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&root))).unwrap();
    core.scan(scope).unwrap();
    let identity = core.repository().scope_identity(scope).unwrap();
    let page = core.project_candidates().unwrap();
    assert_eq!(page.len(), mogumogu::limits::UI_PAGE as usize);
    assert!(page.iter().all(|candidate| candidate.rel_path != "zzzz-tail.py"));
    let hit = core.search("zzzz-tail").unwrap().into_iter().find(|hit| hit.area == SearchArea::Discovery).unwrap();
    assert_eq!(hit.key, format!("{scope}:zzzz-tail.py"));
    let candidate = core.discovery_candidate(scope, "zzzz-tail.py").unwrap();
    assert_eq!(candidate.kind, DiscoveryKind::Script);
    assert_eq!(candidate.name, "zzzz-tail.py");
    assert!(candidate.current && !candidate.registered && candidate.project_id.is_none());
    assert!(core.discovery_candidate(scope, "../outside").is_err());
    assert_eq!(core.repository().scope(scope).unwrap().latest.unwrap().files_read, 0);
    assert_eq!(core.repository().scope_identity(scope).unwrap(), identity);
    assert_eq!(core.repository().scopes().unwrap().len(), 1);
    assert!(core.repository().projects().unwrap().is_empty());
    assert!(core.repository().resources().unwrap().is_empty());
    core.revoke_scope(scope).unwrap();
    assert!(core.discovery_candidate(scope, "zzzz-tail.py").is_err());
    assert!(core.search("zzzz-tail").unwrap().is_empty());
}

#[test]
fn incomplete_discovery_retains_old_findings_but_blocks_adoption() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    write(&root, "app/package.json", "{}");
    write(&root, "scratch/probe.py", "pass");
    let mut core = core(temp.path());
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&root))).unwrap();
    core.scan(scope).unwrap();
    let project = core.register_candidate(scope, "app").unwrap();
    let generation = core.repository().begin_generation(scope, "bounded-discovery-test").unwrap();
    core.repository()
        .finish_generation(
            generation,
            &mogumogu::storage::GenerationResult {
                limit_reason: Some("Eintragslimit erreicht".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let old = core.project_candidates().unwrap();
    assert!(!old.is_empty() && old.iter().all(|c| !c.current));
    assert!(!core.discovery_candidate(scope, "scratch/probe.py").unwrap().current);
    assert!(core.adopt_candidate(scope, "scratch/probe.py", project, None).is_err());
    assert!(core.register_candidate(scope, "scratch").is_err());
    assert!(core.repository().resources().unwrap().is_empty());
}

#[test]
fn regular_project_scan_also_finds_data_notebooks_and_skipped_caches() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    write(&root, "Cargo.toml", "[package]\nname='demo'\nversion='0.1.0'\n");
    write(&root, "experiments/notes.ipynb", "unread notebook");
    write(&root, "fixtures/test.db", "database");
    write(&root, ".pytest_cache/v/cache/nodeids", "cache");
    let mut core = core(temp.path());
    let project = core.register_project("demo", &path_text(&root)).unwrap();
    let scope = core.approve_project(project).unwrap();
    assert_eq!(core.scan(scope).unwrap().state, Some(GenerationState::Complete));
    let candidates = core.project_candidates().unwrap();
    assert!(candidates.iter().any(|c| c.rel_path == "experiments/notes.ipynb" && c.kind == DiscoveryKind::Notebook));
    assert!(candidates.iter().any(|c| c.rel_path == "fixtures/test.db" && c.kind == DiscoveryKind::Database));
    assert!(candidates.iter().any(|c| c.rel_path == ".pytest_cache" && c.kind == DiscoveryKind::Cache));
    assert!(candidates.iter().all(|c| c.project_id == Some(project)));
    assert!(!core.repository().project(project).unwrap().ecosystem.contains("discovery"));
}

#[test]
fn global_stores_keep_provenance_and_do_not_follow_record_paths_or_junctions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("python");
    write(&root, "pyvenv.cfg", "version = 3.13.2\n");
    write(
        &root,
        "Lib/site-packages/child-1.0.dist-info/METADATA",
        "Metadata-Version: 2.4\nName: child\nVersion: 1.0\nRequires-Dist: parent>=1; sys_platform=='win32'\n\nsecret body never stored\n",
    );
    write(&root, "Lib/site-packages/child-1.0.dist-info/RECORD", "../../../outside/private.txt,,\nchild.py,,\n");
    write(&root, "Lib/site-packages/evil.pth", "import os; os.system('must never run')");
    let outside = temp.path().join("outside");
    write(&outside, "foreign-1.0.dist-info/METADATA", "Name: foreign");
    junction(&root.join("Lib").join("site-packages").join("linked"), &outside);
    let mut core = core(temp.path());
    let scope = core.approve_system(ScopeKind::Python, Some(&path_text(&root))).unwrap();
    assert_eq!(core.scan(scope).unwrap().state, Some(GenerationState::Complete));
    let items = core.repository().published_items(scope).unwrap();
    let package = items.iter().find(|i| i.name == "child").unwrap();
    assert_eq!(package.source, SourceClass::Unknown);
    assert_eq!(package.evidence.owned_files, Some(2));
    assert_eq!(package.evidence.dependencies[0].name, "parent");
    let dump = serde_json::to_string(&items).unwrap();
    for forbidden in ["private.txt", "secret body", "foreign", "must never run"] {
        assert!(!dump.contains(forbidden));
    }
    let project = temp.path().join("approved-root");
    std::fs::rename(&root, &project).unwrap();
    std::fs::create_dir(&root).unwrap();
    write(&root, "pyvenv.cfg", "version = 3.14.0\n");
    assert!(core.scan(scope).unwrap().problem.is_some());
    assert_eq!(core.repository().scope(scope).unwrap().status, ScopeStatus::IdentityChanged);
}

#[test]
fn global_npm_versions_and_cargo_install_records_are_static_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let npm = temp.path().join("npm");
    write(
        &npm,
        "@scope/tool/package.json",
        r#"{"name":"@scope/tool","version":"1.0.0","_resolved":"https://u:private@private.example/tool.tgz","scripts":{"install":"never"}}"#,
    );
    let cargo = temp.path().join("cargo");
    write(
        &cargo,
        ".crates.toml",
        "[v1]\n'example 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)' = ['example.exe']\n",
    );
    let mut core = core(temp.path());
    let scope = core.approve_system(ScopeKind::NpmGlobal, Some(&path_text(&npm))).unwrap();
    assert_eq!(core.scan(scope).unwrap().state, Some(GenerationState::Complete));
    let items = core.repository().published_items(scope).unwrap();
    assert_eq!(items[0].name, "@scope/tool");
    assert_eq!(items[0].source, SourceClass::PrivateRegistry);
    assert!(!serde_json::to_string(&items).unwrap().contains("u:private"));
    let scope = core.approve_system(ScopeKind::CargoTools, Some(&path_text(&cargo))).unwrap();
    assert_eq!(core.scan(scope).unwrap().state, Some(GenerationState::Complete));
    let items = core.repository().published_items(scope).unwrap();
    assert_eq!(items[0].name, "example");
    assert_eq!(items[0].source_host.as_deref(), Some("crates.io"));
    assert!(items[0].detail.contains("nicht geprüft"));
}

#[test]
fn projects_and_scratchpads_lead_the_findings_page() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    for index in 0..30 {
        write(&root, &format!("aaa-{index:02}/run.py"), "never read");
        write(&root, &format!("aaa-{index:02}/node_modules/dep/index.js"), "dependency");
    }
    write(&root, "zzz-app/Cargo.toml", "never read");
    write(&root, "zzz-tools/a.ps1", "never read");
    write(&root, "zzz-tools/b.ps1", "never read");
    write(&root, "zzz-poc/main.py", "never read");
    let mut core = core(temp.path());
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&root))).unwrap();
    assert_eq!(core.scan(scope).unwrap().state, Some(GenerationState::Complete));
    let candidates = core.project_candidates().unwrap();
    let kinds: Vec<(&str, DiscoveryKind)> = candidates.iter().take(3).map(|c| (c.rel_path.as_str(), c.kind)).collect();
    assert_eq!(
        kinds,
        [
            ("zzz-app", DiscoveryKind::Project),
            ("zzz-poc", DiscoveryKind::Scratchpad),
            ("zzz-tools", DiscoveryKind::Scratchpad)
        ]
    );
    let tools = candidates.iter().find(|c| c.rel_path == "zzz-tools").unwrap();
    assert!(tools.evidence.contains("2 lose Skripte"), "{}", tools.evidence);
    assert!(candidates.iter().any(|c| c.rel_path == "zzz-tools/a.ps1" && c.kind == DiscoveryKind::Script));
    assert!(candidates.iter().any(|c| c.kind == DiscoveryKind::Dependencies));
}

#[test]
fn nested_projects_point_to_their_root_and_strong_projects_come_first() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    write(&root, "aaa-weak/Makefile", "never read");
    write(&root, "zzz-strong/Cargo.toml", "never read");
    write(&root, "zzz-strong/Cargo.lock", "never read");
    write(&root, "zzz-strong/crates/core/Cargo.toml", "never read");
    write(&root, "zzz-unrelated/Cargo.toml", "never read");
    std::fs::create_dir_all(root.join("zzz-strong/.git")).unwrap();
    let mut core = core(temp.path());
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&root))).unwrap();
    assert_eq!(core.scan(scope).unwrap().state, Some(GenerationState::Complete));
    let candidates = core.project_candidates().unwrap();
    let projects: Vec<(&str, u8)> = candidates
        .iter()
        .filter(|c| c.kind == DiscoveryKind::Project)
        .map(|c| (c.rel_path.as_str(), c.confidence))
        .collect();
    assert_eq!(projects.first(), Some(&("zzz-strong", 3)), "{projects:?}");
    assert_eq!(projects.last(), Some(&("aaa-weak", 1)), "{projects:?}");
    let position = |rel: &str| projects.iter().position(|(p, _)| *p == rel).unwrap();
    assert!(
        position("zzz-unrelated") < position("zzz-strong/crates/core"),
        "members follow outer projects: {projects:?}"
    );
    let nested = candidates.iter().find(|c| c.rel_path == "zzz-strong/crates/core").unwrap();
    assert_eq!(nested.project_rel_path.as_deref(), Some("zzz-strong"));
    assert!(nested.evidence.contains("Teilprojekt von zzz-strong"), "{}", nested.evidence);
    assert!(nested.modified_at.is_some(), "folder activity by day");
}

#[test]
fn go_bun_yarn_gradle_and_maven_projects_are_inventoried_without_walking_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("poly");
    write(&root, "svc/go.mod", "module example.com/svc\n\ngo 1.22\n\nrequire github.com/foo/bar v1.2.3\n");
    write(&root, "svc/go.sum", "github.com/foo/bar v1.2.3 h1:abc=\ngithub.com/foo/bar v1.2.3/go.mod h1:def=\n");
    write(&root, "svc/vendor/modules.txt", "# github.com/foo/bar v1.2.3\n## explicit\n");
    write(&root, "svc/vendor/github.com/foo/bar/go.mod", "module github.com/foo/bar\nrequire never/read v9.9.9\n");
    write(&root, "web/package.json", r#"{"dependencies":{"react":"^18.2.0"}}"#);
    write(
        &root,
        "web/bun.lock",
        r#"{"lockfileVersion":1,"workspaces":{"":{}},"packages":{"react":["react@18.2.0","",{},"sha512-x",],},}"#,
    );
    write(&root, "web/node_modules/react/package.json", "{}");
    write(&root, "site/package.json", r#"{"devDependencies":{"js-tokens":"^4.0.0"}}"#);
    write(&root, "site/yarn.lock", "# yarn lockfile v1\n\njs-tokens@^4.0.0:\n  version \"4.0.0\"\n");
    write(
        &root,
        "android/build.gradle.kts",
        "dependencies {\n    implementation(\"com.squareup.okhttp3:okhttp:4.12.0\")\n}\n",
    );
    write(&root, "android/build/tmp/build.gradle", "dependencies { implementation 'never:read:1.0' }\n");
    write(
        &root,
        "jvm/pom.xml",
        "<project><dependencies><dependency><groupId>com.google.guava</groupId><artifactId>guava</artifactId><version>33.3.1-jre</version></dependency></dependencies></project>",
    );
    write(
        &root,
        "jvm/target/classes/META-INF/maven/x/pom.xml",
        "<project><dependencies><dependency><groupId>never</groupId><artifactId>read</artifactId><version>1</version></dependency></dependencies></project>",
    );
    let mut core = core(temp.path());
    let project = core.register_project("poly", &path_text(&root)).unwrap();
    let scope = core.approve_project(project).unwrap();
    assert_eq!(core.scan(scope).unwrap().state, Some(GenerationState::Complete));
    let items = core.repository().published_items(scope).unwrap();
    let find = |ecosystem: &str, name: &str, state: InstallState| {
        items.iter().any(|i| i.ecosystem == ecosystem && i.name == name && i.install_state == Some(state))
    };
    assert!(find("go", "github.com/foo/bar", InstallState::Declared));
    assert!(find("go", "github.com/foo/bar", InstallState::Resolved));
    assert!(find("go", "github.com/foo/bar", InstallState::Installed));
    assert!(find("bun", "react", InstallState::Declared), "package.json follows the lockfile beside it");
    assert!(find("bun", "react", InstallState::Resolved));
    assert!(find("bun", "react", InstallState::Installed));
    assert!(find("yarn", "js-tokens", InstallState::Declared));
    assert!(find("yarn", "js-tokens", InstallState::Resolved));
    assert!(find("gradle", "com.squareup.okhttp3:okhttp", InstallState::Declared));
    assert!(find("maven", "com.google.guava:guava", InstallState::Declared));
    assert!(!items.iter().any(|i| i.name.starts_with("never")), "vendor, build and target are not walked");
    assert!(items.iter().any(|i| i.ecosystem == "gradle" && i.category == ItemCategory::Output));
    assert!(items.iter().any(|i| i.ecosystem == "maven" && i.category == ItemCategory::Output));
}

#[test]
fn found_projects_are_adopted_together_only_with_the_previewed_fingerprint() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("search");
    write(&root, "alpha/Cargo.toml", "never read");
    std::fs::create_dir_all(root.join("alpha/.git")).unwrap();
    write(&root, "alpha/crates/core/Cargo.toml", "never read");
    write(&root, "beta/package.json", "never read");
    write(&root, "beta/package-lock.json", "never read");
    write(&root, "gone/pyproject.toml", "never read");
    write(&root, "gone/uv.lock", "never read");
    write(&root, "weak/Makefile", "never read");
    write(&root, "known/go.mod", "never read");
    std::fs::create_dir_all(root.join("known/.git")).unwrap();
    write(&root, "known/tools/gen/go.mod", "never read");
    let mut core = core(temp.path());
    core.register_project("known", &path_text(&root.join("known"))).unwrap();
    let scope = core.approve_system(ScopeKind::ProjectSearch, Some(&path_text(&root))).unwrap();
    assert_eq!(core.scan(scope).unwrap().state, Some(GenerationState::Complete));

    let preview = core.found_projects(None).unwrap();
    let names: Vec<&str> = preview.projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["alpha", "beta", "gone"], "certain, outer and untracked only");
    assert!(core.repository().projects().unwrap().iter().all(|p| p.name == "known"), "a preview registers nothing");
    assert!(matches!(core.adopt_found_projects(None, "0000"), Err(mogumogu::Error::Conflict(_))));
    assert_eq!(core.repository().projects().unwrap().len(), 1);

    std::fs::remove_dir_all(root.join("gone")).unwrap();
    let adopted = core.adopt_found_projects(Some(scope), &preview.fingerprint).unwrap();
    assert_eq!(adopted.approved.len(), 2);
    assert_eq!(adopted.skipped.len(), 1);
    assert!(adopted.skipped[0].path.ends_with("gone") && !adopted.skipped[0].reason.is_empty());
    let projects = core.repository().projects().unwrap();
    for name in ["alpha", "beta"] {
        assert!(projects.iter().any(|p| p.name == name && p.read_approved), "{name}");
    }
    let remaining: Vec<String> = core.found_projects(None).unwrap().projects.into_iter().map(|p| p.name).collect();
    assert_eq!(
        remaining,
        ["gone"],
        "registered projects are not offered again; the missing folder stays until rescanned"
    );
}
