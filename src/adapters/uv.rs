//! uv: uv.lock (resolved, with explicit sources), uv.toml and the
//! pyproject.toml beside a uv.lock (declared, with `[tool.uv.sources]`,
//! indexes and the older `dev-dependencies`). Environments are recognised
//! by the pip adapter via `pyvenv.cfg` and attributed to uv when the config
//! says so. The uv cache is never touched (PROJEKTPLAN §10.4).
use super::pip::{self, Indexes, Source};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::toml_table;
use std::collections::HashMap;

pub struct Uv;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "uv",
    name: "uv",
    markers: &["uv.lock", "uv.toml", "pyproject.toml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "uv.lock (version 1)",
        "uv.toml",
        "pyproject.toml mit uv.lock ([tool.uv]: sources, index, dev-dependencies)",
    ],
    limits: "uv-Tools nur mit eigener Freigabe; Cache und fremde Konfigurationen werden nicht gelesen; pyproject.toml von Workspace-Mitgliedern ohne eigenes uv.lock zählt zu pip.",
};

impl Adapter for Uv {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name {
            "uv.lock" => bounded(lock_items(file)?),
            "uv.toml" => {
                let table = toml_table(file.content).map_err(|e| e.to_string())?;
                let indexes = indexes(Some(&table));
                let private = indexes
                    .unpinned
                    .iter()
                    .chain(indexes.named.iter().map(|(_, source)| source))
                    .any(|(class, _)| *class != SourceClass::PublicRegistry);
                let detail = if private { "uv-Konfiguration · private Indexquelle" } else { "uv-Konfiguration" };
                Ok(vec![manifest("uv", file, detail)])
            }
            _ => match pip::owned_pyproject(file, "uv")? {
                Some(table) => bounded(pyproject_items(file, &table)),
                None => Ok(Vec::new()),
            },
        }
    }
}

/// `[[index]]` of `[tool.uv]` or a uv.toml plus the older `index-url`,
/// `extra-index-url` and `find-links`. An `explicit` index serves only the
/// packages pinned to it in `[tool.uv.sources]`.
pub(super) fn indexes(uv: Option<&toml::Table>) -> Indexes {
    let mut indexes = Indexes::default();
    let Some(uv) = uv else { return indexes };
    for index in uv.get("index").and_then(toml::Value::as_array).into_iter().flatten().take(100) {
        let Some(url) = index.get("url").and_then(toml::Value::as_str) else { continue };
        let source = pip::index_source(url);
        if index.get("explicit").and_then(toml::Value::as_bool) != Some(true) {
            indexes.unpinned.push(source.clone());
        }
        if let Some(name) = index.get("name").and_then(toml::Value::as_str) {
            indexes.named.push((name.to_string(), source));
        }
    }
    let lists = ["extra-index-url", "find-links"]
        .into_iter()
        .flat_map(|key| uv.get(key).and_then(toml::Value::as_array).into_iter().flatten())
        .filter_map(toml::Value::as_str);
    let urls = uv.get("index-url").and_then(toml::Value::as_str).into_iter().chain(lists);
    indexes.unpinned.extend(urls.take(100).map(pip::index_source));
    indexes
}

/// `[tool.uv.sources]` by normalised name with the source and its detail;
/// `None` marks a workspace member, which is no dependency.
fn sources(uv: Option<&toml::Table>, indexes: &Indexes) -> HashMap<String, Option<(Source, String)>> {
    let mut pins = HashMap::new();
    let table = uv.and_then(|u| u.get("sources")).and_then(toml::Value::as_table);
    for (name, value) in table.into_iter().flatten().take(10_000) {
        // Several sources with markers: the first stands for all of them.
        let spec = match value {
            toml::Value::Array(list) => list.first(),
            other => Some(other),
        };
        let Some(spec) = spec.and_then(toml::Value::as_table) else { continue };
        let pin = if spec.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
            None
        } else if spec.contains_key("git") {
            Some(((SourceClass::Git, None), " · Git-Quelle".to_string()))
        } else if spec.contains_key("path") {
            Some(((SourceClass::Path, None), " · Pfadquelle".to_string()))
        } else if let Some(url) = spec.get("url").and_then(toml::Value::as_str) {
            Some((pip::direct_source(url), " · URL-Quelle".to_string()))
        } else if let Some(index) = spec.get("index").and_then(toml::Value::as_str) {
            Some((indexes.named(index), format!(" · Index {index}")))
        } else {
            continue;
        };
        pins.insert(pip::normalize(name), pin);
    }
    pins
}

fn pyproject_items(file: &FoundFile<'_>, table: &toml::Table) -> Vec<InventoryItem> {
    let uv = pip::tool(table, "uv");
    let indexes = indexes(uv);
    let mut declared = pip::standard_declarations(table);
    pip::strings(uv.and_then(|u| u.get("dev-dependencies")), "dev", pip::project_name(table), &mut declared);
    let pins = sources(uv, &indexes);
    let declared = declared
        .into_iter()
        .filter_map(|mut declared| {
            if declared.source.is_none()
                && let Some(pin) = pins.get(&declared.name)
            {
                let (source, note) = pin.as_ref()?;
                declared.source = Some(source.clone());
                declared.note.push_str(note);
            }
            Some(declared)
        })
        .collect();
    pip::pyproject_result(file, "uv", table, None, declared, &indexes.default_source())
}

fn lock_items(file: &FoundFile<'_>) -> AdapterResult {
    let table = toml_table(file.content).map_err(|e| e.to_string())?;
    let version = table.get("version").and_then(|v| v.as_integer()).unwrap_or(0);
    let mut items = vec![manifest("uv", file, format!("Lockfile v{version}"))];
    for package in table.get("package").and_then(|p| p.as_array()).into_iter().flatten() {
        let Some(name) = package.get("name").and_then(|n| n.as_str()) else { continue };
        let source = package.get("source").and_then(|s| s.as_table());
        let (class, host) = match source {
            Some(s) if s.contains_key("editable") || s.contains_key("virtual") => continue, // the project itself
            Some(s) if s.contains_key("path") || s.contains_key("directory") => (SourceClass::Path, None),
            Some(s) if s.contains_key("git") => (SourceClass::Git, None),
            Some(s) => s
                .get("registry")
                .or_else(|| s.get("url"))
                .and_then(|u| u.as_str())
                .map_or((SourceClass::Unknown, None), |url| classify_url(url, super::pip::PUBLIC)),
            None => (SourceClass::Unknown, None),
        };
        let mut item = InventoryItem::new(ItemCategory::Package, "uv", super::pip::normalize(name), file.rel_path())
            .version(package.get("version").and_then(|v| v.as_str()).unwrap_or_default())
            .state(InstallState::Resolved)
            .source(class, host)
            .detail("aufgelöst laut uv.lock");
        item.evidence.qualifier = format!(
            "{}|{}|{}",
            item.version.as_deref().unwrap_or(""),
            class,
            item.source_host.as_deref().unwrap_or("")
        );
        item.evidence.dependencies = package
            .get("dependencies")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
            .filter_map(|d| {
                Some(crate::domain::PackageDependency {
                    name: super::pip::normalize(d.get("name")?.as_str()?),
                    requirement: d.get("version").and_then(|v| v.as_str()).unwrap_or("").into(),
                    exact: d.get("version").is_some(),
                    kind: "lock (Marker ungeprüft)".into(),
                })
            })
            .collect();
        items.push(item);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    #[test]
    fn lock_sources_are_explicit() {
        let lock = r#"version = 1
[[package]]
name = "demo"
version = "0.1.0"
source = { editable = "." }
[[package]]
name = "Requests"
version = "2.32.3"
source = { registry = "https://pypi.org/simple" }
[[package]]
name = "internal"
version = "1.0.0"
source = { registry = "https://pkgs.corp.example/simple" }
"#;
        let items = Uv.parse_file(&file("uv.lock", lock)).unwrap();
        assert_eq!(items.len(), 3, "project itself is not a dependency");
        let requests = items.iter().find(|i| i.name == "requests").unwrap();
        assert_eq!(requests.source, SourceClass::PublicRegistry);
        assert_eq!(requests.source_host.as_deref(), Some("pypi.org"));
        assert_eq!(items.iter().find(|i| i.name == "internal").unwrap().source, SourceClass::PrivateRegistry);
    }

    #[test]
    fn pyproject_only_with_lock() {
        assert!(Uv.parse_file(&file("pyproject.toml", "[project]\nname='x'")).unwrap().is_empty());
    }

    #[test]
    fn pyproject_sources_indexes_and_legacy_dev_dependencies() {
        let text = r#"
[project]
name = "app"
dependencies = ["torch>=2.3", "corp-utils", "shared", "local-lib", "fork", "requests"]

[tool.uv]
dev-dependencies = ["pytest>=8"]

[tool.uv.sources]
torch = [{ index = "pytorch-cpu", marker = "sys_platform != 'darwin'" }]
corp-utils = { index = "corp" }
shared = { workspace = true }
local-lib = { path = "../local-lib", editable = true }
fork = { git = "https://github.com/me/fork", rev = "abc" }

[[tool.uv.index]]
name = "pytorch-cpu"
url = "https://download.pytorch.org/whl/cpu"
explicit = true

[[tool.uv.index]]
name = "corp"
url = "https://__token__:secret-value@pkgs.corp.example/simple"
explicit = true
"#;
        let siblings = vec!["uv.lock".to_string()];
        let found = FoundFile { name: "pyproject.toml", rel_dir: "", content: text, siblings: &siblings };
        let items = Uv.parse_file(&found).unwrap();
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        assert!(items.iter().all(|i| i.ecosystem == "uv"));
        assert!(items.iter().all(|i| i.name != "shared"), "workspace members are no dependencies");
        let corp = find("corp-utils");
        assert_eq!(
            (corp.source, corp.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("pkgs.corp.example"))
        );
        assert!(corp.detail.ends_with("Index corp"));
        assert_eq!(find("torch").source_host.as_deref(), Some("download.pytorch.org"));
        assert_eq!(find("local-lib").source, SourceClass::Path);
        assert_eq!(find("fork").source, SourceClass::Git);
        assert_eq!(find("requests").source, SourceClass::Unknown, "explicit indexes serve only pinned packages");
        assert_eq!(find("pytest").evidence.context, "dev");
        assert!(!format!("{items:?}").contains("secret-value"));
    }

    #[test]
    fn default_index_applies_to_unpinned_packages() {
        let text = "[project]\nname='x'\ndependencies=['requests']\n[[tool.uv.index]]\nname='mirror'\nurl='https://mirror.corp.example/simple'\ndefault=true\n";
        let siblings = vec!["uv.lock".to_string()];
        let found = FoundFile { name: "pyproject.toml", rel_dir: "", content: text, siblings: &siblings };
        let items = Uv.parse_file(&found).unwrap();
        assert_eq!(items[1].source, SourceClass::PrivateRegistry);
        let config = Uv.parse_file(&file("uv.toml", "index-url = 'https://pkgs.corp.example/simple'\n")).unwrap();
        assert_eq!(config[0].detail, "uv-Konfiguration · private Indexquelle");
    }
}
