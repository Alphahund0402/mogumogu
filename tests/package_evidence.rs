//! Evidence and lifecycle regressions: no live registries or user resources.
use mogumogu::clock::{DAY, FixedClock};
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::service::Core;
use mogumogu::storage::{GenerationResult, Repository, SCHEMA_VERSION};
use mogumogu::updates::{self, FetchError, NoNetwork, Registry, RegistryMetadata};
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct FakeRegistry {
    calls: Arc<Mutex<Vec<(String, String, String)>>>,
    offline: bool,
}
impl Registry for FakeRegistry {
    fn metadata(&self, host: &str, name: &str, version: &str) -> Result<RegistryMetadata, FetchError> {
        self.calls.lock().unwrap().push((host.into(), name.into(), version.into()));
        if self.offline {
            return Err(FetchError::Offline("fixture".into()));
        }
        Ok(RegistryMetadata {
            latest: Some("2.0.0".into()),
            lifecycle: Lifecycle {
                deprecated: Some(version == "1.0.0"),
                reason: Some("replace legacy version".into()),
                ..Default::default()
            },
        })
    }
}
fn package(name: &str, version: &str, source: SourceClass, host: &str) -> InventoryItem {
    let mut item = InventoryItem::new(ItemCategory::Package, "npm", name, "package-lock.json")
        .version(version)
        .state(InstallState::Resolved)
        .source(source, Some(host.into()));
    item.evidence.qualifier = format!("{version}|{source}|{host}");
    item
}
fn publish(repo: &mut Repository, scope: i64, items: Vec<InventoryItem>) {
    let generation = repo.begin_generation(scope, "evidence-test").unwrap();
    repo.finish_generation(generation, &GenerationResult { items, ..Default::default() }).unwrap();
}
fn project(repo: &mut Repository, name: &str) -> i64 {
    let path = format!(r"C:\fixture\{name}");
    let id = repo.register_project(name, &path).unwrap();
    repo.approve_scope(ScopeKind::Project, Some(id), &path, None).unwrap()
}

#[test]
fn lockfile_occurrences_do_not_lose_parallel_versions_or_sources() {
    use mogumogu::adapters::{FoundFile, by_id};
    for (adapter, name, text) in [
        (
            "npm",
            "package-lock.json",
            r#"{"lockfileVersion":3,"packages":{"node_modules/x":{"version":"1.0.0"},"node_modules/a/node_modules/x":{"version":"2.0.0"}}}"#,
        ),
        (
            "npm",
            "package-lock.json",
            r#"{"lockfileVersion":1,"dependencies":{"x":{"version":"1.0.0"},"a":{"version":"1.0.0","dependencies":{"x":{"version":"2.0.0"}}}}}"#,
        ),
        (
            "cargo",
            "Cargo.lock",
            "version=4\n[[package]]\nname='x'\nversion='1.0.0'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n[[package]]\nname='x'\nversion='2.0.0'\nsource='registry+https://github.com/rust-lang/crates.io-index'\n",
        ),
    ] {
        let items =
            by_id(adapter).unwrap().parse_file(&FoundFile { name, rel_dir: "", content: text, siblings: &[] }).unwrap();
        let mut repo = Repository::memory(false).unwrap();
        let scope = project(&mut repo, "app");
        publish(&mut repo, scope, items);
        let packages = repo.published_items(scope).unwrap().into_iter().filter(|i| i.name == "x").collect::<Vec<_>>();
        assert_eq!(packages.len(), 2, "{adapter} {text}");
        assert_ne!(packages[0].version, packages[1].version);
    }
}

#[test]
fn cache_entries_and_command_references_remain_distinct_from_project_dependencies() {
    let clock = Arc::new(FixedClock::new(10 * DAY));
    let temp = tempfile::tempdir().unwrap();
    let mut core = Core::open_with(
        Config::new(Some(temp.path().join("data")), false).unwrap(),
        clock.clone(),
        Box::new(NoNetwork),
    )
    .unwrap();
    let scope = project(core.repository(), "scripts");
    let mut manifest = InventoryItem::new(ItemCategory::Manifest, "npm", "package.json", "package.json");
    manifest.evidence.commands = vec!["global-tool".into()];
    publish(core.repository(), scope, vec![manifest]);
    let global = core.repository().approve_scope(ScopeKind::NpmGlobal, None, r"C:\fixture\global", None).unwrap();
    let mut item = package("global-package", "1.0.0", SourceClass::PublicRegistry, "registry.npmjs.org");
    item.install_state = Some(InstallState::Installed);
    item.evidence.executables = vec!["global-tool".into()];
    publish(core.repository(), global, vec![item]);
    let report = core.package_report(0, 60).unwrap();
    assert!(report.packages[0].references.is_empty());
    assert!(report.packages[0].possible_references[0].contains("Befehlsname global-tool"));
    let cache = core.repository().approve_scope(ScopeKind::NugetCache, None, r"C:\fixture\nuget", None).unwrap();
    let item = InventoryItem::new(ItemCategory::Cache, "nuget", "example", "example/1.0.0")
        .version("1.0.0")
        .source(SourceClass::PublicRegistry, Some("api.nuget.org".into()));
    publish(core.repository(), cache, vec![item.clone()]);
    let settings =
        Settings { network_updates_enabled: true, update_sources: vec!["api.nuget.org".into()], ..Default::default() };
    let registry = FakeRegistry::default();
    let summary = updates::run(core.repository(), &registry, &settings).unwrap();
    assert_eq!(summary.checked, 1);
    assert_eq!(registry.calls.lock().unwrap()[0].0, "api.nuget.org");
    let report = core.package_report(0, 60).unwrap();
    let finding = report.packages.iter().find(|p| p.id == item.package_id()).unwrap();
    assert_eq!(finding.lifecycle.deprecated, Some(true));
    core.retain_package(&finding.key, true).unwrap();
    let report = core.package_report(0, 60).unwrap();
    let finding = report.packages.iter().find(|p| p.id == item.package_id()).unwrap();
    assert!(finding.retained && finding.review, "keeping does not suppress upstream warnings");
    assert!(finding.occurrences[0].state.is_none(), "cache presence is not installation");
    assert!(core.repository().resources().unwrap().is_empty(), "review never creates cleanup targets");
    assert!(!finding.lifecycle_stale);
    clock.advance(8 * DAY);
    let report = core.package_report(0, 60).unwrap();
    assert!(
        report.packages.iter().find(|p| p.id == item.package_id()).unwrap().lifecycle_stale,
        "a cached page must re-evaluate evidence when the clock changes"
    );
}

#[test]
fn every_version_and_source_gets_separate_evidence_and_backoff() {
    let clock = Arc::new(FixedClock::new(10 * DAY));
    let mut repo = Repository::memory_with_clock(false, clock.clone()).unwrap();
    let scope = project(&mut repo, "app");
    let old = package("same", "1.0.0", SourceClass::PublicRegistry, "registry.npmjs.org");
    let current = package("same", "2.0.0", SourceClass::PublicRegistry, "registry.npmjs.org");
    let private = package("same", "1.0.0", SourceClass::PrivateRegistry, "private.example");
    publish(&mut repo, scope, vec![private.clone(), old.clone(), current.clone()]);
    assert_eq!(repo.published_items(scope).unwrap().len(), 3);
    let registry = FakeRegistry::default();
    let mut settings = Settings {
        network_updates_enabled: true,
        update_sources: vec!["registry.npmjs.org".into()],
        ..Default::default()
    };
    let summary = updates::run(&mut repo, &registry, &settings).unwrap();
    assert_eq!((summary.checked, summary.skipped_private, summary.lifecycle_warnings), (2, 1, 1));
    assert_eq!(registry.calls.lock().unwrap().len(), 2);
    assert_eq!(repo.update_record(&old.package_id()).unwrap().unwrap().status, UpdateStatus::UpdateAvailable);
    assert_eq!(repo.update_record(&current.package_id()).unwrap().unwrap().status, UpdateStatus::Current);
    assert!(repo.update_record(&private.package_id()).unwrap().is_none());
    assert_eq!(updates::run(&mut repo, &registry, &settings).unwrap().cached, 2);
    clock.advance(2 * DAY);
    let offline = FakeRegistry { offline: true, ..Default::default() };
    assert_eq!(updates::run(&mut repo, &offline, &settings).unwrap().failed, 2);
    let retained = repo.update_record(&old.package_id()).unwrap().unwrap();
    assert_eq!(retained.status, UpdateStatus::Offline);
    assert_eq!(retained.lifecycle.deprecated, Some(true));
    assert!(retained.lifecycle.checked_at.unwrap() < retained.checked_at);
    assert_eq!(updates::run(&mut repo, &offline, &settings).unwrap().cached, 2);
    settings.update_sources.clear();
    updates::run(&mut repo, &registry, &settings).unwrap();
    assert_eq!(repo.update_record(&old.package_id()).unwrap().unwrap().lifecycle.deprecated, Some(true));
}

#[test]
fn partial_coverage_and_keep_never_become_proof_of_unused() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::new(Some(temp.path().join("data")), false).unwrap();
    let mut core = Core::open_with(config, Arc::new(FixedClock::new(10 * DAY)), Box::new(NoNetwork)).unwrap();
    let a = project(core.repository(), "a");
    project(core.repository(), "unscanned");
    let global = core.repository().approve_scope(ScopeKind::NpmGlobal, None, r"C:\fixture\global", None).unwrap();
    let mut parent = package("parent", "1.0.0", SourceClass::PublicRegistry, "registry.npmjs.org");
    parent.evidence.dependencies.push(PackageDependency {
        name: "child".into(),
        requirement: "1.0.0".into(),
        kind: "lock".into(),
        exact: true,
    });
    let child = package("child", "1.0.0", SourceClass::PublicRegistry, "registry.npmjs.org");
    publish(core.repository(), a, vec![parent, child.clone()]);
    let mut global_item = package("cli-tool", "1.0.0", SourceClass::Unknown, "");
    global_item.install_state = Some(InstallState::Installed);
    publish(core.repository(), global, vec![global_item.clone()]);
    let report = core.package_report(0, 60).unwrap();
    assert_eq!((report.coverage.complete_projects, report.coverage.incomplete_projects), (1, 1));
    let item = report.packages.iter().find(|p| p.id.name == "child").unwrap();
    assert!(!item.references.is_empty());
    assert!(item.possible_references.iter().any(|r| r.contains("parent@1.0.0")));
    let item = report.packages.iter().find(|p| p.id.name == "cli-tool").unwrap();
    assert!(item.references.is_empty() && item.review);
    assert!(item.next_action.contains("CLI-Nutzung"));
    core.retain_package(&item.key, true).unwrap();
    let retained = core.package_report(0, 60).unwrap().packages.into_iter().find(|p| p.id.name == "cli-tool").unwrap();
    assert!(retained.retained && !retained.review);
    assert!(retained.occurrences[0].project_id.is_none());
    let generation = core.repository().begin_generation(a, "evidence-test").unwrap();
    core.repository()
        .finish_generation(generation, &GenerationResult { errors: vec!["unreadable".into()], ..Default::default() })
        .unwrap();
    let report = core.package_report(0, 60).unwrap();
    assert_eq!(report.coverage.complete_projects, 0);
    assert!(!report.packages.iter().find(|p| p.id.name == "child").unwrap().occurrences[0].current);
    const {
        assert!(!mogumogu::cleanup::PRODUCTION_ENABLED);
    }
}

#[test]
fn schema_v2_upgrade_keeps_inventory_and_backups_and_invalidates_latest_only_cache() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inventory.sqlite3");
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch(include_str!("../migrations/001_initial.sql")).unwrap();
        db.execute_batch(include_str!("../migrations/002_inventory.sql")).unwrap();
        db.execute(
            "INSERT INTO projects(name,path,origin) VALUES('existing','C:\\fixture\\existing','registered')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO update_checks(ecosystem,name,status,checked_at) VALUES('npm','old','current',0)", [])
            .unwrap();
        db.pragma_update(None, "user_version", 2).unwrap();
    }
    let mut repo = Repository::open_path(&path, false, Arc::new(FixedClock::new(100))).unwrap();
    assert_eq!(repo.projects().unwrap()[0].name, "existing");
    assert!(repo.update_records().unwrap().is_empty());
    let backup = temp.path().join("backups/before-schema-3-100.sqlite3");
    let old = Connection::open(&backup).unwrap();
    assert_eq!(old.query_row("SELECT COUNT(*) FROM update_checks", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    let new = Connection::open(&path).unwrap();
    assert_eq!(new.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0)).unwrap(), SCHEMA_VERSION);
    assert_eq!(new.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
}
