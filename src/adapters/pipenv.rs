//! Pipenv: Pipfile (declared) and Pipfile.lock (resolved). Index URLs are
//! classified and dropped; `[requires]` is a runtime pin and `[scripts]`
//! are neither run nor kept. No pipenv command runs, and `.env` files and
//! environment variables inside source URLs are not resolved.
use super::pip::{self, Declared, Indexes, Source};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::{json, toml_table};

pub struct Pipenv;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "pipenv",
    name: "Pipenv",
    markers: &["Pipfile", "Pipfile.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["Pipfile (Quellen, packages, dev-packages, eigene Kategorien)", "Pipfile.lock (pipfile-spec 6)"],
    limits: "Kein pipenv-Aufruf; .env und Umgebungsvariablen in Quell-URLs werden nicht aufgelöst; Marker werden nicht ausgewertet, Skripte nicht ausgeführt.",
};

impl Adapter for Pipenv {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("Pipfile.lock") {
            return bounded(lock_items(file)?);
        }
        bounded(pipfile(file)?)
    }
}

const VCS: &[&str] = &["git", "hg", "svn", "bzr"];

/// `[[source]]`: unpinned packages come from the first source only, unless
/// `install_search_all_sources` restores the search across all of them.
fn indexes(sources: Option<&toml::Value>, search_all: bool) -> Indexes {
    let mut indexes = Indexes::default();
    for source in sources.and_then(toml::Value::as_array).into_iter().flatten().take(100) {
        let Some(url) = source.get("url").and_then(toml::Value::as_str) else { continue };
        let class = pip::index_source(url);
        if search_all || indexes.unpinned.is_empty() {
            indexes.unpinned.push(class.clone());
        }
        if let Some(name) = source.get("name").and_then(toml::Value::as_str) {
            indexes.named.push((name.to_string(), class));
        }
    }
    indexes
}

/// A Pipfile entry: `"*"`, a constraint or a table; `path = "."` is the
/// project itself.
fn dependency(name: &str, value: &toml::Value, context: &str, indexes: &Indexes) -> Option<Declared> {
    let mut declared = Declared::new(pip::normalize(name), context);
    match value {
        toml::Value::String(spec) => {
            declared.version = pip::pinned(spec).map(str::to_string);
            declared.spec = spec.clone();
        }
        toml::Value::Table(spec) => {
            let text = |key: &str| spec.get(key).and_then(toml::Value::as_str);
            if let Some(path) = text("path") {
                if path.trim_end_matches(['/', '\\']) == "." {
                    return None;
                }
                declared = declared.with_source((SourceClass::Path, None));
            } else if VCS.iter().any(|vcs| spec.contains_key(*vcs)) {
                declared = declared.with_source((SourceClass::Git, None));
            } else if let Some(url) = text("file") {
                declared = declared.with_source(pip::direct_source(url));
            } else if let Some(version) = text("version") {
                declared.version = pip::pinned(version).map(str::to_string);
                declared.spec = version.to_string();
            }
            if let Some(index) = text("index") {
                declared.source = Some(indexes.named(index));
                declared.note.push_str(&format!(" · Index {index}"));
            }
            if spec.get("editable").and_then(toml::Value::as_bool) == Some(true) {
                declared.note.push_str(" · editierbar");
            }
        }
        _ => return None,
    }
    Some(declared)
}

fn pipfile(file: &FoundFile<'_>) -> AdapterResult {
    let table = toml_table(file.content).map_err(|e| e.to_string())?;
    let search_all =
        table.get("pipenv").and_then(|p| p.get("install_search_all_sources")).and_then(toml::Value::as_bool);
    let indexes = indexes(table.get("source"), search_all == Some(true));
    let mut declared = Vec::new();
    for (category, packages) in &table {
        let context = match category.as_str() {
            "packages" => "",
            "dev-packages" => "dev",
            "source" | "requires" | "scripts" | "pipenv" => continue,
            other => other,
        };
        for (name, spec) in packages.as_table().into_iter().flatten().take(10_000) {
            declared.extend(dependency(name, spec, context, &indexes));
        }
    }
    let mut detail = format!("Pipfile · {} Abhängigkeiten", declared.len());
    let requires = table.get("requires");
    let python = requires.and_then(|r| r.get("python_full_version").or_else(|| r.get("python_version")));
    if let Some(python) = python.and_then(toml::Value::as_str) {
        detail.push_str(&format!(" · Python {python}"));
    }
    if table.get("scripts").and_then(toml::Value::as_table).is_some_and(|scripts| !scripts.is_empty()) {
        detail.push_str(" · Skripte vorhanden (nicht ausgeführt)");
    }
    let default = indexes.default_source();
    let mut items = vec![manifest("pipenv", file, detail)];
    items.extend(declared.into_iter().map(|d| d.item(file, "pipenv", &default)));
    pip::keep_repeated(&mut items);
    Ok(items)
}

/// Pipfile.lock: `default` and `develop` map to the Pipfile's categories;
/// an entry's `index` names one of `_meta.sources`, whose URL is dropped.
fn lock_items(file: &FoundFile<'_>) -> AdapterResult {
    let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
    let Some(categories) = value.as_object() else { return Err("Pipfile.lock ohne JSON-Objekt".into()) };
    let meta = &value["_meta"];
    let sources: Vec<(&str, Source)> = meta["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .take(100)
        .filter_map(|source| Some((source["name"].as_str()?, pip::index_source(source["url"].as_str()?))))
        .collect();
    let spec = meta["pipfile-spec"].as_i64().map_or_else(|| "?".to_string(), |spec| spec.to_string());
    let mut detail = format!("Lockfile (pipfile-spec {spec})");
    let requires = &meta["requires"];
    if let Some(python) = requires["python_full_version"].as_str().or_else(|| requires["python_version"].as_str()) {
        detail.push_str(&format!(" · Python {python}"));
    }
    let mut items = vec![manifest("pipenv", file, detail)];
    for (category, packages) in categories {
        let context = match category.as_str() {
            "_meta" => continue,
            "default" => "",
            "develop" => "dev",
            other => other,
        };
        for (name, entry) in packages.as_object().into_iter().flatten().take(20_000) {
            let (class, host) = if VCS.iter().any(|vcs| entry.get(*vcs).is_some()) {
                (SourceClass::Git, None)
            } else if let Some(path) = entry["path"].as_str() {
                if path.trim_end_matches(['/', '\\']) == "." {
                    continue; // the project itself
                }
                (SourceClass::Path, None)
            } else if let Some(url) = entry["file"].as_str() {
                pip::direct_source(url)
            } else {
                let index = entry["index"].as_str();
                index
                    .and_then(|index| sources.iter().find(|(name, _)| name.eq_ignore_ascii_case(index)))
                    .map_or(pip::UNKNOWN, |(_, source)| source.clone())
            };
            let mut detail = String::from("aufgelöst laut Pipfile.lock");
            if entry["editable"].as_bool() == Some(true) {
                detail.push_str(" · editierbar");
            }
            let version = entry["version"].as_str().map(|v| v.trim_start_matches('=')).unwrap_or_default();
            let mut item = InventoryItem::new(ItemCategory::Package, "pipenv", pip::normalize(name), file.rel_path())
                .version(version)
                .state(InstallState::Resolved)
                .source(class, host)
                .detail(detail);
            item.evidence.context = context.to_string();
            items.push(item);
        }
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    #[test]
    fn pipfile_sources_categories_and_specs() {
        let text = r#"
[[source]]
url = "https://pypi.org/simple"
verify_ssl = true
name = "pypi"

[[source]]
url = "https://${CORP_USER}:${CORP_TOKEN}@pkgs.corp.example/simple"
verify_ssl = true
name = "corp"

[packages]
requests = "*"
flask = "==3.0.3"
django = {version = ">=4.2", extras = ["bcrypt"]}
corp-lib = {version = "*", index = "corp"}
flask-login = {git = "https://github.com/maxcountryman/flask-login.git", ref = "main"}
my-package = {path = "./my-package", editable = true}
demo = {path = ".", editable = true}

[dev-packages]
pytest = ">=8.0"

[docs]
sphinx = ">=7"

[requires]
python_version = "3.12"

[scripts]
serve = "flask run"
"#;
        let items = Pipenv.parse_file(&file("Pipfile", text)).unwrap();
        assert_eq!(items[0].detail, "Pipfile · 8 Abhängigkeiten · Python 3.12 · Skripte vorhanden (nicht ausgeführt)");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let requests = find("requests");
        assert_eq!((requests.source, requests.source_host.as_deref()), (SourceClass::PublicRegistry, Some("pypi.org")));
        assert_eq!(requests.detail, "Spezifikation *");
        assert_eq!(find("flask").version.as_deref(), Some("3.0.3"));
        let corp = find("corp-lib");
        assert_eq!(
            (corp.source, corp.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("pkgs.corp.example"))
        );
        assert_eq!(find("flask-login").source, SourceClass::Git);
        assert!(find("my-package").detail.contains("editierbar"));
        assert_eq!(find("pytest").evidence.context, "dev");
        assert_eq!(find("sphinx").evidence.context, "docs");
        assert!(items.iter().all(|i| i.name != "demo" && i.name != "python"));
        assert!(!format!("{items:?}").contains("CORP_TOKEN"));
        assert!(Pipenv.parse_file(&file("Pipfile", "[packages\nrequests =")).is_err());
    }

    #[test]
    fn lockfile_indexes_categories_and_sources() {
        let lock = r#"{
    "_meta": {
        "hash": {"sha256": "8f1f9e2a"},
        "pipfile-spec": 6,
        "requires": {"python_version": "3.12"},
        "sources": [
            {"name": "pypi", "url": "https://pypi.org/simple", "verify_ssl": true},
            {"name": "corp", "url": "https://user:hunter2@pkgs.corp.example/simple", "verify_ssl": true}
        ]
    },
    "default": {
        "requests": {"hashes": ["sha256:70761cfe03c773ceb22aa2f671b4757976145175cdfca038c02654d061d6dcc6"], "index": "pypi", "markers": "python_version >= '3.8'", "version": "==2.32.3"},
        "corp-lib": {"hashes": [], "index": "corp", "version": "==1.4.0"},
        "flask-login": {"git": "https://github.com/maxcountryman/flask-login.git", "ref": "0123456789abcdef0123456789abcdef01234567"},
        "my-package": {"editable": true, "path": "./my-package"},
        "demo": {"editable": true, "path": "."},
        "wheel-pkg": {"file": "https://files.pythonhosted.org/packages/wheel_pkg-1.0-py3-none-any.whl"}
    },
    "develop": {
        "requests": {"index": "pypi", "version": "==2.31.0"},
        "pytest": {"index": "pypi", "version": "==8.3.3"}
    },
    "docs": {
        "sphinx": {"version": "==7.4.7"}
    }
}"#;
        let items = Pipenv.parse_file(&file("Pipfile.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile (pipfile-spec 6) · Python 3.12");
        assert_eq!(items.len(), 9, "{items:?}");
        let find = |name: &str, context: &str| {
            items.iter().find(|i| i.name == name && i.evidence.context == context).unwrap_or_else(|| panic!("{name}"))
        };
        let requests = find("requests", "");
        assert_eq!((requests.version.as_deref(), requests.source), (Some("2.32.3"), SourceClass::PublicRegistry));
        assert_eq!(requests.install_state, Some(InstallState::Resolved));
        assert_eq!(find("requests", "dev").version.as_deref(), Some("2.31.0"));
        assert_eq!(find("corp-lib", "").source, SourceClass::PrivateRegistry);
        assert_eq!(find("flask-login", "").source, SourceClass::Git);
        assert_eq!(find("my-package", "").source, SourceClass::Path);
        assert_eq!(find("wheel-pkg", "").source, SourceClass::PublicRegistry);
        assert_eq!(find("sphinx", "docs").source, SourceClass::Unknown, "no index named");
        assert!(!format!("{items:?}").contains("hunter2"));
        assert!(Pipenv.parse_file(&file("Pipfile.lock", "{\"default\": ")).is_err());
    }
}
