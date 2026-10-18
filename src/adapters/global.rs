//! Narrow global inventories using the same bounded access as project adapters.
//! Roots are selected explicitly. No configuration/credential discovery or tools.
use super::{AdapterResult, DirAccess, bounded, classify_url};
use crate::domain::{InstallState, InventoryItem, ItemCategory, ScopeKind, SourceClass};
use crate::limits::PACKAGE_FILE_MAX_BYTES;
use crate::parsers::{json, toml_table, xml};
use std::collections::BTreeSet;

struct Below<'a> {
    access: &'a mut dyn DirAccess,
    prefix: Vec<String>,
}
impl DirAccess for Below<'_> {
    fn read_text(&mut self, path: &[&str], max: u64) -> Option<String> {
        let joined = self.prefix.iter().map(String::as_str).chain(path.iter().copied()).collect::<Vec<_>>();
        self.access.read_text(&joined, max)
    }
    fn list(&mut self, path: &[&str]) -> Vec<(String, bool)> {
        let joined = self.prefix.iter().map(String::as_str).chain(path.iter().copied()).collect::<Vec<_>>();
        self.access.list(&joined)
    }
}

pub fn inventory_global(kind: ScopeKind, access: &mut dyn DirAccess) -> AdapterResult {
    match kind {
        ScopeKind::NpmGlobal => npm(access),
        ScopeKind::Python => python(access, "", true),
        ScopeKind::UvTools => uv_tools(access),
        ScopeKind::CargoTools => cargo(access),
        ScopeKind::NugetCache => nuget(access),
        _ => Err("Globaler Bereich nicht unterstützt".into()),
    }
}

fn npm(access: &mut dyn DirAccess) -> AdapterResult {
    let mut items = Vec::new();
    let mut paths = Vec::new();
    for (name, is_dir) in access.list(&[]) {
        if !is_dir || name.starts_with('.') {
            continue;
        }
        if name.starts_with('@') {
            for (child, dir) in access.list(&[&name]) {
                if dir {
                    paths.push(vec![name.clone(), child]);
                }
            }
        } else {
            paths.push(vec![name]);
        }
    }
    for parts in paths {
        let rel = parts.join("/");
        let mut path = parts.iter().map(String::as_str).collect::<Vec<_>>();
        path.push("package.json");
        let Some(text) = access.read_text(&path, PACKAGE_FILE_MAX_BYTES) else {
            items.push(
                InventoryItem::new(ItemCategory::Package, "npm", &rel, &rel)
                    .detail("Ordner ohne Paketmetadaten; Installation und Version unbekannt"),
            );
            continue;
        };
        let value = json::parse(&text, 64).map_err(|e| e.to_string())?;
        let Some(name) = value["name"].as_str() else { return Err("npm-Paket ohne Namen".into()) };
        let (source, host) =
            value["_resolved"].as_str().map_or((SourceClass::Unknown, None), |u| classify_url(u, super::npm::PUBLIC));
        let mut item = InventoryItem::new(ItemCategory::Package, "npm", name, format!("{rel}/package.json"))
            .version(value["version"].as_str().unwrap_or_default())
            .state(InstallState::Installed)
            .source(source, host)
            .detail("Globale Paketmetadaten; CLI-Nutzung außerhalb von Projekten unbekannt");
        item.evidence.dependencies = super::npm::dependencies(&value);
        item.evidence.deprecated = value["deprecated"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        item.evidence.context = "global-tool".into();
        item.evidence.executables = match &value["bin"] {
            serde_json::Value::Object(bins) => {
                bins.keys().filter_map(|key| super::commands::basename(key)).take(50).collect()
            }
            serde_json::Value::String(_) => {
                name.rsplit('/').next().and_then(super::commands::basename).into_iter().collect()
            }
            _ => Vec::new(),
        };
        items.push(item);
    }
    bounded(items)
}

fn python(access: &mut dyn DirAccess, rel: &str, allow_site: bool) -> AdapterResult {
    let claim = super::DirClaim { adapter: "pip", category: ItemCategory::Environment, label: "Python-Paketbestand" };
    if allow_site && !access.list(&[]).iter().any(|(n, d)| *d && n.eq_ignore_ascii_case("Lib")) {
        return bounded(super::pip::installed_packages(access, &[], rel, "pip"));
    }
    super::Adapter::inspect_dir(&super::pip::Pip, claim, rel, access)
}

fn uv_tools(access: &mut dyn DirAccess) -> AdapterResult {
    let mut items = Vec::new();
    for (name, is_dir) in access.list(&[]) {
        if !is_dir || name.starts_with('.') {
            continue;
        }
        if access.read_text(&[&name, "uv-receipt.toml"], 256 * 1024).is_none() {
            continue;
        }
        let mut below = Below { access, prefix: vec![name.clone()] };
        let mut packages = python(&mut below, &name, false)?;
        for item in &mut packages {
            item.ecosystem = "uv".into();
            item.evidence.context = format!("global-tool:{name}");
            if item.category == ItemCategory::Environment {
                item.detail.push_str(" · uv-Toolbeleg vorhanden; Nutzung unbekannt");
            }
        }
        items.extend(packages);
    }
    bounded(items)
}

fn cargo(access: &mut dyn DirAccess) -> AdapterResult {
    let mut keys = BTreeSet::new();
    if let Some(text) = access.read_text(&[".crates2.json"], PACKAGE_FILE_MAX_BYTES) {
        let value = json::parse(&text, 32).map_err(|e| e.to_string())?;
        let installs = value["installs"].as_object().ok_or("Cargo-Installationsformat unbekannt")?;
        for key in installs.keys() {
            keys.insert(key.clone());
        }
    } else if let Some(text) = access.read_text(&[".crates.toml"], PACKAGE_FILE_MAX_BYTES) {
        let table = toml_table(&text).map_err(|e| e.to_string())?;
        let installs = table.get("v1").and_then(|v| v.as_table()).ok_or("Cargo-Installationsformat unbekannt")?;
        for key in installs.keys() {
            keys.insert(key.clone());
        }
    }
    let mut items = Vec::new();
    for key in keys {
        let Some((name, rest)) = key.split_once(' ') else { return Err("Cargo-Toolbeleg nicht verstanden".into()) };
        let Some((version, source)) = rest.split_once(" (") else {
            return Err("Cargo-Toolquelle nicht verstanden".into());
        };
        let (class, host) = super::cargo::crates_io(source.trim_end_matches(')'));
        let mut item = InventoryItem::new(ItemCategory::Package, "cargo", name, ".crates.toml/.crates2.json")
            .version(version)
            .state(InstallState::Installed)
            .source(class, host)
            .detail("Cargo-Installationsdatensatz; Binärdateien und CLI-Nutzung nicht geprüft");
        item.evidence.context = "global-tool".into();
        item.evidence.qualifier = format!("{version}|{}", item.source_host.as_deref().unwrap_or(""));
        items.push(item);
    }
    bounded(items)
}

fn nuget(access: &mut dyn DirAccess) -> AdapterResult {
    let mut items = Vec::new();
    for (name, is_dir) in access.list(&[]) {
        if !is_dir || name.starts_with('.') {
            continue;
        }
        for (version, is_dir) in access.list(&[&name]) {
            if !is_dir {
                continue;
            }
            let text = access.read_text(&[&name, &version, ".nupkg.metadata"], 256 * 1024);
            let value = text.as_deref().map(|text| json::parse(text, 16).map_err(|e| e.to_string())).transpose()?;
            let (source, host) = value
                .as_ref()
                .and_then(|v| v["source"].as_str())
                .map_or((SourceClass::Unknown, None), |u| classify_url(u, &["api.nuget.org"]));
            let mut item = InventoryItem::new(ItemCategory::Cache, "nuget", &name, format!("{name}/{version}"))
                .version(&version)
                .source(source, host)
                .detail("Im gemeinsamen Paketspeicher; keine Projektzuordnung aus dem Ordnernamen");
            item.evidence.context = "shared-cache".into();
            if let Some(text) = access.read_text(&[&name, &version, &format!("{name}.nuspec")], PACKAGE_FILE_MAX_BYTES)
            {
                for element in xml::elements(&text, 50_000, 32).map_err(|e| e.to_string())? {
                    if element.name == "dependency"
                        && let Some(id) = element.attr("id")
                    {
                        item.evidence.dependencies.push(crate::domain::PackageDependency {
                            name: id.into(),
                            requirement: element.attr("version").unwrap_or("").into(),
                            kind: "dependency".into(),
                            exact: false,
                        });
                    }
                }
            }
            items.push(item);
        }
    }
    bounded(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::FakeDir;
    #[test]
    fn npm_global_never_assumes_public_registry() {
        let mut dir = FakeDir::default();
        dir.dirs.insert("".into(), vec![("tool".into(), true)]);
        dir.files.insert(
            "tool/package.json".into(),
            r#"{"name":"tool","version":"1.0.0","scripts":{"install":"do not run"}}"#.into(),
        );
        let items = inventory_global(ScopeKind::NpmGlobal, &mut dir).unwrap();
        assert_eq!(items[0].source, SourceClass::Unknown);
        assert_eq!(items[0].version.as_deref(), Some("1.0.0"));
    }
    #[test]
    fn unknown_install_formats_are_not_empty_successes() {
        let mut dir = FakeDir::default();
        dir.files.insert(".crates2.json".into(), r#"{"future-installs":{}}"#.into());
        assert!(inventory_global(ScopeKind::CargoTools, &mut dir).is_err());
    }
    #[test]
    fn nuget_cache_is_distinct_from_installed_dependencies() {
        let mut dir = FakeDir::default();
        dir.dirs.insert("".into(), vec![("example".into(), true)]);
        dir.dirs.insert("example".into(), vec![("1.0.0".into(), true)]);
        dir.files.insert(
            "example/1.0.0/.nupkg.metadata".into(),
            r#"{"source":"https://api.nuget.org/v3/index.json"}"#.into(),
        );
        let items = inventory_global(ScopeKind::NugetCache, &mut dir).unwrap();
        assert_eq!(items[0].category, ItemCategory::Cache);
        assert_eq!(items[0].install_state, None);
        assert_eq!(items[0].source_host.as_deref(), Some("api.nuget.org"));
    }
}
