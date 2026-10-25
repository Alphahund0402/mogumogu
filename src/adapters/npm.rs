//! npm: package.json (declared; overrides as edges), package-lock.json
//! (resolved) and node_modules/.package-lock.json (installed). No scripts,
//! no `npm ls`.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::limits::{PACKAGE_FILE_MAX_BYTES, REFERENCE_MAX_DEPTH};
use crate::parsers::json;
use serde_json::{Map, Value};

pub struct Npm;

pub(super) const PUBLIC: &[&str] = &["registry.npmjs.org"];
/// Override edges kept per manifest.
const MAX_OVERRIDES: usize = 5_000;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "npm",
    name: "npm",
    markers: &["package.json", "package-lock.json", "npm-shrinkwrap.json"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "package.json (overrides, resolutions, pnpm.overrides als Kanten)",
        "package-lock.json v1–v3",
        "node_modules/.package-lock.json",
    ],
    limits: "Keine Skripte oder Hooks; globaler Paketbestand nur mit eigener npm-global-Freigabe; Benutzer-.npmrc wird nicht gelesen; Overrides werden nicht auf den Baum angewandt.",
};

impl Adapter for Npm {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let pnpm = file.siblings.iter().any(|s| s == "pnpm-lock.yaml");
        match file.name {
            "package.json" => bounded(package_json(file, manager(file.siblings))?),
            _ if pnpm => Ok(Vec::new()),
            _ => {
                let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
                let mut items = vec![manifest(
                    "npm",
                    file,
                    format!("Lockfile v{}", value["lockfileVersion"].as_i64().unwrap_or(1)),
                )];
                items[0].evidence.dependencies = dependencies(&value["packages"][""]);
                items.extend(lock_packages(&value, &file.rel_path(), InstallState::Resolved));
                bounded(items)
            }
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], children: &[String]) -> Option<DirClaim> {
        let deno = super::deno::store_beside_config(siblings, children);
        (name == "node_modules" && manager(siblings) == "npm" && !deno).then_some(DirClaim {
            adapter: "npm",
            category: ItemCategory::Environment,
            label: "node_modules",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        node_modules("npm", rel_path, access)
    }
}

/// The manager whose lockfile lies next to a package.json or node_modules.
pub(super) fn manager(siblings: &[String]) -> &'static str {
    let has = |name: &str| siblings.iter().any(|s| s == name);
    if has("pnpm-lock.yaml") {
        "pnpm"
    } else if has("bun.lock") || has("bun.lockb") {
        "bun"
    } else if has("yarn.lock") {
        "yarn"
    } else {
        "npm"
    }
}

/// Installed packages of a node_modules folder: versions from npm's hidden
/// lockfile when present, otherwise folder names only.
pub(super) fn node_modules(ecosystem: &str, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
    let mut items = vec![
        InventoryItem::new(ItemCategory::Environment, ecosystem, "node_modules", rel_path)
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
                    access.list(&[&name]).into_iter().filter(|(_, d)| *d).map(|(n, _)| format!("{name}/{n}")).collect()
                } else {
                    vec![name]
                };
                for package in names {
                    items.push(
                        InventoryItem::new(ItemCategory::Package, ecosystem, package, rel_path)
                            .state(InstallState::Installed)
                            .detail("Ordner vorhanden; Version ohne .package-lock.json unbekannt"),
                    );
                }
            }
        }
    }
    bounded(items)
}

pub(super) fn spec_source(spec: &str) -> (SourceClass, Option<String>) {
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
    let bundle = value.get("bundleDependencies").or_else(|| value.get("bundledDependencies"));
    let bundle_names: std::collections::HashSet<&str> = match bundle {
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
        _ => Default::default(),
    };
    let mut bundled = 0;
    let mut items = Vec::new();
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
            // `true` bundles every runtime dependency.
            let in_bundle = match bundle {
                Some(Value::Bool(true)) => matches!(section, "dependencies" | "optionalDependencies"),
                Some(Value::Array(_)) => bundle_names.contains(name.as_str()),
                _ => false,
            };
            bundled += usize::from(in_bundle);
            let flag = if in_bundle { " · gebündelt" } else { "" };
            let mut item = InventoryItem::new(ItemCategory::Package, ecosystem, name.as_str(), file.rel_path())
                .state(InstallState::Declared)
                .source(source, host)
                .detail(format!("Spezifikation {}{label}{flag}", spec_label(spec)));
            item.evidence.context = section.into();
            items.push(item);
        }
    }
    let workspaces = match &value["workspaces"] {
        Value::Array(list) => list.len(),
        Value::Object(map) => map.get("packages").and_then(Value::as_array).map_or(0, Vec::len),
        _ => 0,
    };
    let overrides = overrides(&value);
    let mut detail = String::from("Projektmanifest");
    if workspaces > 0 {
        detail.push_str(&format!(" · {workspaces} Workspace-Muster"));
    }
    if value.get("scripts").and_then(Value::as_object).is_some_and(|s| !s.is_empty()) {
        detail.push_str(" · Skripte vorhanden (nicht ausgeführt)");
    }
    if !overrides.is_empty() {
        detail.push_str(&format!(" · {} Overrides", overrides.len()));
    }
    if bundled > 0 {
        detail.push_str(&format!(" · {bundled} gebündelt"));
    }
    if file.siblings.iter().any(|s| s == "bun.lockb") && !file.siblings.iter().any(|s| s == "bun.lock") {
        detail.push_str(" · bun.lockb ist binär und wird nicht gelesen");
    }
    let mut head = manifest(ecosystem, file, detail);
    head.evidence.dependencies = dependencies(&value);
    head.evidence.dependencies.extend(overrides);
    head.evidence.commands = value["scripts"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, v)| v.as_str())
        .flat_map(super::commands::names)
        .collect();
    items.insert(0, head);
    Ok(items)
}

/// npm `overrides`, Yarn `resolutions` and `pnpm.overrides` as edges of the
/// kind `override`: they replace versions in the tree and declare nothing.
pub(super) fn overrides(value: &Value) -> Vec<PackageDependency> {
    let mut edges = Vec::new();
    if let Some(map) = value["overrides"].as_object() {
        npm_overrides(map, 0, &mut edges);
    }
    let resolutions =
        value["resolutions"].as_object().into_iter().flatten().map(|(key, spec)| (resolution_target(key), spec));
    let pnpm = value["pnpm"]["overrides"].as_object().into_iter().flatten().map(|(key, spec)| (pnpm_target(key), spec));
    for (name, spec) in resolutions.chain(pnpm).take(MAX_OVERRIDES) {
        if let Some(spec) = spec.as_str()
            && edges.len() < MAX_OVERRIDES
        {
            edges.push(override_edge(name, spec));
        }
    }
    edges
}

/// `{"foo": "1.0.0", "bar@2": {"baz": "1.0.0"}, "boo": {".": "1.0.0"}}`.
fn npm_overrides(map: &Map<String, Value>, depth: usize, out: &mut Vec<PackageDependency>) {
    if depth > REFERENCE_MAX_DEPTH {
        return;
    }
    for (selector, value) in map {
        if out.len() >= MAX_OVERRIDES {
            break;
        }
        if selector == "." {
            continue;
        }
        let name = selector_name(selector);
        match value {
            Value::String(spec) => out.push(override_edge(name, spec)),
            Value::Object(nested) => {
                if let Some(spec) = nested.get(".").and_then(Value::as_str) {
                    out.push(override_edge(name, spec));
                }
                npm_overrides(nested, depth + 1, out);
            }
            _ => {}
        }
    }
}

pub(super) fn override_edge(name: &str, spec: &str) -> PackageDependency {
    PackageDependency { name: name.to_string(), requirement: spec_label(spec), kind: "override".into(), exact: false }
}

/// `@npm/bar@2.0.0` or `form-data@` → the package name.
fn selector_name(selector: &str) -> &str {
    let start = usize::from(selector.starts_with('@'));
    selector[start..].find('@').map_or(selector, |index| &selector[..start + index])
}

/// The last package of a Yarn resolution: `**/lodash`, `webpack/**/postcss`,
/// `a/@scope/b` or `lodash@npm:4.17.20`.
fn resolution_target(pattern: &str) -> &str {
    let Some(last) = pattern.rfind('/') else { return selector_name(pattern) };
    let start = pattern[..last].rfind('/').map_or(0, |index| index + 1);
    selector_name(if pattern[start..last].starts_with('@') { &pattern[start..] } else { &pattern[last + 1..] })
}

/// The package a pnpm selector overrides: `qar@1>zoo` → `zoo`.
pub(super) fn pnpm_target(selector: &str) -> &str {
    selector_name(selector.rsplit('>').next().unwrap_or(selector))
}

/// A spec as an edge stores it: local paths only as such, URLs by host.
pub(super) fn spec_label(spec: &str) -> String {
    let protocol = super::bun::protocol(spec);
    if spec == "-" {
        "entfernt".into()
    } else if matches!(protocol, Some("file" | "link" | "portal")) {
        "lokaler Pfad".into()
    } else if spec.contains("://") || matches!(protocol, Some("git" | "github" | "gitlab" | "bitbucket")) {
        match classify_url(spec, PUBLIC) {
            (SourceClass::Git, _) | (_, None) => "Git".into(),
            (_, Some(host)) => format!("URL {host}"),
        }
    } else if matches!(protocol, Some("npm" | "catalog" | "workspace" | "jsr")) {
        spec.into()
    } else if let Some(protocol) = protocol {
        format!("{protocol}:…")
    } else if spec.contains('/') && !spec.starts_with(['@', '$']) {
        "Git".into() // `user/repo` on GitHub
    } else {
        spec.into()
    }
}

pub(super) fn dependencies(value: &Value) -> Vec<PackageDependency> {
    let mut refs = Vec::new();
    for section in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies", "requires"] {
        for (name, spec) in value[section].as_object().into_iter().flatten() {
            refs.push(PackageDependency {
                name: name.clone(),
                // Paths and URLs stay out: `file:C:/Users/…`, tarball URLs.
                requirement: spec_label(spec.as_str().unwrap_or("")),
                kind: section.into(),
                exact: false,
            });
        }
    }
    refs
}

/// Packages of a lockfile (`packages` map of v2/v3, nested `dependencies` of v1).
pub(super) fn lock_packages(value: &Value, rel_path: &str, state: InstallState) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    if let Some(packages) = value["packages"].as_object() {
        for (key, entry) in packages {
            let Some(index) = key.rfind("node_modules/") else { continue };
            let name = &key[index + "node_modules/".len()..];
            let mut item = lock_item(name, entry, rel_path, state);
            item.evidence.qualifier = key.clone();
            items.push(item);
        }
    } else if let Some(dependencies) = value["dependencies"].as_object() {
        collect_v1(dependencies, rel_path, state, 0, "", &mut items);
    }
    items
}

fn collect_v1(
    deps: &serde_json::Map<String, Value>,
    rel_path: &str,
    state: InstallState,
    depth: usize,
    prefix: &str,
    out: &mut Vec<InventoryItem>,
) {
    if depth > REFERENCE_MAX_DEPTH {
        return;
    }
    for (name, entry) in deps {
        let qualifier = format!("{prefix}/node_modules/{name}");
        let mut item = lock_item(name, entry, rel_path, state);
        item.evidence.qualifier = qualifier.clone();
        out.push(item);
        if let Some(nested) = entry["dependencies"].as_object() {
            collect_v1(nested, rel_path, state, depth + 1, &qualifier, out);
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
    let mut item = InventoryItem::new(ItemCategory::Package, "npm", name, rel_path)
        // v1 lockfiles keep git URLs and paths in `version`.
        .version(entry["version"].as_str().filter(|v| !v.contains([':', '/'])).unwrap_or_default())
        .state(state)
        .source(source, host)
        .detail(detail);
    item.evidence.dependencies = dependencies(entry);
    item.evidence.deprecated = entry["deprecated"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    item
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

    #[test]
    fn overrides_are_edges_and_bundles_are_flagged() {
        let manifest = r#"{
          "name": "app",
          "dependencies": { "@npm/foo": "^1.0.0", "bundled-a": "1.0.0", "lodash": "^4.17.0" },
          "optionalDependencies": { "fsevents": "^2.3.0" },
          "bundleDependencies": ["bundled-a"],
          "overrides": {
            "@npm/foo": "$foo",
            "@npm/bar@2.0.0": { "@npm/baz": "1.0.0" },
            "boo": { ".": "1.0.0", "bah": "2.0.0" },
            "local": "file:C:/Users/dev/s3cr3t-pkg"
          },
          "resolutions": { "**/lodash": "4.17.21", "webpack/**/postcss": "8.4.31", "a/@scope/b": "2.0.0", "left-pad@npm:1.1.0": "1.3.0" },
          "pnpm": { "overrides": {
            "qar@1>zoo": "2", "bar@^2.1.0": "3.0.0", "foo@1.0.0>gone": "-", "form-data@": "4.0.6",
            "tool": "git+ssh://git@github.com/x/tool.git", "tar": "https://u:s3cr3t@npm.corp.example/tar.tgz"
          } }
        }"#;
        let items = Npm.parse_file(&file("package.json", manifest)).unwrap();
        let mut edges: Vec<(&str, &str)> = items[0]
            .evidence
            .dependencies
            .iter()
            .filter(|d| d.kind == "override")
            .map(|d| (d.name.as_str(), d.requirement.as_str()))
            .collect();
        edges.sort_unstable();
        assert_eq!(
            edges,
            [
                ("@npm/baz", "1.0.0"),
                ("@npm/foo", "$foo"),
                ("@scope/b", "2.0.0"),
                ("bah", "2.0.0"),
                ("bar", "3.0.0"),
                ("boo", "1.0.0"),
                ("form-data", "4.0.6"),
                ("gone", "entfernt"),
                ("left-pad", "1.3.0"),
                ("local", "lokaler Pfad"),
                ("lodash", "4.17.21"),
                ("postcss", "8.4.31"),
                ("tar", "URL npm.corp.example"),
                ("tool", "Git"),
                ("zoo", "2"),
            ]
        );
        assert!(items[0].detail.contains("15 Overrides · 1 gebündelt"), "{}", items[0].detail);
        let declared: Vec<&str> = items.iter().skip(1).map(|i| i.name.as_str()).collect();
        assert_eq!(declared, ["@npm/foo", "bundled-a", "lodash", "fsevents"], "overrides declare nothing");
        assert!(items[2].detail.ends_with("Spezifikation 1.0.0 · gebündelt"), "{}", items[2].detail);
        assert!(!format!("{items:?}").contains("s3cr3t"));

        let all = r#"{"dependencies": {"a": "1"}, "optionalDependencies": {"b": "2"}, "devDependencies": {"c": "3"}, "bundledDependencies": true}"#;
        let items = Npm.parse_file(&file("package.json", all)).unwrap();
        assert!(items[0].detail.ends_with("2 gebündelt"), "{}", items[0].detail);
        let flagged: Vec<&str> =
            items.iter().skip(1).filter(|i| i.detail.ends_with("gebündelt")).map(|i| i.name.as_str()).collect();
        assert_eq!(flagged, ["a", "b"], "true bundles the runtime dependencies");
    }

    #[test]
    fn node_modules_with_a_deno_store_is_left_to_deno() {
        let deno: Vec<String> = vec!["package.json".into(), "deno.lock".into()];
        assert!(Npm.claim_dir("node_modules", &deno, &[".deno".into()]).is_none());
        assert!(crate::adapters::deno::Deno.claim_dir("node_modules", &deno, &[".deno".into()]).is_some());
        assert!(Npm.claim_dir("node_modules", &deno, &[".package-lock.json".into()]).is_some());
        assert!(Npm.claim_dir("node_modules", &["package.json".into()], &[".deno".into()]).is_some(), "no Deno file");
    }
}
