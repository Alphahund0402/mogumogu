//! npm: package.json (declared), package-lock.json (resolved) and
//! node_modules/.package-lock.json (installed). No scripts, no `npm ls`.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory};
use crate::limits::{PACKAGE_FILE_MAX_BYTES, REFERENCE_MAX_DEPTH};
use crate::parsers::json;
use serde_json::Value;

pub struct Npm;

pub(super) const PUBLIC: &[&str] = &["registry.npmjs.org"];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "npm",
    name: "npm",
    markers: &["package.json", "package-lock.json", "npm-shrinkwrap.json"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    updates: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["package.json", "package-lock.json v1–v3", "node_modules/.package-lock.json"],
    limits: "Keine Skripte oder Lifecycle-Hooks; globale npm-Installationen und Benutzer-.npmrc werden nicht gelesen.",
};

impl Adapter for Npm {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let pnpm = file.siblings.iter().any(|s| s == "pnpm-lock.yaml");
        match file.name {
            "package.json" => bounded(package_json(file, if pnpm { "pnpm" } else { "npm" })?),
            _ if pnpm => Ok(Vec::new()),
            _ => {
                let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
                let mut items = vec![manifest(
                    "npm",
                    file,
                    format!("Lockfile v{}", value["lockfileVersion"].as_i64().unwrap_or(1)),
                )];
                items.extend(lock_packages(&value, &file.rel_path(), InstallState::Resolved));
                bounded(items)
            }
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let pnpm = siblings.iter().any(|s| s == "pnpm-lock.yaml");
        (name == "node_modules" && !pnpm).then_some(DirClaim {
            adapter: "npm",
            category: ItemCategory::Environment,
            label: "node_modules",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, "npm", "node_modules", rel_path)
                .detail("Installierte Abhängigkeiten; nicht ausgeführt"),
        ];
        match access.read_text(&[".package-lock.json"], PACKAGE_FILE_MAX_BYTES) {
            Some(text) => {
                let value = json::parse(&text, 64).map_err(|e| e.to_string())?;
                items.extend(lock_packages(&value, &format!("{rel_path}/.package-lock.json"), InstallState::Installed));
            }
            None => {
                for (name, is_dir) in access.list(&[]) {
                    if !is_dir || name.starts_with('.') {
                        continue;
                    }
                    let names = if name.starts_with('@') {
                        access
                            .list(&[&name])
                            .into_iter()
                            .filter(|(_, d)| *d)
                            .map(|(n, _)| format!("{name}/{n}"))
                            .collect()
                    } else {
                        vec![name]
                    };
                    for package in names {
                        items.push(
                            InventoryItem::new(ItemCategory::Package, "npm", package, rel_path)
                                .state(InstallState::Installed)
                                .detail("Ordner vorhanden; Version ohne .package-lock.json unbekannt"),
                        );
                    }
                }
            }
        }
        bounded(items)
    }
}

fn spec_source(spec: &str) -> (crate::domain::SourceClass, Option<String>) {
    use crate::domain::SourceClass;
    let lower = spec.to_ascii_lowercase();
    if lower.starts_with("http:")
        || lower.starts_with("https:")
        || lower.starts_with("git")
        || lower.starts_with("file:")
        || lower.starts_with("link:")
        || lower.starts_with("workspace:")
    {
        return classify_url(spec, PUBLIC);
    }
    if spec.contains('/') && !spec.starts_with('@') && !spec.contains(':') {
        return (SourceClass::Git, None); // "user/repo" GitHub shorthand
    }
    (SourceClass::Unknown, None)
}

pub(super) fn package_json(file: &FoundFile<'_>, ecosystem: &str) -> AdapterResult {
    let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
    let workspaces = match &value["workspaces"] {
        Value::Array(list) => list.len(),
        Value::Object(map) => map.get("packages").and_then(Value::as_array).map_or(0, Vec::len),
        _ => 0,
    };
    let mut detail = String::from("Projektmanifest");
    if workspaces > 0 {
        detail.push_str(&format!(" · {workspaces} Workspace-Muster"));
    }
    if value.get("scripts").and_then(Value::as_object).is_some_and(|s| !s.is_empty()) {
        detail.push_str(" · Skripte vorhanden (nicht ausgeführt)");
    }
    let mut items = vec![manifest(ecosystem, file, detail)];
    for (section, label) in [
        ("dependencies", ""),
        ("devDependencies", " · dev"),
        ("optionalDependencies", " · optional"),
        ("peerDependencies", " · peer"),
    ] {
        let Some(deps) = value[section].as_object() else { continue };
        for (name, spec) in deps {
            let spec = spec.as_str().unwrap_or("?");
            let (source, host) = spec_source(spec);
            items.push(
                InventoryItem::new(ItemCategory::Package, ecosystem, name.as_str(), file.rel_path())
                    .state(InstallState::Declared)
                    .source(source, host)
                    .detail(format!("Spezifikation {spec}{label}")),
            );
        }
    }
    Ok(items)
}

/// Packages of a lockfile (`packages` map of v2/v3, nested `dependencies` of v1).
pub(super) fn lock_packages(value: &Value, rel_path: &str, state: InstallState) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    if let Some(packages) = value["packages"].as_object() {
        for (key, entry) in packages {
            let Some(index) = key.rfind("node_modules/") else { continue };
            let name = &key[index + "node_modules/".len()..];
            items.push(lock_item(name, entry, rel_path, state));
        }
    } else if let Some(dependencies) = value["dependencies"].as_object() {
        collect_v1(dependencies, rel_path, state, 0, &mut items);
    }
    items
}

fn collect_v1(
    deps: &serde_json::Map<String, Value>,
    rel_path: &str,
    state: InstallState,
    depth: usize,
    out: &mut Vec<InventoryItem>,
) {
    if depth > REFERENCE_MAX_DEPTH {
        return;
    }
    for (name, entry) in deps {
        out.push(lock_item(name, entry, rel_path, state));
        if let Some(nested) = entry["dependencies"].as_object() {
            collect_v1(nested, rel_path, state, depth + 1, out);
        }
    }
}

fn lock_item(name: &str, entry: &Value, rel_path: &str, state: InstallState) -> InventoryItem {
    let (source, host) = if entry["link"].as_bool() == Some(true) {
        (crate::domain::SourceClass::Path, None)
    } else {
        entry["resolved"].as_str().map_or((crate::domain::SourceClass::Unknown, None), |url| classify_url(url, PUBLIC))
    };
    let mut flags = Vec::new();
    if entry["dev"].as_bool() == Some(true) {
        flags.push("dev");
    }
    if entry["optional"].as_bool() == Some(true) {
        flags.push("optional");
    }
    let detail =
        if flags.is_empty() { state.label().to_string() } else { format!("{} · {}", state.label(), flags.join(", ")) };
    InventoryItem::new(ItemCategory::Package, "npm", name, rel_path)
        .version(entry["version"].as_str().unwrap_or_default())
        .state(state)
        .source(source, host)
        .detail(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};
    use crate::domain::SourceClass;

    #[test]
    fn manifest_lock_and_installed_are_separate_states() {
        let manifest = r#"{"name":"app","dependencies":{"left-pad":"^1.3.0"},"devDependencies":{"jest":"29"},"scripts":{"postinstall":"rm -rf /"}}"#;
        let items = Npm.parse_file(&file("package.json", manifest)).unwrap();
        assert!(items.iter().any(|i| i.name == "left-pad" && i.install_state == Some(InstallState::Declared)));
        assert!(items[0].detail.contains("nicht ausgeführt"));

        let lock = r#"{"lockfileVersion":3,"packages":{"":{"name":"app"},
            "node_modules/left-pad":{"version":"1.3.0","resolved":"https://registry.npmjs.org/left-pad/-/left-pad-1.3.0.tgz"},
            "node_modules/@corp/x":{"version":"2.0.0","resolved":"https://npm.corp.example/@corp/x/-/x-2.0.0.tgz","dev":true}}}"#;
        let items = Npm.parse_file(&file("package-lock.json", lock)).unwrap();
        let pad = items.iter().find(|i| i.name == "left-pad").unwrap();
        assert_eq!(pad.version.as_deref(), Some("1.3.0"));
        assert_eq!(pad.source, SourceClass::PublicRegistry);
        let corp = items.iter().find(|i| i.name == "@corp/x").unwrap();
        assert_eq!(corp.source, SourceClass::PrivateRegistry);

        let mut dir = FakeDir::default();
        dir.files.insert(".package-lock.json".into(), lock.into());
        let claim = Npm.claim_dir("node_modules", &[], &[]).unwrap();
        let items = Npm.inspect_dir(claim, "node_modules", &mut dir).unwrap();
        assert!(items.iter().any(|i| i.install_state == Some(InstallState::Installed) && i.name == "left-pad"));
    }

    #[test]
    fn broken_lockfile_is_an_error_not_an_empty_inventory() {
        assert!(Npm.parse_file(&file("package-lock.json", "{not json")).is_err());
    }
}
