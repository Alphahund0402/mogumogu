//! Deno: deno.json / deno.jsonc (declared `imports` and `scopes`, task
//! names), deno.lock v1–v5 (resolved JSR, npm and remote modules) and the
//! store `node_modules/.deno` (installed names). Nothing is run, checked or
//! fetched. Import map files named by `importMap`, `.npmrc`, DENO_DIR and the
//! global cache are not read; remote modules keep host and module path only.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::json;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub struct Deno;

const PUBLIC: &[&str] = &[
    "jsr.io",
    "registry.npmjs.org",
    "deno.land",
    "esm.sh",
    "cdn.jsdelivr.net",
    "unpkg.com",
    "cdn.skypack.dev",
    "ga.jspm.io",
];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "deno",
    name: "Deno",
    markers: &["deno.json", "deno.jsonc", "deno.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "deno.json(c): imports, scopes, tasks, workspace",
        "deno.lock v1–v5",
        "node_modules/.deno (Ordnernamen)",
    ],
    limits: "Kein Deno-Aufruf; Import-Map-Dateien, .npmrc und DENO_DIR werden nicht gelesen – npm-Quellen ohne tarball-Angabe bleiben unbekannt, jsr:-Pakete gelten als jsr.io; URL-Importe nur als Host und Modulpfad.",
};

/// Requested range per resolved `(name, version)` from `specifiers`.
type Requested = BTreeMap<(String, String), String>;

impl Adapter for Deno {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("deno.lock") {
            let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
            return bounded(lock_items(file, &value));
        }
        let value = json::parse_jsonc(file.content, 64).map_err(|e| e.to_string())?;
        bounded(config_items(file, &value))
    }

    fn claim_dir(&self, name: &str, siblings: &[String], children: &[String]) -> Option<DirClaim> {
        (name == "node_modules" && store_beside_config(siblings, children)).then_some(DirClaim {
            adapter: "deno",
            category: ItemCategory::Environment,
            label: "node_modules (Deno)",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, "deno", "node_modules", rel_path)
                .detail("npm-Pakete in Denos Store; nicht ausgeführt"),
        ];
        for (entry, is_dir) in access.list(&[".deno"]) {
            // `name@1.2.3`, `@scope+name@1.2.3`, peers `_peer@1.0.0`, copies `_1`;
            // `_`-prefixed folders encode mixed-case names and are skipped.
            if !is_dir || entry.starts_with(['.', '_']) || entry == "node_modules" {
                continue;
            }
            let decoded = entry.replacen('+', "/", usize::from(entry.starts_with('@')));
            let Some((name, version)) = super::bun::split_spec(&decoded) else { continue };
            let mut item =
                InventoryItem::new(ItemCategory::Package, "deno", format!("npm:{name}"), format!("{rel_path}/.deno"))
                    .version(base_version(version))
                    .state(InstallState::Installed)
                    .detail("im Store node_modules/.deno vorhanden");
            item.evidence.qualifier = entry;
            items.push(item);
        }
        bounded(items)
    }
}

/// A node_modules holding Deno's store `.deno` beside a Deno config or
/// lockfile; npm leaves exactly these to this adapter.
pub(super) fn store_beside_config(siblings: &[String], children: &[String]) -> bool {
    children.iter().any(|c| c == ".deno")
        && siblings.iter().any(|s| matches!(s.as_str(), "deno.json" | "deno.jsonc" | "deno.lock"))
}

fn config_items(file: &FoundFile<'_>, value: &Value) -> Vec<InventoryItem> {
    let mut packages = Vec::new();
    let mut edges = Vec::new();
    let mut local = 0;
    let mut add = |alias: &str, target: &Value, scope: Option<&str>| {
        let target = target.as_str().unwrap_or("");
        match import(file, alias, target, scope) {
            Some((item, edge)) => {
                packages.push(item);
                edges.push(edge);
            }
            None => local += usize::from(is_local(target)),
        }
    };
    for (alias, target) in value["imports"].as_object().into_iter().flatten().take(20_000) {
        add(alias, target, None);
    }
    for (index, (scope, map)) in value["scopes"].as_object().into_iter().flatten().take(1_000).enumerate() {
        let label = scope_label(scope, index);
        for (alias, target) in map.as_object().into_iter().flatten().take(20_000) {
            add(alias, target, Some(&label));
        }
    }
    let mut detail = match value["name"].as_str() {
        Some(name) => format!("Paket {name}"),
        None => "Deno-Konfiguration".to_string(),
    };
    detail.push_str(&format!(" · {} Importe", packages.len()));
    if local > 0 {
        detail.push_str(&format!(" · {local} lokale Zuordnungen"));
    }
    let members = match &value["workspace"] {
        Value::Array(list) => list.len(),
        workspace => workspace["members"].as_array().map_or(0, Vec::len),
    };
    if members > 0 {
        detail.push_str(&format!(" · Workspace mit {members} Mitgliedern"));
    }
    let tasks = value["tasks"].as_object();
    if tasks.is_some_and(|t| !t.is_empty()) {
        detail.push_str(" · Tasks vorhanden (nicht ausgeführt)");
    }
    if value["importMap"].is_string() {
        detail.push_str(" · Import-Map-Datei nicht gelesen");
    }
    match &value["lock"] {
        Value::Bool(false) => detail.push_str(" · Lockfile abgeschaltet"),
        Value::String(_) => detail.push_str(" · eigener Lockfile-Pfad"),
        lock if lock["path"].is_string() => detail.push_str(" · eigener Lockfile-Pfad"),
        _ => {}
    }
    let links = value["links"].as_array().or_else(|| value["patch"].as_array()).map_or(0, Vec::len);
    if links > 0 {
        detail.push_str(&format!(" · {links} lokale Paketverknüpfungen"));
    }
    if value["vendor"].as_bool() == Some(true) {
        detail.push_str(" · vendor-Ordner aktiviert (nicht inventarisiert)");
    }
    let mut items = vec![manifest("deno", file, detail)];
    items[0].evidence.dependencies = edges;
    let mut commands: Vec<String> = tasks
        .into_iter()
        .flatten()
        .filter_map(|(_, task)| task.as_str().or_else(|| task["command"].as_str()))
        .flat_map(super::commands::names)
        .collect();
    commands.sort();
    commands.dedup();
    commands.truncate(50);
    items[0].evidence.commands = commands;
    items.extend(packages);
    items
}

/// One import map entry as a declared package. Local paths, `node:`
/// built-ins and bare remappings are no packages.
fn import(
    file: &FoundFile<'_>,
    alias: &str,
    target: &str,
    scope: Option<&str>,
) -> Option<(InventoryItem, PackageDependency)> {
    let (name, spec, (source, host), detail) = if let Some((name, range)) = registry_spec(target) {
        let source = if name.starts_with("jsr:") {
            (SourceClass::PublicRegistry, Some("jsr.io".to_string()))
        } else {
            (SourceClass::Unknown, None)
        };
        let detail =
            if range.is_empty() { "ohne Versionsangabe".to_string() } else { format!("Spezifikation {range}") };
        (name, range.to_string(), source, detail)
    } else if target.starts_with("https://") || target.starts_with("http://") {
        let (name, version) = url_module(target)?;
        let detail = if version.is_empty() {
            "URL-Import ohne Versionsangabe".into()
        } else {
            format!("URL-Import · Version {version}")
        };
        (name, version, classify_url(target, PUBLIC), detail)
    } else {
        return None;
    };
    let context = if scope.is_some() { "scopes" } else { "imports" };
    let mut item = InventoryItem::new(ItemCategory::Package, "deno", name.as_str(), file.rel_path())
        .state(InstallState::Declared)
        .source(source, host)
        .detail(match scope {
            Some(scope) => format!("{detail} · Scope {scope}"),
            None => detail,
        });
    if semver::Version::parse(spec.strip_prefix('v').unwrap_or(&spec)).is_ok() {
        item = item.version(spec.as_str());
    }
    // Import map keys may be URLs themselves; those keep host and module path.
    let alias = if alias.contains("://") {
        match url_module(alias) {
            Some((module, version)) if version.is_empty() => module,
            Some((module, version)) => format!("{module}@{version}"),
            None => "URL".into(),
        }
    } else {
        alias.to_string()
    };
    item.evidence.context = context.into();
    item.evidence.qualifier = scope.map_or_else(|| alias.clone(), |scope| format!("{scope} {alias}"));
    Some((item, PackageDependency { name, requirement: spec, kind: context.into(), exact: false }))
}

fn is_local(target: &str) -> bool {
    target.starts_with("./") || target.starts_with("../") || target.starts_with('/') || target.starts_with("file:")
}

/// A scope key as shown: relative prefixes as written, URLs by host only.
fn scope_label(key: &str, index: usize) -> String {
    if key.starts_with("./") || key.starts_with("../") {
        return key.to_string();
    }
    crate::privacy::url_host(key).unwrap_or_else(|| format!("Scope {}", index + 1))
}

/// `jsr:@std/path@^1.0.0/posix` or `npm:/preact@10/` → (`jsr:@std/path`,
/// `^1.0.0`); the range is empty when none is given.
fn registry_spec(spec: &str) -> Option<(String, &str)> {
    let (scheme, rest) = spec.split_once(':')?;
    if scheme != "jsr" && scheme != "npm" {
        return None;
    }
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let start = if rest.starts_with('@') { rest.find('/')? + 1 } else { 0 };
    let end = rest[start..].find(['@', '/']).map_or(rest.len(), |index| start + index);
    let name = &rest[..end];
    let valid = name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '-' | '_' | '.' | '~'));
    if end == start || !valid {
        return None;
    }
    let range = rest[end..].strip_prefix('@').map_or("", |range| range.split('/').next().unwrap_or(""));
    Some((format!("{scheme}:{name}"), range))
}

/// `https://deno.land/x/oak@v12.6.1/mod.ts` → (`deno.land/x/oak`, `v12.6.1`):
/// the host and the path up to the versioned segment. Without such a
/// segment only the host is kept; queries, credentials and files never are.
fn url_module(url: &str) -> Option<(String, String)> {
    let host = crate::privacy::url_host(url)?;
    let rest = url.split_once("://")?.1;
    let path = rest.split(['?', '#']).next().unwrap_or("").split_once('/').map_or("", |(_, path)| path);
    // esm.sh puts build folders (`v135`, `stable`) before the package.
    let esm = host == "esm.sh";
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '@' | '-' | '_' | '.' | '~' | '+' | '^' | '=');
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/').take(8) {
        let segment = if esm { segment.trim_start_matches('*') } else { segment };
        let build =
            segment == "stable" || segment.strip_prefix('v').is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()));
        if esm && parts.is_empty() && build && segment.len() > 1 {
            continue;
        }
        if segment.is_empty() || !segment.chars().all(plain) {
            break;
        }
        match segment.get(1..).and_then(|s| s.find('@')) {
            Some(index) if index + 2 < segment.len() => {
                parts.push(&segment[..index + 1]);
                return Some((format!("{host}/{}", parts.join("/")), segment[index + 2..].to_string()));
            }
            Some(_) => break,
            None => parts.push(segment),
        }
    }
    Some((host, String::new()))
}

/// `18.3.1_react@18.3.1` (peer suffix) or `5.3.0_1` (copy) → `18.3.1`.
fn base_version(version: &str) -> &str {
    version.split('_').next().unwrap_or(version)
}

/// deno.lock: v1 is a plain URL → checksum map, v2 nests npm packages under
/// `npm`, v3 all packages under `packages`, v4 and v5 keep them at the top.
/// Other versions are read by the same shapes and marked in the detail.
fn lock_items(file: &FoundFile<'_>, value: &Value) -> Vec<InventoryItem> {
    let v1 = value.get("version").is_none();
    let version = value["version"].as_str().unwrap_or(if v1 { "1" } else { "?" });
    let v2 = value["npm"]["packages"].is_object();
    let root = if value["packages"].is_object() { &value["packages"] } else { value };
    let (specifiers, jsr, npm) = if v2 {
        (value["npm"]["specifiers"].as_object(), None, value["npm"]["packages"].as_object())
    } else {
        (root["specifiers"].as_object(), root["jsr"].as_object(), root["npm"].as_object())
    };
    let requested = requested(specifiers, v2);
    let rel = file.rel_path();
    let mut items = Vec::new();
    for (key, entry) in jsr.into_iter().flatten().take(100_000) {
        let Some((name, version)) = super::bun::split_spec(key) else { continue };
        let mut item = resolved(&format!("jsr:{name}"), version, key, &rel, &requested, "")
            .source(SourceClass::PublicRegistry, Some("jsr.io".into()));
        item.evidence.dependencies = entry["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|spec| {
                let (name, range) = registry_spec(spec)?;
                Some(PackageDependency { name, requirement: range.into(), kind: "lock".into(), exact: false })
            })
            .collect();
        items.push(item);
    }
    let jsr_count = items.len();
    for (key, entry) in npm.into_iter().flatten().take(100_000) {
        let Some((name, version)) = super::bun::split_spec(key) else { continue };
        let note =
            if entry["scripts"].as_bool() == Some(true) { " · Installationsskripte (nicht ausgeführt)" } else { "" };
        let (source, host) =
            entry["tarball"].as_str().map_or((SourceClass::Unknown, None), |url| classify_url(url, PUBLIC));
        let mut item =
            resolved(&format!("npm:{name}"), base_version(version), key, &rel, &requested, note).source(source, host);
        item.evidence.dependencies = npm_edges(entry);
        item.evidence.deprecated =
            (entry["deprecated"].as_bool() == Some(true)).then(|| "laut deno.lock veraltet".into());
        items.push(item);
    }
    let npm_count = items.len() - jsr_count;
    let remote = if v1 { value.as_object() } else { value["remote"].as_object() };
    let mut modules: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    for url in remote.into_iter().flatten().map(|(url, _)| url).take(200_000) {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            continue;
        }
        if let Some(module) = url_module(url) {
            modules.entry(module).or_insert_with(|| (0, url.clone())).0 += 1;
        }
    }
    let remote_count = modules.len();
    for ((name, version), (files, url)) in modules {
        let (source, host) = classify_url(&url, PUBLIC);
        let mut item = InventoryItem::new(ItemCategory::Package, "deno", name, rel.as_str())
            .version(version.as_str())
            .state(InstallState::Resolved)
            .source(source, host)
            .detail(format!("Remote-Modul · {files} Dateien mit Prüfsumme in deno.lock"));
        item.evidence.context = "remote".into();
        item.evidence.qualifier = version;
        items.push(item);
    }
    let workspace = &value["workspace"];
    let mut edges = workspace_edges(workspace, "workspace");
    let members = workspace["members"].as_object();
    for (_, member) in members.into_iter().flatten().take(1_000) {
        edges.extend(workspace_edges(member, "workspace member"));
    }
    let mut detail = format!("Lockfile v{version} · {jsr_count} JSR, {npm_count} npm, {remote_count} Remote-Module");
    if let Some(members) = members.filter(|m| !m.is_empty()) {
        detail.push_str(&format!(" · Workspace mit {} Mitgliedern", members.len()));
    }
    if !matches!(version, "1" | "2" | "3" | "4" | "5") {
        detail.push_str(" · unbekannte Version, nur bekannte Felder gelesen");
    }
    let mut head = manifest("deno", file, detail);
    head.evidence.dependencies = edges;
    items.insert(0, head);
    items
}

/// `jsr:@std/path@^1` → `1.0.8` (v4/v5), `jsr:@std/path@1.0.8` (v3 and
/// aliases) or `chalk@5` → `chalk@5.3.0` (v2, npm only).
fn requested(specifiers: Option<&Map<String, Value>>, v2: bool) -> Requested {
    let mut out = Requested::new();
    for (key, target) in specifiers.into_iter().flatten().take(100_000) {
        let Some(target) = target.as_str() else { continue };
        let (key, target) =
            if v2 { (format!("npm:{key}"), format!("npm:{target}")) } else { (key.clone(), target.into()) };
        let Some((name, range)) = registry_spec(&key) else { continue };
        let (name, version) = match registry_spec(&target) {
            Some((name, version)) => (name, version.to_string()),
            None => (name, target.clone()),
        };
        out.entry((name, base_version(&version).to_string())).or_insert_with(|| range.to_string());
    }
    out
}

fn resolved(name: &str, version: &str, key: &str, rel: &str, requested: &Requested, note: &str) -> InventoryItem {
    let mut detail = String::from("aufgelöst laut deno.lock");
    if let Some(range) = requested.get(&(name.to_string(), version.to_string()))
        && !range.is_empty()
    {
        detail.push_str(&format!(" · Spezifikation {range}"));
    }
    detail.push_str(note);
    let mut item = InventoryItem::new(ItemCategory::Package, "deno", name, rel)
        .version(version)
        .state(InstallState::Resolved)
        .detail(detail);
    item.evidence.qualifier = key.to_string();
    item
}

/// v4/v5 list `name`, `name@1.0.0` or `alias@npm:name@1.0.0`; v2/v3 map an
/// alias to `name@1.0.0`. Versions keep no peer suffix.
fn npm_edges(entry: &Value) -> Vec<PackageDependency> {
    let mut edges = Vec::new();
    for (field, kind) in
        [("dependencies", "lock"), ("optionalDependencies", "optional"), ("optionalPeers", "optional peer")]
    {
        let ids: Vec<&str> = match &entry[field] {
            Value::Array(list) => list
                .iter()
                .filter_map(Value::as_str)
                .map(|id| id.split_once("@npm:").map_or(id, |(_, target)| target))
                .collect(),
            Value::Object(map) => map.values().filter_map(Value::as_str).collect(),
            _ => continue,
        };
        for id in ids.into_iter().take(10_000) {
            let (name, version) = super::bun::split_spec(id).map_or((id, ""), |(name, v)| (name, base_version(v)));
            if !name.is_empty() {
                edges.push(PackageDependency {
                    name: format!("npm:{name}"),
                    exact: !version.is_empty(),
                    requirement: version.into(),
                    kind: kind.into(),
                });
            }
        }
    }
    edges
}

/// `dependencies` and `packageJson.dependencies` of the workspace root or a
/// member: the requested specifiers, not resolved versions.
fn workspace_edges(config: &Value, kind: &str) -> Vec<PackageDependency> {
    let mut edges = Vec::new();
    for (list, kind) in [
        (&config["dependencies"], kind.to_string()),
        (&config["packageJson"]["dependencies"], format!("{kind} · package.json")),
    ] {
        for spec in list.as_array().into_iter().flatten().filter_map(Value::as_str).take(10_000) {
            if let Some((name, range)) = registry_spec(spec) {
                edges.push(PackageDependency { name, requirement: range.into(), kind: kind.clone(), exact: false });
            }
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    fn find<'a>(items: &'a [InventoryItem], name: &str) -> &'a InventoryItem {
        items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name} missing: {items:#?}"))
    }

    #[test]
    fn config_imports_scopes_and_tasks() {
        let config = r#"{
          // JSONC with comments and trailing commas
          "name": "@acme/app",
          "workspace": ["./packages/a", "./packages/b"],
          "imports": {
            "@std/path": "jsr:@std/path@^1.0.0",
            "@std/assert": "jsr:@std/assert@1.0.8",
            "chalk": "npm:chalk@^5",
            "preact/": "npm:/preact@10.24.3/",
            "@std/": "jsr:/@std/",
            "oak": "https://deno.land/x/oak@v12.6.1/mod.ts",
            "react": "https://esm.sh/react@18?dev",
            "corp": "https://deploy:s3cr3tpw@deno.corp.example/lib@2.0.0/mod.ts",
            "raw": "https://raw.githubusercontent.com/acme/tools/main/mod.ts",
            "https://deno.land/std/": "https://deno.land/std@0.224.0/",
            "https://mirror.example/pad.ts?token=t0k3n": "npm:left-pad@1.3.0",
            "@/": "./src/",
            "fs": "node:fs",
          },
          "scopes": {
            "./legacy/": { "@std/path": "jsr:@std/path@^0.224.0" },
            "https://deno.land/x/old/": { "chalk": "npm:chalk@4" },
          },
          "tasks": {
            "dev": "deno run --watch --allow-net main.ts",
            "build": { "command": "TOKEN=abc123 deno compile -A main.ts && echo done", "description": "Build" },
          },
          "importMap": "./import_map.json",
          "lock": { "path": "./deps.lock", "frozen": true },
        }"#;
        let items = Deno.parse_file(&file("deno.jsonc", config)).unwrap();
        let head = &items[0];
        assert_eq!(head.category, ItemCategory::Manifest);
        assert!(head.detail.starts_with("Paket @acme/app · 12 Importe · 1 lokale Zuordnungen"), "{}", head.detail);
        assert!(head.detail.contains("Workspace mit 2 Mitgliedern") && head.detail.contains("Import-Map-Datei"));
        assert!(head.detail.ends_with("eigener Lockfile-Pfad"), "{}", head.detail);
        assert_eq!(head.evidence.commands, ["deno", "echo"]);
        assert_eq!(head.evidence.dependencies.len(), 12);
        let std = find(&items, "deno.land/std");
        assert_eq!((std.version.as_deref(), std.evidence.qualifier.as_str()), (Some("0.224.0"), "deno.land"));
        assert_eq!(find(&items, "npm:left-pad").evidence.qualifier, "mirror.example", "URL keys keep the host");

        let path = items.iter().find(|i| i.name == "jsr:@std/path" && i.evidence.context == "imports").unwrap();
        assert_eq!((path.install_state, path.source), (Some(InstallState::Declared), SourceClass::PublicRegistry));
        assert_eq!((path.source_host.as_deref(), path.version.as_deref()), (Some("jsr.io"), None));
        assert!(path.detail.ends_with("Spezifikation ^1.0.0"), "{}", path.detail);
        assert_eq!(find(&items, "jsr:@std/assert").version.as_deref(), Some("1.0.8"), "an exact version is kept");
        let chalk = items.iter().find(|i| i.name == "npm:chalk" && i.evidence.context == "imports").unwrap();
        assert_eq!(chalk.source, SourceClass::Unknown, ".npmrc is not read");
        assert!(find(&items, "npm:preact").detail.ends_with("Spezifikation 10.24.3"));
        let oak = find(&items, "deno.land/x/oak");
        assert_eq!((oak.version.as_deref(), oak.source), (Some("v12.6.1"), SourceClass::PublicRegistry));
        assert_eq!(find(&items, "esm.sh/react").detail, "URL-Import · Version 18");
        let corp = find(&items, "deno.corp.example/lib");
        assert_eq!(
            (corp.source, corp.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("deno.corp.example"))
        );
        let raw = find(&items, "raw.githubusercontent.com");
        assert!(raw.version.is_none() && raw.detail == "URL-Import ohne Versionsangabe");

        let legacy = items.iter().find(|i| i.name == "jsr:@std/path" && i.evidence.context == "scopes").unwrap();
        assert_eq!(legacy.evidence.qualifier, "./legacy/ @std/path");
        assert!(legacy.detail.ends_with("Scope ./legacy/"));
        let old = items.iter().find(|i| i.name == "npm:chalk" && i.evidence.context == "scopes").unwrap();
        assert_eq!(old.evidence.qualifier, "deno.land chalk", "URL scopes keep the host only");

        let all = format!("{items:?}");
        for hidden in
            ["s3cr3tpw", "deploy", "acme/tools", "?dev", "abc123", "t0k3n", "pad.ts", "./src/", "node:fs", "jsr:@std\""]
        {
            assert!(!all.contains(hidden), "{hidden} must not be stored");
        }
        assert!(Deno.parse_file(&file("deno.json", "{\"imports\": ")).is_err());
    }

    const V5: &str = r#"{
      "version": "5",
      "specifiers": {
        "jsr:@std/path@^1.0.0": "1.0.8",
        "jsr:@std/internal@^1.0.5": "1.0.5",
        "npm:chalk@^5.3.0": "5.3.0",
        "npm:react@18": "18.3.1",
        "npm:react-dom@18": "18.3.1_react@18.3.1",
        "npm:@corp/tool@1": "1.2.0"
      },
      "jsr": {
        "@std/internal@1.0.5": { "integrity": "a1" },
        "@std/path@1.0.8": { "integrity": "a2", "dependencies": ["jsr:@std/internal"] }
      },
      "npm": {
        "chalk@5.3.0": { "integrity": "sha512-a" },
        "loose-envify@1.4.0": { "integrity": "sha512-b", "bin": true },
        "react@18.3.1": { "integrity": "sha512-c", "dependencies": ["loose-envify"] },
        "react-dom@18.3.1_react@18.3.1": {
          "integrity": "sha512-d",
          "dependencies": ["loose-envify", "react", "scheduler@0.23.2"],
          "optionalPeers": ["@types/react@18.3.0"]
        },
        "scheduler@0.23.2": { "integrity": "sha512-e", "dependencies": ["loose-envify"] },
        "@corp/tool@1.2.0": {
          "integrity": "sha512-f",
          "tarball": "https://ci:s3cr3tpw@npm.corp.example/@corp/tool/-/tool-1.2.0.tgz",
          "scripts": true,
          "deprecated": true
        }
      },
      "remote": {
        "https://deno.land/x/oak@v12.6.1/mod.ts": "h1",
        "https://deno.land/x/oak@v12.6.1/router.ts": "h2",
        "https://esm.sh/v135/react@18.2.0/denonext/react.mjs": "h3",
        "https://esm.sh/react@18.2.0?target=denonext&token=abc": "h4",
        "https://raw.githubusercontent.com/acme/tools/main/mod.ts": "h5"
      },
      "workspace": {
        "dependencies": ["jsr:@std/path@^1.0.0", "npm:chalk@^5.3.0"],
        "packageJson": { "dependencies": ["npm:react@18", "npm:react-dom@18"] },
        "members": { "packages/a": { "dependencies": ["npm:@corp/tool@1"] } }
      }
    }"#;

    #[test]
    fn lockfile_v5_with_peers_tarballs_remote_modules_and_workspace() {
        let items = Deno.parse_file(&file("deno.lock", V5)).unwrap();
        assert_eq!(
            items[0].detail, "Lockfile v5 · 2 JSR, 6 npm, 3 Remote-Module · Workspace mit 1 Mitgliedern",
            "{:#?}",
            items
        );
        let edges = &items[0].evidence.dependencies;
        assert_eq!(edges.len(), 5);
        assert!(edges.iter().any(|e| e.name == "jsr:@std/path" && e.requirement == "^1.0.0" && e.kind == "workspace"));
        assert!(edges.iter().any(|e| e.name == "npm:react-dom" && e.kind == "workspace · package.json"));
        assert!(edges.iter().any(|e| e.name == "npm:@corp/tool" && e.kind == "workspace member"));

        let path = find(&items, "jsr:@std/path");
        assert_eq!((path.version.as_deref(), path.install_state), (Some("1.0.8"), Some(InstallState::Resolved)));
        assert_eq!(path.detail, "aufgelöst laut deno.lock · Spezifikation ^1.0.0");
        assert_eq!((path.source, path.source_host.as_deref()), (SourceClass::PublicRegistry, Some("jsr.io")));
        assert_eq!(path.evidence.dependencies[0].name, "jsr:@std/internal");

        let dom = find(&items, "npm:react-dom");
        assert_eq!(dom.version.as_deref(), Some("18.3.1"));
        assert_eq!(dom.evidence.qualifier, "react-dom@18.3.1_react@18.3.1");
        assert!(dom.detail.ends_with("Spezifikation 18"), "{}", dom.detail);
        let deps: Vec<(&str, &str, &str, bool)> = dom
            .evidence
            .dependencies
            .iter()
            .map(|d| (d.name.as_str(), d.requirement.as_str(), d.kind.as_str(), d.exact))
            .collect();
        assert_eq!(
            deps,
            [
                ("npm:loose-envify", "", "lock", false),
                ("npm:react", "", "lock", false),
                ("npm:scheduler", "0.23.2", "lock", true),
                ("npm:@types/react", "18.3.0", "optional peer", true),
            ]
        );
        assert_eq!(find(&items, "npm:chalk").source, SourceClass::Unknown, "no tarball: registry from .npmrc");
        let tool = find(&items, "npm:@corp/tool");
        assert_eq!(
            (tool.source, tool.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("npm.corp.example"))
        );
        assert!(tool.detail.ends_with("Installationsskripte (nicht ausgeführt)") && tool.evidence.deprecated.is_some());

        let oak = find(&items, "deno.land/x/oak");
        assert_eq!((oak.version.as_deref(), oak.evidence.context.as_str()), (Some("v12.6.1"), "remote"));
        assert!(oak.detail.starts_with("Remote-Modul · 2 Dateien"));
        let react = find(&items, "esm.sh/react");
        assert!(react.version.as_deref() == Some("18.2.0") && react.detail.contains("2 Dateien"), "{react:?}");
        let raw = find(&items, "raw.githubusercontent.com");
        assert_eq!((raw.version.as_deref(), raw.source), (None, SourceClass::PrivateRegistry));
        let all = format!("{items:?}");
        for hidden in ["s3cr3tpw", "token", "acme/tools", "integrity", "sha512"] {
            assert!(!all.contains(hidden), "{hidden} must not be stored");
        }
    }

    #[test]
    fn lockfile_v4_lists_versions_and_aliases() {
        let lock = r#"{
          "version": "4",
          "specifiers": {
            "jsr:@std/assert@1": "1.0.6",
            "npm:other@1": "npm:package-d@1.0.0",
            "npm:package-a@3": "3.3.4"
          },
          "jsr": { "@std/assert@1.0.6": { "integrity": "x", "dependencies": ["jsr:@std/internal@^1.0.4", "npm:chalk@5"] } },
          "npm": {
            "package-a@3.3.4": { "integrity": "y", "dependencies": ["package-b", "package-c@1.0.0", "other@npm:package-d@1.0.0"] },
            "package-d@1.0.0": { "integrity": "z" }
          },
          "remote": {},
          "workspace": { "dependencies": ["jsr:@std/assert@1", "npm:other@1", "npm:package-a@3"] }
        }"#;
        let items = Deno.parse_file(&file("deno.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v4 · 1 JSR, 2 npm, 0 Remote-Module");
        let assert = find(&items, "jsr:@std/assert");
        assert_eq!(assert.detail, "aufgelöst laut deno.lock · Spezifikation 1");
        let deps: Vec<(&str, &str)> =
            assert.evidence.dependencies.iter().map(|d| (d.name.as_str(), d.requirement.as_str())).collect();
        assert_eq!(deps, [("jsr:@std/internal", "^1.0.4"), ("npm:chalk", "5")]);
        let a = find(&items, "npm:package-a");
        let names: Vec<&str> = a.evidence.dependencies.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["npm:package-b", "npm:package-c", "npm:package-d"]);
        assert!(find(&items, "npm:package-d").detail.ends_with("Spezifikation 1"), "aliased specifier");
    }

    #[test]
    fn lockfile_v3_and_v2_layouts() {
        let v3 = r#"{
          "version": "3",
          "packages": {
            "specifiers": { "jsr:@std/assert@^0.218.2": "jsr:@std/assert@0.218.2", "npm:wrap@1": "npm:wrap@1.0.0" },
            "jsr": {
              "@std/assert@0.218.2": { "integrity": "x", "dependencies": ["jsr:@std/fmt@^0.218.2"] },
              "@std/fmt@0.218.2": { "integrity": "y" }
            },
            "npm": {
              "ansi-styles@6.2.1": { "integrity": "a", "dependencies": {} },
              "wrap@1.0.0": { "integrity": "b", "dependencies": { "ansi-styles": "ansi-styles@6.2.1", "other": "package-d@1.0.0_peer@2.0.0" } }
            }
          },
          "remote": { "https://deno.land/std@0.170.0/fmt/colors.ts": "h" },
          "workspace": { "dependencies": ["jsr:@std/assert@^0.218.2", "npm:wrap@1"] }
        }"#;
        let items = Deno.parse_file(&file("deno.lock", v3)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v3 · 2 JSR, 2 npm, 1 Remote-Module");
        assert_eq!(items[0].evidence.dependencies.len(), 2);
        let assert = find(&items, "jsr:@std/assert");
        assert!(assert.version.as_deref() == Some("0.218.2") && assert.detail.ends_with("Spezifikation ^0.218.2"));
        assert_eq!(assert.evidence.dependencies[0].requirement, "^0.218.2");
        let wrap = find(&items, "npm:wrap");
        let deps: Vec<(&str, &str, bool)> =
            wrap.evidence.dependencies.iter().map(|d| (d.name.as_str(), d.requirement.as_str(), d.exact)).collect();
        assert_eq!(deps, [("npm:ansi-styles", "6.2.1", true), ("npm:package-d", "1.0.0", true)]);
        assert_eq!(find(&items, "deno.land/std").version.as_deref(), Some("0.170.0"));

        let v2 = r#"{
          "version": "2",
          "remote": { "https://deno.land/std@0.170.0/fmt/colors.ts": "h" },
          "npm": {
            "specifiers": { "chalk@5": "chalk@5.2.0" },
            "packages": { "chalk@5.2.0": { "integrity": "sha512-x", "dependencies": {} } }
          }
        }"#;
        let items = Deno.parse_file(&file("deno.lock", v2)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v2 · 0 JSR, 1 npm, 1 Remote-Module");
        let chalk = find(&items, "npm:chalk");
        assert_eq!(
            (chalk.version.as_deref(), chalk.detail.as_str()),
            (Some("5.2.0"), "aufgelöst laut deno.lock · Spezifikation 5")
        );
    }

    #[test]
    fn lockfile_v1_unknown_versions_and_broken_input() {
        let v1 = r#"{ "https://deno.land/std@0.150.0/path/mod.ts": "h1", "https://deno.land/std@0.150.0/path/win32.ts": "h2" }"#;
        let items = Deno.parse_file(&file("deno.lock", v1)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v1 · 0 JSR, 0 npm, 1 Remote-Module");
        assert_eq!(find(&items, "deno.land/std").version.as_deref(), Some("0.150.0"));

        let future = r#"{ "version": "9", "specifiers": { "npm:chalk@5": "5.3.0" }, "npm": { "chalk@5.3.0": {} }, "newField": {} }"#;
        let items = Deno.parse_file(&file("deno.lock", future)).unwrap();
        assert!(items[0].detail.ends_with("unbekannte Version, nur bekannte Felder gelesen"), "{}", items[0].detail);
        assert_eq!(find(&items, "npm:chalk").version.as_deref(), Some("5.3.0"));
        assert!(Deno.parse_file(&file("deno.lock", "{\"version\": \"4\", ")).is_err());
    }

    #[test]
    fn deno_store_in_node_modules() {
        assert!(Deno.claim_dir("node_modules", &["deno.json".into()], &[".deno".into(), "chalk".into()]).is_some());
        assert!(Deno.claim_dir("node_modules", &["deno.json".into()], &[".package-lock.json".into()]).is_none());
        assert!(Deno.claim_dir("node_modules", &["package.json".into()], &[".deno".into()]).is_none(), "no sibling");
        let mut dir = FakeDir::default();
        dir.dirs.insert(
            ".deno".into(),
            vec![
                ("chalk@5.3.0".into(), true),
                ("@denotest+add@1.0.0".into(), true),
                ("react-dom@18.3.1_react@18.3.1".into(), true),
                ("_ab1c2d@1.0.0".into(), true),
                (".setup-cache.bin".into(), false),
                ("node_modules".into(), true),
            ],
        );
        let claim = Deno.claim_dir("node_modules", &["deno.lock".into()], &[".deno".into()]).unwrap();
        let items = Deno.inspect_dir(claim, "node_modules", &mut dir).unwrap();
        assert_eq!(items.len(), 4, "{items:#?}");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Installed)));
        assert_eq!(find(&items, "npm:@denotest/add").version.as_deref(), Some("1.0.0"));
        assert_eq!(find(&items, "npm:react-dom").version.as_deref(), Some("18.3.1"));
    }

    #[test]
    fn specifiers_and_url_modules() {
        assert_eq!(registry_spec("jsr:@std/path@^1.0.0/posix"), Some(("jsr:@std/path".into(), "^1.0.0")));
        assert_eq!(registry_spec("npm:/@types/node@22/"), Some(("npm:@types/node".into(), "22")));
        assert_eq!(registry_spec("npm:chalk"), Some(("npm:chalk".into(), "")));
        assert_eq!(registry_spec("jsr:/@std/"), None);
        assert_eq!(registry_spec("node:fs"), None);
        let module = |url: &str| url_module(url).unwrap();
        assert_eq!(
            module("https://esm.sh/@preact/signals@1.2.0?bundle"),
            ("esm.sh/@preact/signals".into(), "1.2.0".into())
        );
        assert_eq!(
            module("https://cdn.jsdelivr.net/npm/lodash@4.17.21/lodash.min.js").0,
            "cdn.jsdelivr.net/npm/lodash"
        );
        assert_eq!(module("https://esm.sh/stable/preact@10.19.2/denonext/preact.mjs").0, "esm.sh/preact");
        assert_eq!(module("https://example.com/%40scope/x@1.0.0/mod.ts"), ("example.com".into(), String::new()));
        assert_eq!(module("https://example.com/x@/mod.ts"), ("example.com".into(), String::new()));
    }
}
