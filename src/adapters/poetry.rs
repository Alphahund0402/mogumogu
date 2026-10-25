//! Poetry: poetry.lock (resolved, lock formats 1.x to 2.1) and the
//! pyproject.toml Poetry owns (declared: `[tool.poetry]` dependencies,
//! groups, extras and sources, plus the PEP 621 tables of Poetry 2). No
//! Poetry command, plugin or build backend runs; poetry.toml and the user's
//! Poetry configuration are not read. A lock entry without
//! `[package.source]` names no index, so its source stays unknown.
use super::pip::{self, Declared, Indexes};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::toml_table;
use std::collections::HashMap;

pub struct Poetry;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "poetry",
    name: "Poetry",
    markers: &["poetry.lock", "pyproject.toml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "poetry.lock (lock-version 1.x, 2.0, 2.1)",
        "pyproject.toml: [tool.poetry] (Abhängigkeiten, Gruppen, Extras, Quellen) und PEP 621 (Poetry 2)",
    ],
    limits: "Kein Poetry-Aufruf; poetry.toml und Benutzerkonfiguration werden nicht gelesen – ohne Quellenangabe bleibt die Quelle unbekannt; Marker und Python-Einschränkungen werden nicht ausgewertet.",
};

impl Adapter for Poetry {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("poetry.lock") {
            return bounded(lock_items(file)?);
        }
        match pip::owned_pyproject(file, "poetry")? {
            Some(table) => bounded(pyproject_items(file, &table)),
            None => Ok(Vec::new()),
        }
    }
}

/// `[tool.poetry]` declares dependencies (Poetry 1 tables or groups).
pub(super) fn declares_dependencies(table: &toml::Table) -> bool {
    let Some(poetry) = pip::tool(table, "poetry") else { return false };
    poetry.contains_key("dependencies")
        || poetry.contains_key("dev-dependencies")
        || poetry
            .get("group")
            .and_then(toml::Value::as_table)
            .is_some_and(|groups| groups.values().any(|group| group.get("dependencies").is_some()))
}

/// `[[tool.poetry.source]]`: primary sources (the default priority, also
/// the older `default`) serve unpinned packages; supplemental and explicit
/// ones serve only what names them. PyPI declared by name has no URL.
pub(super) fn indexes(poetry: Option<&toml::Table>) -> Indexes {
    let mut indexes = Indexes::default();
    let sources = poetry.and_then(|p| p.get("source")).and_then(toml::Value::as_array);
    for source in sources.into_iter().flatten().take(100) {
        let Some(name) = source.get("name").and_then(toml::Value::as_str) else { continue };
        let class = match source.get("url").and_then(toml::Value::as_str) {
            Some(url) => pip::index_source(url),
            None if name.eq_ignore_ascii_case("pypi") => (SourceClass::PublicRegistry, Some("pypi.org".into())),
            None => (SourceClass::Unknown, None),
        };
        let primary = match source.get("priority").and_then(toml::Value::as_str) {
            Some(priority) => matches!(priority, "primary" | "default"),
            None => source.get("secondary").and_then(toml::Value::as_bool) != Some(true),
        };
        if primary {
            indexes.unpinned.push(class.clone());
        }
        indexes.named.push((name.to_string(), class));
    }
    indexes
}

/// Poetry reads a bare version (`1.2.3`) as an exact pin, like `==1.2.3`.
fn exact(spec: &str) -> Option<String> {
    let spec = spec.trim();
    let bare = spec.starts_with(|c: char| c.is_ascii_digit())
        && spec.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '!' | '-' | '_'));
    if bare { Some(spec.to_string()) } else { pip::pinned(spec).map(str::to_string) }
}

/// `[tool.poetry.extras]`: extra name and the normalised names it lists.
/// Every optional dependency searches all of them, so 2,000 names at most.
fn extras(poetry: Option<&toml::Table>) -> Vec<(String, Vec<String>)> {
    let table = poetry.and_then(|p| p.get("extras")).and_then(toml::Value::as_table);
    let mut budget = 2_000_usize;
    table
        .into_iter()
        .flatten()
        .take(1_000)
        .map(|(extra, names)| {
            let names = names.as_array().into_iter().flatten().filter_map(toml::Value::as_str);
            let names: Vec<String> = names.take(budget).map(pip::normalize).collect();
            budget -= names.len();
            (extra.clone(), names)
        })
        .collect()
}

/// One Poetry dependency: a constraint, a table or several tables with
/// markers. An optional main dependency belongs to the extras listing it.
fn dependency(
    name: &str,
    value: &toml::Value,
    context: &str,
    indexes: &Indexes,
    extras: &[(String, Vec<String>)],
) -> Vec<Declared> {
    let mut declared = Declared::new(pip::normalize(name), context);
    let tables: Vec<&toml::Table> = match value {
        toml::Value::Table(table) => vec![table],
        toml::Value::Array(list) => list.iter().filter_map(toml::Value::as_table).collect(),
        _ => Vec::new(),
    };
    let first = match (value.as_str(), tables.first()) {
        (Some(spec), _) => {
            declared.version = exact(spec);
            declared.spec = spec.to_string();
            return vec![declared];
        }
        (None, Some(first)) => *first,
        _ => return Vec::new(),
    };
    if first.contains_key("git") {
        declared = declared.with_source((SourceClass::Git, None));
    } else if first.contains_key("path") {
        declared = declared.with_source((SourceClass::Path, None));
        if first.get("develop").and_then(toml::Value::as_bool) == Some(true) {
            declared.note.push_str(" · editierbar");
        }
    } else if let Some(url) = first.get("url").and_then(toml::Value::as_str) {
        declared = declared.with_source(pip::direct_source(url));
    } else {
        let specs: Vec<&str> = tables.iter().filter_map(|t| t.get("version").and_then(toml::Value::as_str)).collect();
        if let [spec] = specs.as_slice() {
            declared.version = exact(spec);
        }
        declared.spec = specs.join(" | ");
    }
    if let Some(source) = first.get("source").and_then(toml::Value::as_str) {
        declared.source = Some(indexes.named(source));
        declared.note.push_str(&format!(" · Quelle {source}"));
    }
    if context.is_empty() && first.get("optional").and_then(toml::Value::as_bool) == Some(true) {
        let listed: Vec<Declared> = extras
            .iter()
            .filter(|(_, names)| names.contains(&declared.name))
            .map(|(extra, _)| Declared { context: format!("extra:{extra}"), ..declared.clone() })
            .collect();
        if !listed.is_empty() {
            return listed;
        }
        declared.context = "optional".into();
    }
    vec![declared]
}

fn pyproject_items(file: &FoundFile<'_>, table: &toml::Table) -> Vec<InventoryItem> {
    let poetry = pip::tool(table, "poetry");
    let indexes = indexes(poetry);
    let extras = extras(poetry);
    let mut declared = pip::standard_declarations(table);
    // Poetry 2: an entry for a dependency `[project]` declares only adds its source.
    let mut known: HashMap<(String, String), usize> =
        declared.iter().enumerate().map(|(index, d)| ((d.name.clone(), d.context.clone()), index)).collect();
    let mut add =
        |declared: &mut Vec<Declared>, entry: Declared| match known.get(&(entry.name.clone(), entry.context.clone())) {
            Some(&index) if declared[index].source.is_none() => {
                declared[index].source = entry.source;
                declared[index].note.push_str(&entry.note);
            }
            Some(_) => {}
            None => {
                known.insert((entry.name.clone(), entry.context.clone()), declared.len());
                declared.push(entry);
            }
        };
    for (key, context) in [("dependencies", ""), ("dev-dependencies", "dev")] {
        let deps = poetry.and_then(|p| p.get(key)).and_then(toml::Value::as_table);
        for (name, spec) in deps.into_iter().flatten().take(10_000) {
            if !name.eq_ignore_ascii_case("python") {
                for entry in dependency(name, spec, context, &indexes, &extras) {
                    add(&mut declared, entry);
                }
            }
        }
    }
    let groups = poetry.and_then(|p| p.get("group")).and_then(toml::Value::as_table);
    for (group, config) in groups.into_iter().flatten().take(1_000) {
        let optional = config.get("optional").and_then(toml::Value::as_bool) == Some(true);
        let deps = config.get("dependencies").and_then(toml::Value::as_table);
        for (name, spec) in deps.into_iter().flatten().take(10_000) {
            for mut entry in dependency(name, spec, group, &indexes, &extras) {
                if optional {
                    entry.note.push_str(" · optionale Gruppe");
                }
                add(&mut declared, entry);
            }
        }
    }
    let python = poetry.and_then(|p| p.get("dependencies")).and_then(|d| d.get("python")).and_then(toml::Value::as_str);
    pip::pyproject_result(file, "poetry", table, python, declared, &indexes.default_source())
}

/// Constraints of `[package.dependencies]` as edges; a bare version is exact.
fn edges(dependencies: Option<&toml::Value>) -> Vec<PackageDependency> {
    let table = dependencies.and_then(toml::Value::as_table);
    table
        .into_iter()
        .flatten()
        .take(10_000)
        .map(|(name, spec)| {
            let requirement = match spec {
                toml::Value::String(spec) => spec.clone(),
                toml::Value::Table(table) => table.get("version").and_then(toml::Value::as_str).unwrap_or("").into(),
                toml::Value::Array(list) => list
                    .iter()
                    .filter_map(|t| t.get("version").and_then(toml::Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" | "),
                _ => String::new(),
            };
            let pin = exact(&requirement);
            PackageDependency {
                name: pip::normalize(name),
                exact: pin.is_some(),
                requirement: pin.unwrap_or(requirement),
                kind: "lock (Marker ungeprüft)".into(),
            }
        })
        .collect()
}

/// poetry.lock: `groups` (lock 2.1) or `category` (older locks) is the context.
fn lock_items(file: &FoundFile<'_>) -> AdapterResult {
    let table = toml_table(file.content).map_err(|e| e.to_string())?;
    let metadata = table.get("metadata");
    let text = |key: &str| metadata.and_then(|m| m.get(key)).and_then(toml::Value::as_str);
    let mut detail = format!("Lockfile v{}", text("lock-version").unwrap_or("?"));
    if let Some(python) = text("python-versions") {
        detail.push_str(&format!(" · Python {python}"));
    }
    let mut items = vec![manifest("poetry", file, detail)];
    for package in table.get("package").and_then(toml::Value::as_array).into_iter().flatten() {
        let Some(name) = package.get("name").and_then(toml::Value::as_str) else { continue };
        let version = package.get("version").and_then(toml::Value::as_str).unwrap_or_default();
        let source = package.get("source");
        let field = |key: &str| source.and_then(|s| s.get(key)).and_then(toml::Value::as_str);
        let kind = field("type");
        let (class, host) = match kind {
            Some("git") => (SourceClass::Git, None),
            Some("directory" | "file") => (SourceClass::Path, None),
            Some(_) => field("url").map_or((SourceClass::Unknown, None), pip::index_source),
            None => (SourceClass::Unknown, None),
        };
        let mut detail = String::from("aufgelöst laut poetry.lock");
        if let (Some("legacy"), Some(reference)) = (kind, field("reference")) {
            detail.push_str(&format!(" · Quelle {reference}"));
        }
        if package.get("optional").and_then(toml::Value::as_bool) == Some(true) {
            detail.push_str(" · optional");
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "poetry", pip::normalize(name), file.rel_path())
            .version(version)
            .state(InstallState::Resolved)
            .source(class, host)
            .detail(detail);
        let groups: Vec<&str> = package
            .get("groups")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|g| g.as_str())
            .collect();
        item.evidence.context = if groups.is_empty() {
            package.get("category").and_then(toml::Value::as_str).unwrap_or("").to_string()
        } else {
            groups.join(",")
        };
        item.evidence.qualifier = format!("{version}|{class}|{}", item.source_host.as_deref().unwrap_or(""));
        item.evidence.dependencies = edges(package.get("dependencies"));
        items.push(item);
    }
    pip::keep_repeated(&mut items);
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    const LOCK_2_1: &str = r#"# This file is automatically @generated by Poetry 2.1.3 and should not be changed by hand.

[[package]]
name = "certifi"
version = "2025.1.31"
description = "Python package for providing Mozilla's CA Bundle."
optional = false
python-versions = ">=3.6"
groups = ["main"]
files = [
    {file = "certifi-2025.1.31-py3-none-any.whl", hash = "sha256:ca78db4565a652026a4db2bcdf68f2fb589ea80d0be70e03929ed730746b84fe"},
]

[[package]]
name = "colorama"
version = "0.4.6"
description = "Cross-platform colored terminal text."
optional = false
python-versions = "!=3.0.*,!=3.1.*,!=3.2.*,!=3.3.*,!=3.4.*,!=3.5.*,!=3.6.*,>=2.7"
groups = ["main", "dev"]
markers = {main = "platform_system == \"Windows\"", dev = "sys_platform == \"win32\""}
files = []

[[package]]
name = "requests"
version = "2.32.3"
description = "Python HTTP for Humans."
optional = false
python-versions = ">=3.8"
groups = ["main"]
files = []

[package.dependencies]
certifi = ">=2017.4.17"
charset-normalizer = ">=2,<4"
idna = {version = ">=2.5,<4", markers = "python_version >= \"3.8\""}
urllib3 = "2.2.3"

[package.extras]
socks = ["PySocks (>=1.5.6,!=1.5.7)"]

[[package]]
name = "Corp_Lib"
version = "1.4.0"
description = ""
optional = true
python-versions = ">=3.9"
groups = ["main"]
files = []

[package.source]
type = "legacy"
url = "https://user:s3cret@pkgs.corp.example/simple"
reference = "corp"

[[package]]
name = "my-fork"
version = "0.3.0"
description = ""
optional = false
python-versions = "^3.9"
groups = ["main"]
files = []
develop = false

[package.source]
type = "git"
url = "https://github.com/me/my-fork.git"
reference = "main"
resolved_reference = "0123456789abcdef0123456789abcdef01234567"

[[package]]
name = "local-pkg"
version = "0.1.0"
description = ""
optional = false
python-versions = "*"
groups = ["dev"]
files = []
develop = true

[package.source]
type = "directory"
url = "../local-pkg"

[metadata]
lock-version = "2.1"
python-versions = "^3.11"
content-hash = "54b6e3e0b4a0b0b6b4e4d7bd2f6d7c4bc6e6d0d0e1f1f1f1f1f1f1f1f1f1f1f1"
"#;

    #[test]
    fn lock_2_1_groups_sources_and_edges() {
        let items = Poetry.parse_file(&file("poetry.lock", LOCK_2_1)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v2.1 · Python ^3.11");
        assert_eq!(items.len(), 7);
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let requests = find("requests");
        assert_eq!(
            (requests.install_state, requests.version.as_deref()),
            (Some(InstallState::Resolved), Some("2.32.3"))
        );
        assert_eq!(requests.source, SourceClass::Unknown, "no [package.source]: no index named");
        assert_eq!(requests.evidence.dependencies.len(), 4);
        let urllib3 = requests.evidence.dependencies.iter().find(|d| d.name == "urllib3").unwrap();
        assert!(urllib3.exact && urllib3.requirement == "2.2.3");
        let idna = requests.evidence.dependencies.iter().find(|d| d.name == "idna").unwrap();
        assert_eq!((idna.requirement.as_str(), idna.exact), (">=2.5,<4", false));
        assert_eq!(find("colorama").evidence.context, "main,dev");
        let corp = find("corp-lib");
        assert_eq!(
            (corp.source, corp.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("pkgs.corp.example"))
        );
        assert!(corp.detail.contains("Quelle corp") && corp.detail.contains("optional"));
        assert_eq!(find("my-fork").source, SourceClass::Git);
        assert_eq!(find("local-pkg").source, SourceClass::Path);
        assert_eq!(find("local-pkg").evidence.context, "dev");
        assert!(!format!("{items:?}").contains("s3cret"));
    }

    #[test]
    fn lock_1_1_categories_and_broken_lock() {
        let lock = r#"[[package]]
name = "requests"
version = "2.25.1"
description = "Python HTTP for Humans."
category = "main"
optional = false
python-versions = ">=2.7, !=3.0.*, !=3.1.*, !=3.2.*, !=3.3.*, !=3.4.*"

[package.dependencies]
certifi = ">=2017.4.17"

[[package]]
name = "pytest"
version = "6.2.2"
description = "pytest: simple powerful testing with Python"
category = "dev"
optional = false
python-versions = ">=3.6"

[metadata]
lock-version = "1.1"
python-versions = "^3.8"
content-hash = "abc"

[metadata.files]
requests = [
    {file = "requests-2.25.1-py2.py3-none-any.whl", hash = "sha256:c210084e36a42ae6b9219e00e48287def368a26d03a048ddad7bfee44f75871e"},
]
"#;
        let items = Poetry.parse_file(&file("poetry.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v1.1 · Python ^3.8");
        assert_eq!(items.iter().find(|i| i.name == "pytest").unwrap().evidence.context, "dev");
        assert_eq!(items.iter().find(|i| i.name == "requests").unwrap().evidence.context, "main");
        assert!(Poetry.parse_file(&file("poetry.lock", "[[package]\nname = ")).is_err());
    }

    #[test]
    fn poetry_1_tables_groups_extras_and_sources() {
        let text = r#"
[tool.poetry]
name = "demo"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.11"
requests = "^2.31"
numpy = "1.26.4"
corp-lib = { version = "^1.4", source = "corp" }
my-fork = { git = "https://github.com/me/my-fork.git", branch = "main" }
local-pkg = { path = "../local-pkg", develop = true }
wheel-pkg = { url = "https://user:secret@files.corp.example/wheel_pkg-1.0-py3-none-any.whl" }
foo = [
    { version = "<=1.9", python = ">=3.6,<3.8" },
    { version = "^2.0", python = ">=3.8" },
]
pdfkit = { version = "^1.0", optional = true }
lonely = { version = "^3", optional = true }

[tool.poetry.extras]
pdf = ["pdfkit"]
all = ["pdfkit"]

[tool.poetry.dev-dependencies]
black = "^24.1"

[tool.poetry.group.test.dependencies]
pytest = "^8.0"

[tool.poetry.group.docs]
optional = true

[tool.poetry.group.docs.dependencies]
mkdocs = "*"

[[tool.poetry.source]]
name = "corp"
url = "https://user:token@pkgs.corp.example/simple"
priority = "explicit"
"#;
        let items = Poetry.parse_file(&file("pyproject.toml", text)).unwrap();
        assert!(items[0].detail.starts_with("Projekt demo · 13 Abhängigkeiten · Python ^3.11"), "{}", items[0].detail);
        assert!(items.iter().all(|i| i.name != "python"), "the Python pin is no package");
        let find = |name: &str, context: &str| {
            items.iter().find(|i| i.name == name && i.evidence.context == context).unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(find("requests", "").source, SourceClass::Unknown);
        assert_eq!(find("numpy", "").version.as_deref(), Some("1.26.4"));
        let corp = find("corp-lib", "");
        assert_eq!(
            (corp.source, corp.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("pkgs.corp.example"))
        );
        assert_eq!(corp.detail, "Spezifikation ^1.4 · Quelle corp");
        assert_eq!(find("my-fork", "").source, SourceClass::Git);
        assert!(find("local-pkg", "").detail.contains("editierbar"));
        assert_eq!(find("wheel-pkg", "").source_host.as_deref(), Some("files.corp.example"));
        assert_eq!(find("foo", "").detail, "Spezifikation <=1.9 | ^2.0");
        find("pdfkit", "extra:pdf");
        find("pdfkit", "extra:all");
        find("lonely", "optional");
        find("black", "dev");
        find("pytest", "test");
        assert!(find("mkdocs", "docs").detail.ends_with("optionale Gruppe"));
        let text = format!("{items:?}");
        assert!(!text.contains("secret") && !text.contains("token"));
    }

    #[test]
    fn poetry_2_project_tables_with_a_primary_private_source() {
        let text = r#"
[project]
name = "demo"
requires-python = ">=3.11"
dependencies = ["requests (>=2.31,<3)", "corp-lib (>=1.4)"]

[project.optional-dependencies]
pdf = ["pdfkit (>=1.0)"]

[tool.poetry.dependencies]
corp-lib = { source = "corp" }

[tool.poetry.group.dev.dependencies]
pytest = "^8"

[[tool.poetry.source]]
name = "corp"
url = "https://pkgs.corp.example/simple"
priority = "primary"
"#;
        let items = Poetry.parse_file(&file("pyproject.toml", text)).unwrap();
        assert_eq!(items[0].detail, "Projekt demo · 4 Abhängigkeiten · Python >=3.11 · private Quelle");
        assert_eq!(items.iter().filter(|i| i.name == "corp-lib").count(), 1, "the tool entry only adds a source");
        let corp = items.iter().find(|i| i.name == "corp-lib").unwrap();
        assert_eq!(corp.detail, "Spezifikation >=1.4 · Quelle corp");
        let requests = items.iter().find(|i| i.name == "requests").unwrap();
        assert_eq!(requests.source, SourceClass::PrivateRegistry, "a primary source replaces PyPI");
        assert_eq!(items.iter().find(|i| i.name == "pdfkit").unwrap().evidence.context, "extra:pdf");
    }

    #[test]
    fn pyproject_of_another_manager_is_not_reported() {
        let text = "[project]\nname='x'\ndependencies=['requests']\n[tool.poetry]\npackage-mode=false\n";
        assert!(Poetry.parse_file(&file("pyproject.toml", text)).unwrap().is_empty());
    }
}
