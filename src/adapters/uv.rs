//! uv: uv.lock (resolved, with explicit sources), uv.toml and pyproject.toml
//! of uv projects. Environments are recognised by the pip adapter via
//! `pyvenv.cfg` and attributed to uv when the config says so. The uv cache
//! is never touched (PROJEKTPLAN §10.4).
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::toml_table;

pub struct Uv;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "uv",
    name: "uv",
    markers: &["uv.lock", "uv.toml", "pyproject.toml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    updates: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["uv.lock (version 1)", "uv.toml", "pyproject.toml mit uv.lock"],
    limits: "Tool-, Skript- und Cachekontext außerhalb freigegebener Ordner wird nicht gelesen.",
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
                let private = table
                    .get("index")
                    .and_then(|i| i.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|i| i.get("url")?.as_str())
                    .chain(table.get("index-url").and_then(|v| v.as_str()))
                    .any(|url| classify_url(url, super::pip::PUBLIC).0 != SourceClass::PublicRegistry);
                let detail = if private { "uv-Konfiguration · private Indexquelle" } else { "uv-Konfiguration" };
                Ok(vec![manifest("uv", file, detail)])
            }
            _ if file.siblings.iter().any(|s| s == "uv.lock") => bounded(super::pip::pyproject_items(file, "uv")?),
            _ => Ok(Vec::new()),
        }
    }
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
        items.push(
            InventoryItem::new(ItemCategory::Package, "uv", super::pip::normalize(name), file.rel_path())
                .version(package.get("version").and_then(|v| v.as_str()).unwrap_or_default())
                .state(InstallState::Resolved)
                .source(class, host)
                .detail("aufgelöst laut uv.lock"),
        );
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
}
