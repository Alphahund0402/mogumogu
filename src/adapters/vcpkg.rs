//! vcpkg: vcpkg.json (declared, with features, host tools and overrides),
//! vcpkg-configuration.json (registries, summarised) and the installed
//! database of `vcpkg_installed` beside a vcpkg.json (`vcpkg/status` with
//! pending updates, else the `info/*.list` names). Ports, triplets and
//! overlays are never evaluated or built. A vcpkg instance (`.vcpkg-root`)
//! inside a project is claimed so its ports tree is not walked; its classic
//! installation database is read the same way.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::limits::PACKAGE_FILE_MAX_BYTES;
use crate::parsers::json;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub struct Vcpkg;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "vcpkg",
    name: "vcpkg",
    markers: &["vcpkg.json", "vcpkg-configuration.json"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "vcpkg.json (dependencies, features, overrides, eingebettete Konfiguration)",
        "vcpkg-configuration.json (Registries, zusammengefasst)",
        "vcpkg_installed/vcpkg/status (+ updates, info/*.list)",
        "vcpkg-Instanz im Projekt (.vcpkg-root): installed/vcpkg/status",
    ],
    limits: "Kein vcpkg-Aufruf; Ports, Triplets und Overlays werden nicht ausgewertet – mit Overlay-Ports oder getrennter vcpkg-configuration.json bleibt die Quelle der Abhängigkeiten unbekannt; aufgelöste Versionen gibt es erst im Installationsbestand.",
};

type Source = (SourceClass, Option<String>);

fn builtin() -> Source {
    (SourceClass::PublicRegistry, Some("github.com".into()))
}

impl Adapter for Vcpkg {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let value = json::parse(file.content, 32).map_err(|e| e.to_string())?;
        if !value.is_object() {
            return Err(format!("{}: kein JSON-Objekt", file.name));
        }
        if file.name.eq_ignore_ascii_case("vcpkg-configuration.json") {
            let detail = format!("Registry-Konfiguration · {}", Registries::new(Some(&value)).summary);
            return Ok(vec![manifest("vcpkg", file, detail)]);
        }
        bounded(manifest_items(file, &value))
    }

    fn claim_dir(&self, name: &str, siblings: &[String], children: &[String]) -> Option<DirClaim> {
        // CMake builds put it into the build folder (`build/vcpkg_installed`),
        // there the `vcpkg` database folder inside identifies it.
        let manifest = siblings.iter().any(|s| s == "vcpkg.json") || children.iter().any(|c| c == "vcpkg");
        if name == "vcpkg_installed" && manifest {
            return Some(DirClaim { adapter: "vcpkg", category: ItemCategory::Environment, label: "vcpkg_installed" });
        }
        children.iter().any(|c| c == ".vcpkg-root").then_some(DirClaim {
            adapter: "vcpkg",
            category: ItemCategory::Environment,
            label: "vcpkg-Instanz",
        })
    }

    fn inspect_dir(&self, claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let (base, rel, name): (&[&str], String, &str) = if claim.label == "vcpkg_installed" {
            (&[], rel_path.to_string(), "vcpkg_installed")
        } else {
            (&["installed"], format!("{rel_path}/installed"), "vcpkg-Instanz")
        };
        let (packages, origin) = installed(access, base, &rel);
        let mut detail = format!("{} installierte Ports{origin}; nicht ausgeführt", packages.len());
        if claim.label != "vcpkg_installed" {
            detail = format!("vcpkg-Installation, Ports-Baum nicht durchsucht · {detail}");
        }
        let mut items = vec![InventoryItem::new(ItemCategory::Environment, "vcpkg", name, rel_path).detail(detail)];
        items.extend(packages);
        bounded(items)
    }
}

/// Registry routing of one configuration: the most specific `packages`
/// pattern wins, other ports use the default registry. Overlay ports take
/// precedence over every registry, so they make a public source unknown.
struct Registries {
    default: Source,
    patterns: Vec<(String, Source)>,
    overlays: bool,
    summary: String,
}

impl Registries {
    fn new(config: Option<&Value>) -> Self {
        let Some(config) = config else {
            return Self { default: builtin(), patterns: Vec::new(), overlays: false, summary: String::new() };
        };
        let default = match config.get("default-registry") {
            None => builtin(),
            Some(Value::Null) => (SourceClass::Unknown, None),
            Some(registry) => registry_source(registry),
        };
        let mut patterns = Vec::new();
        let mut kinds = Vec::new();
        for registry in config["registries"].as_array().into_iter().flatten().take(1_000) {
            let kind = registry["kind"].as_str().unwrap_or("?");
            kinds.push(kind);
            if kind == "artifact" {
                continue;
            }
            let source = registry_source(registry);
            let room = 1_000_usize.saturating_sub(patterns.len());
            for pattern in registry["packages"].as_array().into_iter().flatten().filter_map(Value::as_str).take(room) {
                patterns.push((pattern.to_string(), source.clone()));
            }
        }
        let count = |key: &str| config[key].as_array().map_or(0, Vec::len);
        let overlays = count("overlay-ports") > 0;
        let mut summary = match config.get("default-registry") {
            Some(Value::Null) => "keine Standard-Registry".to_string(),
            Some(registry) => {
                format!("Standard-Registry {} ({})", registry["kind"].as_str().unwrap_or("?"), label(&default))
            }
            None => "Standard-Registry builtin (öffentlich)".to_string(),
        };
        if !kinds.is_empty() {
            summary.push_str(&format!(" · {} weitere Registries ({})", kinds.len(), kinds.join(", ")));
        }
        for (key, name) in [("overlay-ports", "Overlay-Ports"), ("overlay-triplets", "Overlay-Triplets")] {
            if count(key) > 0 {
                summary.push_str(&format!(" · {} {name}", count(key)));
            }
        }
        Self { default, patterns, overlays, summary }
    }

    /// Everything unknown: a separate vcpkg-configuration.json routes ports
    /// that this file cannot see.
    fn unknown() -> Self {
        Self { default: (SourceClass::Unknown, None), patterns: Vec::new(), overlays: false, summary: String::new() }
    }

    fn source(&self, port: &str) -> Source {
        let exact = self.patterns.iter().find(|(pattern, _)| pattern == port);
        let prefix = self
            .patterns
            .iter()
            .filter(|(pattern, _)| pattern.strip_suffix('*').is_some_and(|prefix| port.starts_with(prefix)))
            .max_by_key(|(pattern, _)| pattern.len());
        let source = exact.or(prefix).map_or_else(|| self.default.clone(), |(_, source)| source.clone());
        if self.overlays && source.0 == SourceClass::PublicRegistry { (SourceClass::Unknown, None) } else { source }
    }
}

fn label(source: &Source) -> String {
    match (source.0, &source.1) {
        (SourceClass::PublicRegistry, _) => "öffentlich".into(),
        (SourceClass::PrivateRegistry, Some(host)) => format!("privat, {host}"),
        (SourceClass::PrivateRegistry, None) => "privat".into(),
        (SourceClass::Path, _) => "lokal".into(),
        _ => "unbekannt".into(),
    }
}

fn registry_source(registry: &Value) -> Source {
    match registry["kind"].as_str() {
        Some("builtin") => builtin(),
        Some("git") => repository_source(registry["repository"].as_str().unwrap_or_default()),
        Some("filesystem") => (SourceClass::Path, None),
        _ => (SourceClass::Unknown, None),
    }
}

/// A git registry: github.com/microsoft/vcpkg is the public curated
/// registry, any other repository keeps only its host.
fn repository_source(url: &str) -> Source {
    let url = url.trim();
    if url.is_empty() {
        return (SourceClass::Unknown, None);
    }
    if super::swiftpm::is_local(url) {
        return (SourceClass::Path, None);
    }
    let lower = url.to_ascii_lowercase();
    let (host, path) = match lower.split_once("://") {
        Some((_, rest)) => {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            (authority.rsplit('@').next().unwrap_or(authority).split(':').next().unwrap_or("").to_string(), path)
        }
        // `git@github.com:microsoft/vcpkg`
        None => match lower.split_once(':') {
            Some((authority, path)) => (authority.rsplit('@').next().unwrap_or(authority).to_string(), path),
            None => return (SourceClass::Unknown, None),
        },
    };
    if host == "github.com" && path.trim_end_matches('/').trim_end_matches(".git") == "microsoft/vcpkg" {
        return builtin();
    }
    let valid = !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    (SourceClass::PrivateRegistry, valid.then_some(host))
}

/// `version`, `version-semver`, `version-date` or `version-string` with
/// a `#port-version` suffix when it is not part of the text already.
fn version_of(value: &Value) -> Option<String> {
    let text =
        ["version", "version-semver", "version-date", "version-string"].iter().find_map(|key| value[*key].as_str())?;
    match value["port-version"].as_u64() {
        Some(port) if port > 0 && !text.contains('#') => Some(format!("{text}#{port}")),
        _ => Some(text.to_string()),
    }
}

fn manifest_items(file: &FoundFile<'_>, value: &Value) -> Vec<InventoryItem> {
    // `configuration` (older spelling `vcpkg-configuration`) excludes a
    // separate vcpkg-configuration.json.
    let embedded = value.get("configuration").or_else(|| value.get("vcpkg-configuration"));
    let separate = file.siblings.iter().any(|s| s == "vcpkg-configuration.json");
    let registries = match (embedded, separate) {
        (Some(config), _) => Registries::new(Some(config)),
        (None, true) => Registries::unknown(),
        (None, false) => Registries::new(None),
    };
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    collect(file, &value["dependencies"], "", &registries, &mut seen, &mut items);
    for (feature, spec) in value["features"].as_object().into_iter().flatten().take(1_000) {
        let group = format!("feature:{feature}");
        collect(file, &spec["dependencies"], &group, &registries, &mut seen, &mut items);
    }
    let dependencies = items.len();
    for entry in value["overrides"].as_array().into_iter().flatten().take(10_000) {
        let (Some(name), Some(version)) = (entry["name"].as_str(), version_of(entry)) else { continue };
        let (source, host) = registries.source(name);
        let mut item = InventoryItem::new(ItemCategory::Package, "vcpkg", name, file.rel_path())
            .version(&version)
            .state(InstallState::Declared)
            .source(source, host)
            .detail(format!("Override auf Version {version}"));
        item.evidence.context = "override".into();
        items.push(item);
    }
    let mut detail = match (value["name"].as_str(), version_of(value)) {
        (Some(name), Some(version)) => format!("Manifest {name} {version}"),
        (Some(name), None) => format!("Manifest {name}"),
        _ => "Manifest ohne Namen".to_string(),
    };
    if let Some(baseline) = value["builtin-baseline"].as_str() {
        detail.push_str(&format!(" · Baseline {}", super::swiftpm::short(baseline)));
    }
    match (embedded, separate) {
        (Some(_), _) => detail.push_str(&format!(" · Registries eingebettet: {}", registries.summary)),
        (None, true) => detail.push_str(" · Registries in vcpkg-configuration.json"),
        (None, false) => {}
    }
    detail.push_str(&format!(" · {dependencies} Abhängigkeiten"));
    if items.len() > dependencies {
        detail.push_str(&format!(" · {} Overrides", items.len() - dependencies));
    }
    items.insert(0, manifest("vcpkg", file, detail));
    items
}

/// Dependencies are port names or objects with `name`, `version>=`,
/// `features`, `default-features`, `host` and `platform`. A port listed
/// again (for another platform) keeps its own entry by a qualifier.
fn collect(
    file: &FoundFile<'_>,
    list: &Value,
    group: &str,
    registries: &Registries,
    seen: &mut BTreeSet<(String, String)>,
    items: &mut Vec<InventoryItem>,
) {
    for (index, dependency) in list.as_array().into_iter().flatten().take(10_000).enumerate() {
        // Past the cap `bounded` rejects the file anyway; stop building.
        if items.len() > crate::limits::ITEMS_PER_FILE {
            return;
        }
        let (name, spec) = match dependency {
            Value::String(name) => (name.as_str(), &Value::Null),
            Value::Object(_) => match dependency["name"].as_str() {
                Some(name) => (name, dependency),
                None => continue,
            },
            _ => continue,
        };
        let host = spec["host"].as_bool() == Some(true);
        let mut detail = match spec["version>="].as_str() {
            Some(minimum) => format!("Spezifikation >= {minimum}"),
            None => "ohne Versionsangabe (Baseline)".to_string(),
        };
        let features: Vec<&str> = spec["features"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f.as_str().or_else(|| f["name"].as_str()))
            .take(50)
            .collect();
        if !features.is_empty() {
            detail.push_str(&format!(" · Features {}", features.join(", ")));
        }
        if spec["default-features"].as_bool() == Some(false) {
            detail.push_str(" · ohne Standardfeatures");
        }
        if let Some(platform) = spec["platform"].as_str() {
            detail.push_str(&format!(" · Plattform {platform}"));
        }
        if host {
            detail.push_str(" · Host-Werkzeug");
        }
        let context = match (group.is_empty(), host) {
            (true, false) => String::new(),
            (true, true) => "host".to_string(),
            (false, false) => group.to_string(),
            (false, true) => format!("{group}:host"),
        };
        let (source, source_host) = registries.source(name);
        let mut item = InventoryItem::new(ItemCategory::Package, "vcpkg", name, file.rel_path())
            .state(InstallState::Declared)
            .source(source, source_host)
            .detail(detail);
        if !seen.insert((name.to_string(), context.clone())) {
            item.evidence.qualifier = spec["platform"].as_str().map_or_else(|| format!("#{index}"), str::to_string);
        }
        item.evidence.context = context;
        items.push(item);
    }
}

/// One paragraph of the installed database (`Package: zlib` …).
#[derive(Default)]
struct Paragraph {
    fields: Vec<(String, String)>,
}

impl Paragraph {
    fn get(&self, key: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

/// Paragraphs of a status file; continuation lines are skipped.
fn paragraphs(text: &str) -> Vec<Paragraph> {
    let mut out = vec![Paragraph::default()];
    for line in text.lines().take(2_000_000) {
        if line.trim().is_empty() {
            if out.last().is_some_and(|p| !p.fields.is_empty()) {
                out.push(Paragraph::default());
            }
            continue;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        if let Some((key, value)) = line.split_once(':')
            && let Some(last) = out.last_mut()
            && last.fields.len() < 64
        {
            last.fields.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    out.retain(|p| !p.fields.is_empty());
    out
}

/// Installed ports from `vcpkg/status` and the not yet merged files in
/// `vcpkg/updates` (later paragraphs win, as in vcpkg), else from the
/// `vcpkg/info/<port>_<version>_<triplet>.list` names.
fn installed(access: &mut dyn DirAccess, base: &[&str], rel: &str) -> (Vec<InventoryItem>, &'static str) {
    let path = |rest: &[&str]| -> Vec<String> { base.iter().chain(rest).map(|s| s.to_string()).collect() };
    let read = |access: &mut dyn DirAccess, parts: Vec<String>, max: u64| {
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        access.read_text(&parts, max)
    };
    let list = |access: &mut dyn DirAccess, parts: Vec<String>| {
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        access.list(&parts)
    };
    let mut items = Vec::new();
    let Some(status) = read(access, path(&["vcpkg", "status"]), PACKAGE_FILE_MAX_BYTES) else {
        for (entry, is_dir) in list(access, path(&["vcpkg", "info"])).into_iter().take(20_000) {
            let Some(stem) = entry.strip_suffix(".list").filter(|_| !is_dir) else { continue };
            let Some((rest, triplet)) = stem.rsplit_once('_') else { continue };
            let Some((name, version)) = rest.split_once('_') else { continue };
            let mut item = InventoryItem::new(ItemCategory::Package, "vcpkg", name, format!("{rel}/vcpkg/info"))
                .version(version)
                .state(InstallState::Installed)
                .detail(format!("installiert laut info/*.list · Triplet {triplet}"));
            item.evidence.context = triplet.to_string();
            items.push(item);
        }
        let origin = if items.is_empty() { " (keine Installationsdatenbank)" } else { " laut info/*.list" };
        return (items, origin);
    };
    let mut updates: Vec<String> = list(access, path(&["vcpkg", "updates"]))
        .into_iter()
        .filter(|(name, is_dir)| !is_dir && name != "incomplete")
        .map(|(name, _)| name)
        .collect();
    updates.sort();
    // (port, feature, triplet) → the latest paragraph.
    let mut database: BTreeMap<(String, String, String), Paragraph> = BTreeMap::new();
    let mut apply = |text: &str| {
        for paragraph in paragraphs(text) {
            let Some(port) = paragraph.get("Package").map(str::to_string) else { continue };
            let feature = paragraph.get("Feature").unwrap_or_default().to_string();
            let triplet = paragraph.get("Architecture").unwrap_or_default().to_string();
            database.insert((port, feature, triplet), paragraph);
        }
    };
    apply(&status);
    // Each pending update holds the paragraphs of one installed port.
    for update in updates.into_iter().take(1_000) {
        if let Some(text) = read(access, path(&["vcpkg", "updates", &update]), 1 << 20) {
            apply(&text);
        }
    }
    let is_installed = |p: &Paragraph| p.get("Status").and_then(|s| s.split_whitespace().last()) == Some("installed");
    // Installed features per port and triplet, gathered in one pass.
    let mut installed_features: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for ((port, feature, triplet), paragraph) in &database {
        if !feature.is_empty() && feature != "core" && is_installed(paragraph) {
            let features = installed_features.entry((port.as_str(), triplet.as_str())).or_default();
            if features.len() < 50 {
                features.push(feature.as_str());
            }
        }
    }
    for ((port, feature, triplet), paragraph) in &database {
        if !feature.is_empty() || !is_installed(paragraph) {
            continue;
        }
        let features = installed_features.get(&(port.as_str(), triplet.as_str())).cloned().unwrap_or_default();
        let version = match (paragraph.get("Version"), paragraph.get("Port-Version")) {
            (Some(version), Some(port_version)) if port_version != "0" => format!("{version}#{port_version}"),
            (Some(version), _) => version.to_string(),
            (None, _) => String::new(),
        };
        let mut detail = format!("installiert laut vcpkg/status · Triplet {triplet}");
        if !features.is_empty() {
            detail.push_str(&format!(" · Features {}", features.join(", ")));
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "vcpkg", port.as_str(), format!("{rel}/vcpkg/status"))
            .version(version)
            .state(InstallState::Installed)
            .detail(detail);
        item.evidence.context = triplet.clone();
        if let Some(depends) = paragraph.get("Depends") {
            item.evidence.dependencies = depends
                .split(',')
                .filter_map(|d| d.trim().split(['[', ':', ' ']).next().filter(|n| !n.is_empty()))
                .take(500)
                .map(|name| crate::domain::PackageDependency {
                    name: name.to_string(),
                    requirement: String::new(),
                    kind: "Depends".into(),
                    exact: false,
                })
                .collect();
        }
        items.push(item);
    }
    (items, " laut vcpkg/status")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const MANIFEST: &str = r#"{
  "$schema": "https://raw.githubusercontent.com/microsoft/vcpkg-tool/main/docs/vcpkg.schema.json",
  "name": "my-application",
  "version": "0.15.2",
  "builtin-baseline": "3426db05b996481ca31e95fff3734cf23e0f51bc",
  "dependencies": [
    "boost-system",
    { "name": "cpprestsdk", "default-features": false },
    {
      "name": "arrow",
      "default-features": false,
      "features": ["json", { "name": "mimalloc", "platform": "windows" }]
    },
    { "name": "fmt", "version>=": "10.1.1#1" },
    { "name": "vcpkg-cmake", "host": true },
    { "name": "picosha2", "platform": "!windows" }
  ],
  "features": {
    "tests": {
      "description": "Build tests",
      "dependencies": ["gtest", { "name": "fmt", "host": true }]
    }
  },
  "overrides": [
    { "name": "fmt", "version": "9.1.0" },
    { "name": "zlib", "version-string": "1.2.13", "port-version": 1 }
  ]
}"#;

    #[test]
    fn manifest_dependencies_features_hosts_and_overrides() {
        let items = Vcpkg.parse_file(&file("vcpkg.json", MANIFEST)).unwrap();
        assert_eq!(
            items[0].detail,
            "Manifest my-application 0.15.2 · Baseline 3426db05b996 · 8 Abhängigkeiten · 2 Overrides"
        );
        let find = |name: &str, context: &str| {
            items
                .iter()
                .find(|i| i.name == name && i.evidence.context == context)
                .unwrap_or_else(|| panic!("{name} {context}"))
        };
        let boost = find("boost-system", "");
        assert_eq!((boost.source, boost.source_host.as_deref()), (SourceClass::PublicRegistry, Some("github.com")));
        assert_eq!(boost.install_state, Some(InstallState::Declared));
        assert!(find("cpprestsdk", "").detail.ends_with("ohne Standardfeatures"));
        assert!(find("arrow", "").detail.contains("Features json, mimalloc"));
        assert_eq!(find("fmt", "").detail, "Spezifikation >= 10.1.1#1");
        assert!(find("vcpkg-cmake", "host").detail.ends_with("Host-Werkzeug"));
        assert!(find("picosha2", "").detail.ends_with("Plattform !windows"));
        assert!(find("gtest", "feature:tests").version.is_none());
        find("fmt", "feature:tests:host");
        assert_eq!(find("fmt", "override").version.as_deref(), Some("9.1.0"));
        assert_eq!(find("zlib", "override").version.as_deref(), Some("1.2.13#1"));

        let twice = r#"{"dependencies":[{"name":"zlib","platform":"windows"},{"name":"zlib","platform":"linux"}]}"#;
        let items = Vcpkg.parse_file(&file("vcpkg.json", twice)).unwrap();
        let qualifiers: Vec<&str> = items.iter().skip(1).map(|i| i.evidence.qualifier.as_str()).collect();
        assert_eq!(qualifiers, ["", "linux"], "a port listed per platform keeps both entries");
    }

    #[test]
    fn registries_route_sources_and_overlays_make_them_unknown() {
        let embedded = r#"{
  "dependencies": ["beicode", "boost-asio", "zlib", "fmt"],
  "configuration": {
    "default-registry": { "kind": "git", "repository": "https://github.com/microsoft/vcpkg", "baseline": "eefee7408133f3a0fef711ef9c6a3677b7e06fd7" },
    "registries": [
      { "kind": "git", "repository": "https://ci:s3cr3t@git.corp.example/vcpkg-registry.git", "baseline": "768f6a3ad9f9b6c4c2ff390137690cf26e3c3453", "packages": ["beicode", "boost-*"] },
      { "kind": "filesystem", "path": "C:/registries/local", "packages": ["fmt"] },
      { "kind": "artifact", "location": "https://aka.ms/vcpkg-ce-default", "name": "microsoft" }
    ]
  }
}"#;
        let items = Vcpkg.parse_file(&file("vcpkg.json", embedded)).unwrap();
        let source = |name: &str| {
            let item = items.iter().find(|i| i.name == name).unwrap();
            (item.source, item.source_host.clone())
        };
        assert_eq!(source("beicode"), (SourceClass::PrivateRegistry, Some("git.corp.example".into())));
        assert_eq!(source("boost-asio").0, SourceClass::PrivateRegistry);
        assert_eq!(source("zlib"), (SourceClass::PublicRegistry, Some("github.com".into())));
        assert_eq!(source("fmt"), (SourceClass::Path, None));
        assert!(items[0].detail.contains("3 weitere Registries (git, filesystem, artifact)"), "{}", items[0].detail);
        let debug = format!("{items:?}");
        assert!(!debug.contains("s3cr3t") && !debug.contains("registries/local"));

        let overlays = r#"{"dependencies":["zlib"],"vcpkg-configuration":{"overlay-ports":["./my-ports"]}}"#;
        let items = Vcpkg.parse_file(&file("vcpkg.json", overlays)).unwrap();
        assert_eq!(items[1].source, SourceClass::Unknown, "an overlay may provide any port");

        let siblings = ["vcpkg-configuration.json".to_string()];
        let separate =
            FoundFile { name: "vcpkg.json", rel_dir: "", content: r#"{"dependencies":["zlib"]}"#, siblings: &siblings };
        let items = Vcpkg.parse_file(&separate).unwrap();
        assert_eq!(items[1].source, SourceClass::Unknown);
        assert!(items[0].detail.contains("Registries in vcpkg-configuration.json"));

        let config = r#"{
  "default-registry": { "kind": "builtin", "baseline": "7476f0d4e77d3333fbb249657df8251c28c4faae" },
  "registries": [{ "kind": "git", "repository": "git@github.com:northwind/registry.git", "baseline": "x", "packages": ["beison"] }],
  "overlay-triplets": ["./triplets"]
}"#;
        let items = Vcpkg.parse_file(&file("vcpkg-configuration.json", config)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].detail,
            "Registry-Konfiguration · Standard-Registry builtin (öffentlich) · 1 weitere Registries (git) · 1 Overlay-Triplets"
        );
        assert!(Vcpkg.parse_file(&file("vcpkg-configuration.json", "{\"registries\": [")).is_err());
        assert!(Vcpkg.parse_file(&file("vcpkg.json", "[]")).is_err());
    }

    const STATUS: &str = "Package: vcpkg-cmake\nVersion: 2023-05-04\nArchitecture: x64-windows\nMulti-Arch: same\nAbi: 5a1ba4b2\nType: Port\nStatus: install ok installed\n\nPackage: zlib\nVersion: 1.3\nPort-Version: 1\nDepends: vcpkg-cmake:x64-windows\nArchitecture: x64-windows\nMulti-Arch: same\nAbi: 0d3d3e2f\nDescription: A compression library\n  continued description line\nStatus: install ok installed\n\nPackage: curl\nVersion: 8.4.0\nDepends: zlib\nArchitecture: x64-windows\nStatus: install ok installed\n\nPackage: curl\nFeature: ssl\nDepends: curl, openssl\nArchitecture: x64-windows\nStatus: install ok installed\n\nPackage: zlib\nVersion: 1.3\nArchitecture: x64-windows-static\nStatus: purge ok not-installed\n";

    #[test]
    fn installed_database_with_updates_features_and_triplets() {
        assert!(Vcpkg.claim_dir("vcpkg_installed", &["CMakeLists.txt".into()], &[]).is_none());
        let build = ["CMakeCache.txt".to_string()];
        assert!(Vcpkg.claim_dir("vcpkg_installed", &build, &["vcpkg".into(), "x64-windows".into()]).is_some());
        assert!(Vcpkg.claim_dir("installed", &build, &["vcpkg".into()]).is_none());
        let claim = Vcpkg.claim_dir("vcpkg_installed", &["vcpkg.json".into()], &["vcpkg".into()]).unwrap();
        let mut dir = FakeDir::default();
        dir.files.insert("vcpkg/status".into(), STATUS.into());
        dir.dirs.insert(
            "vcpkg/updates".into(),
            vec![("0000000001".into(), false), ("0000000000".into(), false), ("incomplete".into(), false)],
        );
        dir.files.insert(
            "vcpkg/updates/0000000000".into(),
            "Package: fmt\nVersion: 10.1.1\nArchitecture: x64-windows\nStatus: install ok installed\n".into(),
        );
        dir.files.insert(
            "vcpkg/updates/0000000001".into(),
            "Package: fmt\nVersion: 10.1.1\nArchitecture: x64-windows\nStatus: purge ok not-installed\n".into(),
        );
        let items = Vcpkg.inspect_dir(claim, "vcpkg_installed", &mut dir).unwrap();
        assert_eq!(items[0].detail, "3 installierte Ports laut vcpkg/status; nicht ausgeführt");
        let zlib = items.iter().find(|i| i.name == "zlib").unwrap();
        assert_eq!((zlib.version.as_deref(), zlib.evidence.context.as_str()), (Some("1.3#1"), "x64-windows"));
        assert_eq!(zlib.evidence.dependencies[0].name, "vcpkg-cmake");
        let curl = items.iter().find(|i| i.name == "curl").unwrap();
        assert!(curl.detail.ends_with("Features ssl"), "{}", curl.detail);
        assert!(!items.iter().any(|i| i.name == "fmt"), "a later update removed fmt");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Installed)));
    }

    #[test]
    fn list_file_names_without_status_and_vcpkg_instances() {
        let claim = Vcpkg.claim_dir("vcpkg_installed", &["vcpkg.json".into()], &[]).unwrap();
        let mut dir = FakeDir::default();
        dir.dirs.insert(
            "vcpkg/info".into(),
            vec![
                ("zlib_1.3_x64-windows.list".into(), false),
                ("vcpkg-cmake_2023-05-04_x64-windows.list".into(), false),
            ],
        );
        let items = Vcpkg.inspect_dir(claim, "build/vcpkg_installed", &mut dir).unwrap();
        assert_eq!(items.len(), 3);
        let cmake = items.iter().find(|i| i.name == "vcpkg-cmake").unwrap();
        assert_eq!((cmake.version.as_deref(), cmake.evidence.context.as_str()), (Some("2023-05-04"), "x64-windows"));

        let instance = Vcpkg.claim_dir("vcpkg", &[], &[".vcpkg-root".into(), "ports".into()]).unwrap();
        assert_eq!(instance.label, "vcpkg-Instanz");
        let mut dir = FakeDir::default();
        dir.files.insert("installed/vcpkg/status".into(), STATUS.into());
        let items = Vcpkg.inspect_dir(instance, "external/vcpkg", &mut dir).unwrap();
        assert!(items[0].detail.starts_with("vcpkg-Installation, Ports-Baum nicht durchsucht"));
        assert_eq!(items.iter().find(|i| i.name == "zlib").unwrap().rel_path, "external/vcpkg/installed/vcpkg/status");
        assert!(Vcpkg.claim_dir("ports", &[], &["zlib".into()]).is_none());
    }
}
