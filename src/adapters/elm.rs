//! Elm: elm.json of applications (exact versions of every direct, indirect
//! and test dependency) and of packages (version ranges). elm-stuff/ beside
//! it is compiler output and is not walked. The Elm compiler is never
//! started and the package cache ~/.elm is not read.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::json;
use serde_json::Value;

pub struct Elm;

/// Elm 0.19 installs from this one registry.
const REGISTRY: &str = "package.elm-lang.org";

static DESCRIPTOR: Descriptor = Descriptor {
    id: "elm",
    name: "Elm",
    markers: &["elm.json"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["elm.json Anwendung (exakte Versionen)", "elm.json Paket (Versionsbereiche)", "elm-stuff/ als Ausgabe"],
    limits: "Kein Elm-Aufruf; Paketcache ~/.elm und elm-stuff werden nicht gelesen, Versionsbereiche von Paketen nicht aufgelöst.",
};

impl Adapter for Elm {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let value = json::parse(file.content, 32).map_err(|e| e.to_string())?;
        match value["type"].as_str() {
            Some("application") => bounded(application(file, &value)),
            Some("package") => bounded(package(file, &value)),
            _ => Ok(Vec::new()), // not an elm.json of Elm 0.19
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        (name == "elm-stuff" && siblings.iter().any(|s| s == "elm.json")).then_some(DirClaim {
            adapter: "elm",
            category: ItemCategory::Output,
            label: "elm-stuff",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, _access: &mut dyn DirAccess) -> AdapterResult {
        Ok(vec![
            InventoryItem::new(ItemCategory::Output, "elm", "elm-stuff", rel_path)
                .detail("Generierte Compilerausgabe; Größe erst nach Messung bekannt"),
        ])
    }
}

/// An application pins the complete dependency set, like a lockfile.
fn application(file: &FoundFile<'_>, value: &Value) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    let mut edges = Vec::new();
    for (section, test) in [("dependencies", false), ("test-dependencies", true)] {
        for (group, direct) in [("direct", true), ("indirect", false)] {
            for (name, version) in value[section][group].as_object().into_iter().flatten().take(10_000) {
                let version = version.as_str().unwrap_or_default();
                let context = if test { "test" } else { group };
                let kind = match (test, direct) {
                    (false, true) => "direkt",
                    (false, false) => "indirekt",
                    (true, true) => "Test, direkt",
                    (true, false) => "Test, indirekt",
                };
                let mut item = InventoryItem::new(ItemCategory::Package, "elm", name.as_str(), file.rel_path())
                    .version(version)
                    .state(InstallState::Resolved)
                    .source(SourceClass::PublicRegistry, Some(REGISTRY.into()))
                    .detail(format!("festgelegt laut elm.json · {kind}"));
                item.evidence.context = context.into();
                items.push(item);
                if direct {
                    edges.push(PackageDependency {
                        name: name.clone(),
                        requirement: version.into(),
                        kind: context.into(),
                        exact: true,
                    });
                }
            }
        }
    }
    let elm = value["elm-version"].as_str().unwrap_or("?");
    let mut head = manifest("elm", file, format!("Elm-Anwendung · Elm {elm} · {} Pakete", items.len()));
    head.evidence.dependencies = edges;
    items.insert(0, head);
    items
}

/// A package declares ranges such as `1.0.0 <= v < 2.0.0`; its own name is
/// no dependency.
fn package(file: &FoundFile<'_>, value: &Value) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    let mut edges = Vec::new();
    for (section, context) in [("dependencies", ""), ("test-dependencies", "test")] {
        for (name, range) in value[section].as_object().into_iter().flatten().take(10_000) {
            let range = range.as_str().unwrap_or("?");
            let test = if context.is_empty() { "" } else { " · Test" };
            let mut item = InventoryItem::new(ItemCategory::Package, "elm", name.as_str(), file.rel_path())
                .state(InstallState::Declared)
                .source(SourceClass::PublicRegistry, Some(REGISTRY.into()))
                .detail(format!("Spezifikation {range}{test}"));
            item.evidence.context = context.into();
            items.push(item);
            edges.push(PackageDependency {
                name: name.clone(),
                requirement: range.into(),
                kind: section.into(),
                exact: false,
            });
        }
    }
    let name = value["name"].as_str().unwrap_or("ohne Namen");
    let version = value["version"].as_str().unwrap_or("?");
    let elm = value["elm-version"].as_str().unwrap_or("?");
    let detail = format!("Elm-Paket {name} {version} · Elm {elm} · {} Abhängigkeiten", items.len());
    let mut head = manifest("elm", file, detail);
    head.evidence.dependencies = edges;
    items.insert(0, head);
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    #[test]
    fn application_pins_direct_indirect_and_test_dependencies() {
        let text = r#"{
            "type": "application",
            "source-directories": ["src"],
            "elm-version": "0.19.1",
            "dependencies": {
                "direct": { "elm/browser": "1.0.2", "elm/core": "1.0.5", "elm/html": "1.0.0" },
                "indirect": { "elm/json": "1.1.3", "elm/virtual-dom": "1.0.3" }
            },
            "test-dependencies": {
                "direct": { "elm-explorations/test": "2.1.1" },
                "indirect": { "elm/random": "1.0.0" }
            }
        }"#;
        let items = Elm.parse_file(&file("elm.json", text)).unwrap();
        assert_eq!(items[0].detail, "Elm-Anwendung · Elm 0.19.1 · 7 Pakete");
        assert_eq!(items[0].evidence.dependencies.len(), 4, "direct dependencies are edges");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap();
        let core = find("elm/core");
        assert_eq!((core.version.as_deref(), core.install_state), (Some("1.0.5"), Some(InstallState::Resolved)));
        assert_eq!((core.evidence.context.as_str(), core.source_host.as_deref()), ("direct", Some(REGISTRY)));
        assert_eq!(core.source, SourceClass::PublicRegistry);
        assert_eq!(find("elm/json").evidence.context, "indirect");
        let test = find("elm-explorations/test");
        assert_eq!(
            (test.evidence.context.as_str(), test.detail.as_str()),
            ("test", "festgelegt laut elm.json · Test, direkt")
        );
        assert_eq!(find("elm/random").detail, "festgelegt laut elm.json · Test, indirekt");
        assert!(Elm.claim_dir("elm-stuff", &["elm.json".into()], &[]).is_some());
        assert!(Elm.claim_dir("elm-stuff", &["package.json".into()], &[]).is_none());
    }

    #[test]
    fn package_declares_ranges_and_foreign_files_are_ignored() {
        let text = r#"{
            "type": "package",
            "name": "elm/json",
            "summary": "Encode and decode JSON values",
            "license": "BSD-3-Clause",
            "version": "1.1.3",
            "exposed-modules": ["Json.Decode", "Json.Encode"],
            "elm-version": "0.19.0 <= v < 0.20.0",
            "dependencies": { "elm/core": "1.0.0 <= v < 2.0.0" },
            "test-dependencies": { "elm-explorations/test": "2.0.0 <= v < 3.0.0" }
        }"#;
        let items = Elm.parse_file(&file("elm.json", text)).unwrap();
        assert_eq!(items[0].detail, "Elm-Paket elm/json 1.1.3 · Elm 0.19.0 <= v < 0.20.0 · 2 Abhängigkeiten");
        assert_eq!(items.len(), 3, "the package itself is no dependency");
        let core = &items[1];
        assert_eq!((core.name.as_str(), core.install_state), ("elm/core", Some(InstallState::Declared)));
        assert!(core.version.is_none() && core.detail == "Spezifikation 1.0.0 <= v < 2.0.0");
        assert_eq!(items[2].evidence.context, "test");

        assert!(Elm.parse_file(&file("elm.json", r#"{"name": "other tool", "elm": true}"#)).unwrap().is_empty());
        assert!(Elm.parse_file(&file("elm.json", "{\"type\": ")).is_err());
    }
}
