//! PDM: pdm.lock (resolved) and the pyproject.toml beside it (declared:
//! PEP 621, `[dependency-groups]`, the older `[tool.pdm.dev-dependencies]`
//! and `[[tool.pdm.source]]`). No PDM command or plugin runs; pdm.toml and
//! the user's configuration are not read. Unless the `static_urls` strategy
//! wrote download URLs, the lockfile names no index and sources stay
//! unknown.
use super::pip::{self, Indexes, Source};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::toml_table;
use std::collections::HashMap;

pub struct Pdm;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "pdm",
    name: "PDM",
    markers: &["pdm.lock", "pyproject.toml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "pdm.lock (lock_version 4.x, ältere mit sections)",
        "pyproject.toml mit pdm.lock (PEP 621, dependency-groups, [tool.pdm.dev-dependencies], Quellen)",
    ],
    limits: "Kein PDM-Aufruf; pdm.toml und Benutzerkonfiguration werden nicht gelesen – ohne static_urls nennt das Lockfile keine Quelle, sie bleibt unbekannt; Marker, Lock-Ziele und Quellfilter werden nicht ausgewertet.",
};

impl Adapter for Pdm {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("pdm.lock") {
            return bounded(lock_items(file)?);
        }
        match pip::owned_pyproject(file, "pdm")? {
            Some(table) => bounded(pyproject_items(file, &table)),
            None => Ok(Vec::new()),
        }
    }
}

/// `[[tool.pdm.source]]`. Include/exclude filters are not evaluated, so any
/// source may serve any package.
pub(super) fn indexes(pdm: Option<&toml::Table>) -> Indexes {
    let mut indexes = Indexes::default();
    let sources = pdm.and_then(|p| p.get("source")).and_then(toml::Value::as_array);
    for source in sources.into_iter().flatten().take(100) {
        let Some(url) = source.get("url").and_then(toml::Value::as_str) else { continue };
        let class = pip::index_source(url);
        indexes.unpinned.push(class.clone());
        if let Some(name) = source.get("name").and_then(toml::Value::as_str) {
            indexes.named.push((name.to_string(), class));
        }
    }
    indexes
}

fn pyproject_items(file: &FoundFile<'_>, table: &toml::Table) -> Vec<InventoryItem> {
    let pdm = pip::tool(table, "pdm");
    let own = pip::project_name(table);
    let mut declared = pip::standard_declarations(table);
    let groups = pdm.and_then(|p| p.get("dev-dependencies")).and_then(toml::Value::as_table);
    for (group, list) in groups.into_iter().flatten().take(1_000) {
        pip::strings(Some(list), group, own, &mut declared);
    }
    pip::pyproject_result(file, "pdm", table, None, declared, &indexes(pdm).default_source())
}

/// A VCS, path or URL entry, or the download URLs `static_urls` keeps.
fn source(package: &toml::Value) -> Source {
    if ["git", "hg", "svn", "bzr"].iter().any(|vcs| package.get(*vcs).is_some()) {
        return (SourceClass::Git, None);
    }
    if package.get("path").is_some() {
        return (SourceClass::Path, None);
    }
    if let Some(url) = package.get("url").and_then(toml::Value::as_str) {
        return pip::direct_source(url);
    }
    let first = package.get("files").and_then(toml::Value::as_array).and_then(|files| files.first());
    first.and_then(|f| f.get("url")).and_then(toml::Value::as_str).map_or(pip::UNKNOWN, pip::index_source)
}

/// Requirement strings as edges, without the entry's own name (an extra's base).
fn edges(value: Option<&toml::Value>, own: &str) -> Vec<PackageDependency> {
    let texts = value.and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str);
    texts
        .take(10_000)
        .filter_map(pip::requirement)
        .filter(|(name, _)| name != own)
        .map(|(name, spec)| {
            let pin = pip::pinned(&spec).map(str::to_string);
            PackageDependency {
                name,
                exact: pin.is_some(),
                requirement: pin.unwrap_or_else(|| pip::kept_spec(spec)),
                kind: "lock (Marker ungeprüft)".into(),
            }
        })
        .collect()
}

fn names(value: Option<&toml::Value>) -> Vec<&str> {
    value.and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str).take(1_000).collect()
}

/// pdm.lock: `groups` (`sections` in old locks) is the context. An entry
/// with `extras` repeats its base package; only its edges are kept.
fn lock_items(file: &FoundFile<'_>) -> AdapterResult {
    let table = toml_table(file.content).map_err(|e| e.to_string())?;
    let metadata = table.get("metadata");
    let version = metadata.and_then(|m| m.get("lock_version")).and_then(toml::Value::as_str).unwrap_or("?");
    let mut detail = format!("Lockfile v{version}");
    for (key, label) in [("groups", "Gruppen"), ("strategy", "Strategie")] {
        let values = names(metadata.and_then(|m| m.get(key)));
        if !values.is_empty() {
            detail.push_str(&format!(" · {label} {}", values.join(", ")));
        }
    }
    let mut items = vec![manifest("pdm", file, detail)];
    let mut bases: HashMap<(String, String), usize> = HashMap::new();
    for package in table.get("package").and_then(toml::Value::as_array).into_iter().flatten() {
        let Some(name) = package.get("name").and_then(toml::Value::as_str).map(pip::normalize) else { continue };
        let version = package.get("version").and_then(toml::Value::as_str).unwrap_or_default().to_string();
        let edges = edges(package.get("dependencies"), &name);
        if package.get("extras").and_then(toml::Value::as_array).is_some_and(|extras| !extras.is_empty()) {
            if let Some(&index) = bases.get(&(name, version)) {
                items[index].evidence.dependencies.extend(edges);
            }
            continue;
        }
        if package.get("path").and_then(toml::Value::as_str).is_some_and(|p| p.trim_end_matches(['/', '\\']) == ".") {
            continue; // the project itself
        }
        let (class, host) = source(package);
        let mut detail = String::from("aufgelöst laut pdm.lock");
        if package.get("editable").and_then(toml::Value::as_bool) == Some(true) {
            detail.push_str(" · editierbar");
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "pdm", name.clone(), file.rel_path())
            .version(version.clone())
            .state(InstallState::Resolved)
            .source(class, host)
            .detail(detail);
        item.evidence.context = names(package.get("groups").or_else(|| package.get("sections"))).join(",");
        item.evidence.qualifier = format!("{version}|{class}|{}", item.source_host.as_deref().unwrap_or(""));
        item.evidence.dependencies = edges;
        bases.insert((name, version), items.len());
        items.push(item);
    }
    pip::keep_repeated(&mut items);
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    const LOCK: &str = r#"# This file is @generated by PDM.
# It is not intended for manual editing.

[metadata]
groups = ["default", "test"]
strategy = ["inherit_metadata"]
lock_version = "4.5.0"
content_hash = "sha256:26e33e854bc59dacbc9c38890c020e0626de4d6bc0c90790a2daa83f46c281d4"

[[metadata.targets]]
requires_python = ">=3.10"

[[package]]
name = "coverage"
version = "7.6.1"
requires_python = ">=3.8"
summary = "Code coverage measurement for Python"
groups = ["test"]
files = [
    {file = "coverage-7.6.1-py3-none-any.whl", hash = "sha256:aaaa"},
]

[[package]]
name = "coverage"
version = "7.6.1"
extras = ["toml"]
requires_python = ">=3.8"
summary = "Code coverage measurement for Python"
groups = ["test"]
dependencies = [
    "coverage==7.6.1",
    "tomli; python_full_version <= \"3.11.0a6\"",
]
files = [
    {file = "coverage-7.6.1-py3-none-any.whl", hash = "sha256:aaaa"},
]

[[package]]
name = "Requests"
version = "2.32.3"
requires_python = ">=3.8"
summary = "Python HTTP for Humans."
groups = ["default"]
dependencies = [
    "certifi>=2017.4.17",
    "idna<4,>=2.5",
    "urllib3==2.2.3",
]
files = [
    {url = "https://files.pythonhosted.org/packages/f9/9b/requests-2.32.3-py3-none-any.whl", hash = "sha256:bbbb"},
]

[[package]]
name = "corp-fork"
version = "0.2.0"
git = "https://deploy:hunter2@git.corp.example/team/fork.git"
ref = "main"
revision = "1234567890abcdef1234567890abcdef12345678"
summary = ""
groups = ["default"]

[[package]]
name = "sub-package"
version = "0.1.0"
path = "./packages/sub"
editable = true
summary = ""
groups = ["default", "test"]
dependencies = [
    "corp-fork @ git+https://deploy:hunter2@git.corp.example/team/fork.git@main",
]

[[package]]
name = "demo"
version = "0.1.0"
path = "."
summary = ""
groups = ["default"]
"#;

    #[test]
    fn lock_groups_sources_and_extras() {
        let items = Pdm.parse_file(&file("pdm.lock", LOCK)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v4.5.0 · Gruppen default, test · Strategie inherit_metadata");
        assert_eq!(items.len(), 5, "an extra and the project itself are no extra items: {items:?}");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let coverage = find("coverage");
        assert_eq!(coverage.source, SourceClass::Unknown, "file names alone name no index");
        assert_eq!(coverage.evidence.context, "test");
        assert_eq!(coverage.evidence.dependencies.len(), 1, "the extra's edges, not the base itself");
        let requests = find("requests");
        assert_eq!(
            (requests.source, requests.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("files.pythonhosted.org"))
        );
        let urllib3 = requests.evidence.dependencies.iter().find(|d| d.name == "urllib3").unwrap();
        assert!(urllib3.exact && urllib3.requirement == "2.2.3");
        assert_eq!(find("corp-fork").source, SourceClass::Git);
        let sub = find("sub-package");
        assert_eq!((sub.source, sub.evidence.context.as_str()), (SourceClass::Path, "default,test"));
        assert!(sub.detail.contains("editierbar"));
        assert_eq!(sub.evidence.dependencies[0].requirement, "Direktverweis");
        let text = format!("{items:?}");
        assert!(!text.contains("hunter2") && !text.contains("team/fork"), "no URL is kept");
        assert!(Pdm.parse_file(&file("pdm.lock", "[metadata\nlock_version =")).is_err());
    }

    #[test]
    fn old_lock_uses_sections() {
        let lock = "[[package]]\nname = \"click\"\nversion = \"8.0.1\"\nsections = [\"default\"]\n\n[metadata]\nlock_version = \"3.1\"\n";
        let items = Pdm.parse_file(&file("pdm.lock", lock)).unwrap();
        assert_eq!(items[1].evidence.context, "default");
    }

    #[test]
    fn pyproject_beside_the_lockfile() {
        let text = r#"
[project]
name = "demo"
requires-python = ">=3.10"
dependencies = ["requests>=2.32"]

[dependency-groups]
test = ["pytest>=8"]

[tool.pdm.dev-dependencies]
lint = ["ruff>=0.5"]
dev = ["-e file:///${PROJECT_ROOT}/packages/sub#egg=sub-package", "demo[test]"]

[[tool.pdm.source]]
name = "private"
url = "https://${PDM_USER}:${PDM_PASSWORD}@pkgs.corp.example/simple"
verify_ssl = true
"#;
        let siblings = vec!["pdm.lock".to_string()];
        let found = FoundFile { name: "pyproject.toml", rel_dir: "", content: text, siblings: &siblings };
        let items = Pdm.parse_file(&found).unwrap();
        assert_eq!(items[0].detail, "Projekt demo · 4 Abhängigkeiten · Python >=3.10 · private Quelle");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(find("requests").source, SourceClass::PrivateRegistry, "a private source may serve any package");
        assert_eq!(find("requests").source_host.as_deref(), Some("pkgs.corp.example"));
        assert_eq!(find("pytest").evidence.context, "test");
        assert_eq!(find("ruff").evidence.context, "lint");
        let sub = find("sub-package");
        assert_eq!((sub.source, sub.evidence.context.as_str()), (SourceClass::Path, "dev"));
        assert!(items.iter().all(|i| i.name != "demo"));
        assert!(!format!("{items:?}").contains("PDM_PASSWORD"));
        assert!(Pdm.parse_file(&file("pyproject.toml", text)).unwrap().is_empty(), "without pdm.lock pip reports it");
    }
}
