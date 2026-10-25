//! Nix: flake.lock (resolved inputs of the flake graph, direct and
//! transitive), flake.nix (inputs with a literal URL, matched as text) and
//! devbox.json / devbox.lock (Nix packages of a Devbox environment; the
//! generated `.devbox` folder is claimed, not walked). Nix code is never
//! evaluated; nothing is built, fetched or read from the store. URL paths,
//! queries and store paths are never kept.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::limits::REFERENCE_MAX_DEPTH;
use crate::parsers::json;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub struct Nix;

const PUBLIC: &[&str] = &[
    "github.com",
    "gitlab.com",
    "git.sr.ht",
    "flakehub.com",
    "api.flakehub.com",
    "channels.nixos.org",
    "releases.nixos.org",
];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "nix",
    name: "Nix",
    markers: &["flake.lock", "flake.nix", "devbox.json", "devbox.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["flake.lock v5–v7", "flake.nix (wörtliche url im inputs-Block)", "devbox.json", "devbox.lock v1"],
    limits: "Kein nix- oder devbox-Aufruf; flake.nix wird nicht ausgewertet – nur wörtliche url-Angaben vor outputs; Registry-Verweise (indirect) werden nicht aufgelöst, der Nix-Store wird nie gelesen.",
};

/// `name.url = "…";` or `inputs.name.url = "…";` on one line.
static INPUT_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*(?:inputs\s*\.\s*)?([A-Za-z_][\w'-]*)\s*\.\s*url\s*=\s*"([^"]*)"\s*;"#).expect("valid regex")
});
/// `name = {` or `inputs.name = {`: the attribute set of one input.
static INPUT_SET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*(?:inputs\s*\.\s*)?([A-Za-z_][\w'-]*)\s*=\s*\{"#).expect("valid regex"));
static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:^|[\s{;])url\s*=\s*"([^"]*)"\s*;"#).expect("valid regex"));

impl Adapter for Nix {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name.to_ascii_lowercase().as_str() {
            "flake.lock" => bounded(flake_lock(file)?),
            "flake.nix" => bounded(flake_nix(file)),
            "devbox.json" => bounded(devbox_json(file)?),
            _ => bounded(devbox_lock(file)?),
        }
    }

    /// `.devbox` holds a generated flake and Nix profile links; walking it
    /// would report the environment's packages a second time.
    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        (name == ".devbox" && siblings.iter().any(|s| s == "devbox.json")).then_some(DirClaim {
            adapter: "nix",
            category: ItemCategory::Environment,
            label: ".devbox",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, _access: &mut dyn DirAccess) -> AdapterResult {
        Ok(vec![
            InventoryItem::new(ItemCategory::Environment, "nix", ".devbox", rel_path)
                .detail("Devbox-Arbeitsordner (generiert); Inhalt nicht gelesen"),
        ])
    }
}

/// A host name from untrusted text, or `None`.
pub(super) fn valid_host(host: &str) -> Option<String> {
    crate::privacy::url_host(&format!("https://{host}/")).filter(|h| h.len() > 1)
}

/// Host of a Git remote: `https://…`, `ssh://git@host/…`, `git+https://…`
/// or the scp form `git@host:owner/repo`. Credentials and paths are dropped.
pub(super) fn git_host(url: &str) -> Option<String> {
    let url = url.trim();
    let url = url.split_once('+').filter(|(scheme, _)| matches!(*scheme, "git" | "hg")).map_or(url, |(_, rest)| rest);
    if url.contains("://") {
        return crate::privacy::url_host(url).filter(|h| h.len() > 1);
    }
    let (authority, _) = url.split_once(':')?;
    valid_host(authority.rsplit('@').next()?)
}

/// A ref or revision fit to show as a version: commit ids shortened to 12
/// characters, anything else only when it looks like a plain name.
pub(super) fn short_ref(reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.len() >= 40 && reference.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(reference[..12].to_string());
    }
    let plain = !reference.is_empty()
        && reference.len() <= 100
        && reference.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '+'));
    plain.then(|| reference.to_string())
}

/// A flake reference reduced to what may be stored.
struct FlakeRef {
    name: String,
    reference: Option<String>,
    source: SourceClass,
    host: Option<String>,
    kind: &'static str,
}

/// Classifies a flake URL (`github:owner/repo/ref`, `git+https://…`,
/// `path:…`, a tarball URL or a registry id). `input` names it when the
/// reference itself has no owner/repo.
fn flake_ref(url: &str, input: &str) -> FlakeRef {
    let url = url.split('#').next().unwrap_or(url).trim();
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let param = |key: &str| query.split('&').find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='));
    let query_ref = param("ref").or_else(|| param("rev")).and_then(short_ref);
    let named =
        |kind, source, host| FlakeRef { name: input.to_string(), reference: query_ref.clone(), source, host, kind };
    let (scheme, rest) = base.split_once(':').unwrap_or(("", base));
    match scheme {
        "github" | "gitlab" | "sourcehut" => {
            let mut parts = rest.split('/');
            let (owner, repo) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let (default, kind) = match scheme {
                "github" => ("github.com", "GitHub"),
                "gitlab" => ("gitlab.com", "GitLab"),
                _ => ("git.sr.ht", "SourceHut"),
            };
            let host = param("host").and_then(valid_host).unwrap_or_else(|| default.to_string());
            let name = if owner.is_empty() || repo.is_empty() { input.to_string() } else { format!("{owner}/{repo}") };
            let reference = parts.next().and_then(short_ref).or(query_ref.clone());
            FlakeRef { name, reference, source: SourceClass::Git, host: Some(host), kind }
        }
        "git+file" | "path" => named("lokaler Pfad", SourceClass::Path, None),
        // A bare path (`./sub`, `/home/x/flake`) is no registry id.
        "" if base.starts_with(['/', '.', '~']) => named("lokaler Pfad", SourceClass::Path, None),
        s if s.starts_with("git+") || s == "git" => named("Git", SourceClass::Git, git_host(base)),
        s if s.starts_with("hg+") => named("Mercurial", SourceClass::Unknown, git_host(base)),
        "http" | "https" | "tarball+http" | "tarball+https" | "file+http" | "file+https" => {
            let address = base.trim_start_matches("tarball+").trim_start_matches("file+");
            let (source, host) = classify_url(address, PUBLIC);
            named("Archiv", source, host)
        }
        "" | "flake" => {
            let mut parts = rest.split('/');
            let id = parts.next().filter(|id| !id.is_empty()).unwrap_or(input);
            let reference = parts.next().and_then(short_ref);
            FlakeRef {
                name: id.to_string(),
                reference,
                source: SourceClass::Unknown,
                host: None,
                kind: "Registry-Verweis",
            }
        }
        _ => named("unbekannter Typ", SourceClass::Unknown, None),
    }
}

fn flake_lock(file: &FoundFile<'_>) -> AdapterResult {
    let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
    let nodes = value["nodes"].as_object().ok_or("flake.lock ohne nodes")?;
    let root_key = value["root"].as_str().unwrap_or("root");
    let root = nodes.get(root_key).ok_or("flake.lock ohne Wurzelknoten")?;
    // Direct inputs name a node; `follows` (a path) adds no node of its own.
    let mut direct: BTreeMap<&str, &str> = BTreeMap::new();
    for (input, target) in root["inputs"].as_object().into_iter().flatten() {
        if let Some(key) = target.as_str() {
            direct.entry(key).or_insert(input);
        }
    }
    let names: BTreeMap<&str, String> = nodes
        .iter()
        .take(20_000)
        .filter(|(key, node)| *key != root_key && node.is_object())
        .map(|(key, node)| {
            let fallback = direct.get(key.as_str()).copied().unwrap_or_else(|| numbered(key));
            (key.as_str(), display_name(node, fallback))
        })
        .collect();
    let version = value["version"].as_i64().unwrap_or(0);
    let mut items = vec![manifest("nix", file, format!("Lockfile v{version} · {} direkte Eingaben", direct.len()))];
    items[0].evidence.dependencies = inputs(root, nodes, root_key, &names);
    for (key, name) in &names {
        let node = &nodes[*key];
        let locked = &node["locked"];
        let (source, host, kind) = locked_source(locked);
        let reference = node["original"]["ref"]
            .as_str()
            .or_else(|| locked["ref"].as_str())
            .map(|r| r.trim_start_matches("refs/heads/").trim_start_matches("refs/tags/"))
            .and_then(short_ref);
        let rev = locked["rev"].as_str().and_then(short_ref);
        let mut detail = format!("aufgelöst laut flake.lock · {kind}");
        if let (Some(_), Some(rev)) = (&reference, &rev) {
            detail.push_str(&format!(" · Rev {rev}"));
        }
        if node["flake"].as_bool() == Some(false) {
            detail.push_str(" · kein Flake");
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "nix", name.as_str(), file.rel_path())
            .version(reference.or(rev).unwrap_or_default())
            .state(InstallState::Resolved)
            .source(source, host)
            .detail(detail);
        item.evidence.context = if direct.contains_key(key) { "direct" } else { "transitive" }.into();
        item.evidence.qualifier = (*key).to_string();
        item.evidence.dependencies = inputs(node, nodes, root_key, &names);
        items.push(item);
    }
    Ok(items)
}

/// `owner/repo` for forge inputs, otherwise the input name.
fn display_name(node: &Value, fallback: &str) -> String {
    let field = |key: &str| node["locked"][key].as_str().or_else(|| node["original"][key].as_str());
    match (field("type"), field("owner"), field("repo")) {
        (Some("github" | "gitlab" | "sourcehut"), Some(owner), Some(repo)) => format!("{owner}/{repo}"),
        _ => fallback.to_string(),
    }
}

/// Node keys of repeated inputs carry a counter (`nixpkgs_2`).
fn numbered(key: &str) -> &str {
    key.rsplit_once('_')
        .filter(|(base, n)| !base.is_empty() && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        .map_or(key, |(base, _)| base)
}

fn locked_source(locked: &Value) -> (SourceClass, Option<String>, &'static str) {
    let url = locked["url"].as_str().unwrap_or("");
    let forge = |default: &str| locked["host"].as_str().and_then(valid_host).or_else(|| Some(default.to_string()));
    match locked["type"].as_str().unwrap_or("") {
        "github" => (SourceClass::Git, forge("github.com"), "GitHub"),
        "gitlab" => (SourceClass::Git, forge("gitlab.com"), "GitLab"),
        "sourcehut" => (SourceClass::Git, forge("git.sr.ht"), "SourceHut"),
        "git" if url.starts_with("file:") || url.starts_with('/') => (SourceClass::Path, None, "lokales Git"),
        "git" => (SourceClass::Git, git_host(url), "Git"),
        "mercurial" => (SourceClass::Unknown, git_host(url), "Mercurial"),
        "path" => (SourceClass::Path, None, "lokaler Pfad"),
        "tarball" | "file" => {
            let (source, host) = classify_url(url, PUBLIC);
            (source, host, "Archiv")
        }
        "indirect" => (SourceClass::Unknown, None, "Registry-Verweis"),
        _ => (SourceClass::Unknown, None, "unbekannter Typ"),
    }
}

/// Inputs of one node as edges; a `follows` path is resolved from the root.
fn inputs(
    node: &Value,
    nodes: &Map<String, Value>,
    root: &str,
    names: &BTreeMap<&str, String>,
) -> Vec<PackageDependency> {
    node["inputs"]
        .as_object()
        .into_iter()
        .flatten()
        .take(1_000)
        .filter_map(|(input, target)| {
            let (key, kind) = match target {
                Value::String(key) => (Some(key.as_str()), "input"),
                Value::Array(path) => (follow(nodes, root, path, 0, &mut FOLLOW_STEPS.clone()), "follows"),
                _ => return None,
            };
            let name = key.and_then(|key| names.get(key)).cloned().unwrap_or_else(|| input.clone());
            Some(PackageDependency { name, requirement: String::new(), kind: kind.into(), exact: false })
        })
        .collect()
}

/// Lookups one `follows` path may take, nested paths included: without a
/// shared budget nested paths multiply to 16^depth.
const FOLLOW_STEPS: usize = 256;

/// The node a `follows` path such as `["nixpkgs"]` points to.
fn follow<'a>(
    nodes: &'a Map<String, Value>,
    root: &'a str,
    path: &'a [Value],
    depth: usize,
    steps: &mut usize,
) -> Option<&'a str> {
    if depth > REFERENCE_MAX_DEPTH || path.is_empty() {
        return None;
    }
    let mut current = root;
    for segment in path.iter().take(REFERENCE_MAX_DEPTH) {
        *steps = steps.checked_sub(1)?;
        current = match &nodes.get(current)?["inputs"][segment.as_str()?] {
            Value::String(key) => key.as_str(),
            Value::Array(inner) => follow(nodes, root, inner, depth + 1, steps)?,
            _ => return None,
        };
    }
    Some(current)
}

fn flake_nix(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut declared: Vec<(String, String)> = Vec::new();
    let (mut depth, mut inputs_level, mut current) = (0_usize, None::<usize>, None::<(String, usize)>);
    for raw in file.content.lines().take(20_000) {
        let line = without_comment(raw);
        let trimmed = line.trim_start();
        if trimmed.strip_prefix("outputs").is_some_and(|rest| rest.trim_start().starts_with('=')) {
            break;
        }
        let dotted = trimmed.strip_prefix("inputs").is_some_and(|rest| rest.trim_start().starts_with('.'));
        let in_inputs = dotted || inputs_level == Some(depth);
        if let Some(captures) = INPUT_URL.captures(line).filter(|_| in_inputs) {
            declared.push((captures[1].to_string(), captures[2].to_string()));
        } else if let Some(captures) = INPUT_SET.captures(line) {
            let (name, end) = (captures[1].to_string(), captures.get(0).map_or(0, |m| m.end()));
            if name == "inputs" && !dotted {
                inputs_level = Some(depth + 1);
            } else if in_inputs {
                if let Some(url) = URL.captures(&line[end..]) {
                    declared.push((name.clone(), url[1].to_string()));
                }
                current = Some((name, depth + 1));
            }
        } else if let Some((name, level)) = &current
            && depth == *level
            && let Some(url) = URL.captures(line)
        {
            declared.push((name.clone(), url[1].to_string()));
        }
        depth = braces(depth, line);
        if current.as_ref().is_some_and(|(_, level)| depth < *level) {
            current = None;
        }
        if inputs_level.is_some_and(|level| depth < level) {
            inputs_level = None;
        }
    }
    declared.retain(|(name, _)| name != "self" && name != "inputs");
    let mut items = vec![manifest(
        "nix",
        file,
        format!("Flake (nicht ausgewertet) · {} Eingaben mit wörtlicher URL", declared.len()),
    )];
    for (input, url) in declared.into_iter().take(1_000) {
        let reference = flake_ref(&url, &input);
        let mut item = InventoryItem::new(ItemCategory::Package, "nix", reference.name, file.rel_path())
            .version(reference.reference.unwrap_or_default())
            .state(InstallState::Declared)
            .source(reference.source, reference.host)
            .detail(format!("Eingabe {input} · {}", reference.kind));
        item.evidence.context = "input".into();
        item.evidence.qualifier = input;
        items.push(item);
    }
    items
}

/// The line up to a `#` comment outside a string.
fn without_comment(line: &str) -> &str {
    let mut quoted = false;
    for (index, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..index],
            _ => {}
        }
    }
    line
}

/// Brace depth after `line`; braces inside strings do not count.
fn braces(depth: usize, line: &str) -> usize {
    let mut quoted = false;
    line.chars().fold(depth, |depth, ch| match ch {
        '"' => {
            quoted = !quoted;
            depth
        }
        '{' if !quoted => depth + 1,
        '}' if !quoted => depth.saturating_sub(1),
        _ => depth,
    })
}

/// A Devbox flake package (`github:NixOS/nixpkgs#hello`, `path:../x#tool`):
/// named by its output attribute, else by the flake, never by path or URL.
fn flake_package(spec: &str) -> (String, FlakeRef, String) {
    let attribute = spec.split_once('#').map(|(_, attribute)| attribute).filter(|attribute| {
        !attribute.is_empty()
            && attribute.len() <= 100
            && attribute.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    });
    let reference = flake_ref(spec, attribute.unwrap_or("Flake"));
    let name = attribute.map_or_else(|| reference.name.clone(), str::to_string);
    let mut detail = format!("Flake-Paket · {}", reference.kind);
    if reference.name != name {
        detail.push_str(&format!(" {}", reference.name));
    }
    (name, reference, detail)
}

fn devbox_json(file: &FoundFile<'_>) -> AdapterResult {
    let value = json::parse_jsonc(file.content, 32).map_err(|e| e.to_string())?;
    let packages: Vec<(String, Option<String>)> = match &value["packages"] {
        Value::Array(list) => list
            .iter()
            .filter_map(Value::as_str)
            .map(|spec| match spec.split_once('@').filter(|_| !spec.contains(':')) {
                Some((name, version)) => (name.to_string(), Some(version.to_string())),
                None => (spec.to_string(), None),
            })
            .collect(),
        Value::Object(map) => map
            .iter()
            .map(|(name, spec)| {
                let version = spec.as_str().or_else(|| spec["version"].as_str()).map(str::to_string);
                (name.clone(), version)
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut detail = format!("Devbox-Umgebung · {} Pakete", packages.len());
    if value["shell"]["scripts"].as_object().is_some_and(|s| !s.is_empty()) {
        detail.push_str(" · Skripte vorhanden (nicht ausgeführt)");
    }
    let mut items = vec![manifest("nix", file, detail)];
    for (spec, version) in packages.into_iter().take(5_000) {
        let item = if spec.contains(':') {
            let (name, reference, detail) = flake_package(&spec);
            InventoryItem::new(ItemCategory::Package, "nix", name, file.rel_path())
                .version(reference.reference.unwrap_or_default())
                .source(reference.source, reference.host)
                .detail(detail)
        } else {
            let detail = version.as_deref().map_or("ohne Versionsangabe".into(), |v| format!("Spezifikation {v}"));
            InventoryItem::new(ItemCategory::Package, "nix", spec, file.rel_path()).detail(detail)
        };
        let mut item = item.state(InstallState::Declared);
        item.evidence.context = "devbox".into();
        items.push(item);
    }
    Ok(items)
}

fn devbox_lock(file: &FoundFile<'_>) -> AdapterResult {
    let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
    let packages = value["packages"].as_object().ok_or("devbox.lock ohne packages")?;
    let version = value["lockfile_version"].as_str().unwrap_or("?");
    let mut items = vec![manifest("nix", file, format!("Lockfile v{version} · {} Einträge", packages.len()))];
    for (key, entry) in packages.iter().take(5_000) {
        let flake = key.contains(':');
        let (name, spec) = key.split_once('@').filter(|_| !flake).unwrap_or((key, "latest"));
        let resolved = flake_ref(entry["resolved"].as_str().unwrap_or(""), name);
        let (name, version, detail) = if flake {
            let (name, _, detail) = flake_package(key);
            (name, resolved.reference.clone(), format!("aufgelöst laut devbox.lock · {detail}"))
        } else {
            let mut detail = format!("aufgelöst laut devbox.lock · Spezifikation {spec}");
            if resolved.source == SourceClass::Git {
                detail.push_str(&format!(" · aus {}", resolved.name));
            }
            (name.to_string(), entry["version"].as_str().map(str::to_string), detail)
        };
        let mut item = InventoryItem::new(ItemCategory::Package, "nix", name.as_str(), file.rel_path())
            .version(version.unwrap_or_default())
            .state(InstallState::Resolved)
            .source(resolved.source, resolved.host)
            .detail(detail);
        item.evidence.context = "devbox".into();
        // Never the key itself: a flake key can hold a local path or URL.
        item.evidence.qualifier = if flake { name } else { key.clone() };
        items.push(item);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    const LOCK: &str = r#"{
  "nodes": {
    "flake-utils": {
      "inputs": { "systems": "systems" },
      "locked": {
        "lastModified": 1710146030,
        "narHash": "sha256-SZ5L6eA7HJ/nmkzGG7/ISclqe6oZdOZTNoesiInkXPQ=",
        "owner": "numtide",
        "repo": "flake-utils",
        "rev": "b1d9ab70662946ef0850d488da1c9019f3a9752a",
        "type": "github"
      },
      "original": { "owner": "numtide", "repo": "flake-utils", "type": "github" }
    },
    "home-manager": {
      "inputs": { "nixpkgs": ["nixpkgs"] },
      "locked": {
        "lastModified": 1720042825,
        "narHash": "sha256-A0vrUB6x82/jvf17qPCpxaM+ulJnD8YZwH9Ci0BsAzE=",
        "owner": "nix-community",
        "repo": "home-manager",
        "rev": "e1391fb22e18a36f57e6999c7a9f966dc80ac073",
        "type": "github"
      },
      "original": { "owner": "nix-community", "ref": "release-24.05", "repo": "home-manager", "type": "github" }
    },
    "nixpkgs": {
      "locked": {
        "lastModified": 1720535198,
        "narHash": "sha256-zwVvxrdIzralnSbcpghA92tWu2DV2lwv89xZc8MTrbg=",
        "owner": "NixOS",
        "repo": "nixpkgs",
        "rev": "205fd4226592cc83fd4c0885a3e4c9c400efabb5",
        "type": "github"
      },
      "original": { "owner": "NixOS", "ref": "nixos-24.05", "repo": "nixpkgs", "type": "github" }
    },
    "private": {
      "flake": false,
      "locked": {
        "lastModified": 1719000000,
        "narHash": "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "ref": "refs/heads/main",
        "rev": "0123456789abcdef0123456789abcdef01234567",
        "revCount": 42,
        "type": "git",
        "url": "https://deploy:s3cr3t-token@git.corp.example/infra/private.git?dir=nix"
      },
      "original": { "type": "git", "url": "https://deploy:s3cr3t-token@git.corp.example/infra/private.git?dir=nix" }
    },
    "local": {
      "locked": { "lastModified": 1, "narHash": "sha256-x", "path": "/home/alice/flakes/local", "type": "path" },
      "original": { "path": "/home/alice/flakes/local", "type": "path" }
    },
    "root": {
      "inputs": {
        "flake-utils": "flake-utils",
        "home-manager": "home-manager",
        "nixpkgs": "nixpkgs",
        "private": "private",
        "local": "local",
        "utils": ["flake-utils"]
      }
    },
    "systems": {
      "locked": {
        "lastModified": 1681028828,
        "narHash": "sha256-Vy1rq5AaRuLzOxct8nz4T6wlgyUR7zLU309k9mBC768=",
        "owner": "nix-systems",
        "repo": "default",
        "rev": "da67096a3b9bf56a91d16901293e51ba5b49a27e",
        "type": "github"
      },
      "original": { "owner": "nix-systems", "repo": "default", "type": "github" }
    }
  },
  "root": "root",
  "version": 7
}"#;

    #[test]
    fn flake_lock_separates_direct_and_transitive_inputs() {
        let items = Nix.parse_file(&file("flake.lock", LOCK)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v7 · 5 direkte Eingaben");
        assert_eq!(items.len(), 7, "follows adds no node: {items:#?}");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let nixpkgs = find("NixOS/nixpkgs");
        assert_eq!(nixpkgs.version.as_deref(), Some("nixos-24.05"));
        assert_eq!((nixpkgs.source, nixpkgs.source_host.as_deref()), (SourceClass::Git, Some("github.com")));
        assert_eq!(
            (nixpkgs.evidence.context.as_str(), nixpkgs.install_state),
            ("direct", Some(InstallState::Resolved))
        );
        assert!(nixpkgs.detail.ends_with("Rev 205fd4226592"), "{}", nixpkgs.detail);
        let utils = find("numtide/flake-utils");
        assert_eq!(utils.version.as_deref(), Some("b1d9ab706629"), "12-character revision without a ref");
        assert_eq!(utils.evidence.dependencies[0].name, "nix-systems/default");
        let systems = find("nix-systems/default");
        assert_eq!(systems.evidence.context, "transitive");
        let home = find("nix-community/home-manager");
        assert_eq!(home.evidence.dependencies[0].name, "NixOS/nixpkgs");
        assert_eq!(home.evidence.dependencies[0].kind, "follows");
        let private = find("private");
        assert_eq!((private.source, private.source_host.as_deref()), (SourceClass::Git, Some("git.corp.example")));
        assert_eq!(private.version.as_deref(), Some("main"));
        assert!(private.detail.contains("kein Flake"));
        let local = find("local");
        assert_eq!((local.source, local.source_host.as_deref()), (SourceClass::Path, None));
        let roots: Vec<&str> = items[0].evidence.dependencies.iter().map(|d| d.name.as_str()).collect();
        assert!(roots.contains(&"numtide/flake-utils") && roots.contains(&"NixOS/nixpkgs"));
        let text = format!("{items:?}");
        for secret in ["s3cr3t-token", "deploy", "infra/private", "/home/alice", "dir=nix", "narHash", "sha256-"] {
            assert!(!text.contains(secret), "{secret} must not be stored");
        }
    }

    #[test]
    fn broken_or_shapeless_flake_lock_is_an_error() {
        assert!(Nix.parse_file(&file("flake.lock", "{\"nodes\": {")).is_err());
        assert!(Nix.parse_file(&file("flake.lock", "{\"version\": 7}")).is_err());
    }

    #[test]
    fn flake_nix_reads_literal_input_urls_only() {
        let nix = r#"{
  description = "A very basic flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05"; # stable
    flake-utils.url = "github:numtide/flake-utils";
    home-manager = {
      url = "github:nix-community/home-manager?ref=release-24.05";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    corp = { url = "git+ssh://git@git.corp.example/team/corp-flake.git?ref=main"; flake = false; };
    registry.url = "nixpkgs/nixos-unstable";
    local.url = "path:./nix/local";
    outputs-helper.url = "github:example/outputs-helper";
    # commented.url = "github:not/declared";
  };
  inputs.extra.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1.*.tar.gz";

  outputs = { self, nixpkgs, ... }: {
    packages.x86_64-linux.src.url = "github:not/an-input";
  };
}
"#;
        let items = Nix.parse_file(&file("flake.nix", nix)).unwrap();
        assert_eq!(items[0].detail, "Flake (nicht ausgewertet) · 8 Eingaben mit wörtlicher URL");
        assert!(items.iter().any(|i| i.name == "example/outputs-helper"), "an input named outputs-… is no outputs");
        let find =
            |input: &str| items.iter().find(|i| i.evidence.qualifier == input).unwrap_or_else(|| panic!("{input}"));
        let nixpkgs = find("nixpkgs");
        assert_eq!((nixpkgs.name.as_str(), nixpkgs.version.as_deref()), ("NixOS/nixpkgs", Some("nixos-24.05")));
        assert_eq!(nixpkgs.install_state, Some(InstallState::Declared));
        assert_eq!(find("home-manager").version.as_deref(), Some("release-24.05"));
        let corp = find("corp");
        assert_eq!((corp.source, corp.source_host.as_deref()), (SourceClass::Git, Some("git.corp.example")));
        assert_eq!(corp.version.as_deref(), Some("main"));
        let registry = find("registry");
        assert_eq!((registry.name.as_str(), registry.source), ("nixpkgs", SourceClass::Unknown));
        assert_eq!(find("local").source, SourceClass::Path);
        let extra = find("extra");
        assert_eq!((extra.source, extra.source_host.as_deref()), (SourceClass::PublicRegistry, Some("flakehub.com")));
        assert!(!items.iter().any(|i| i.name.contains("not/")), "comments and outputs are not inputs");
        assert!(!format!("{items:?}").contains("team/corp-flake"));
    }

    #[test]
    fn devbox_manifest_and_lockfile() {
        let json = r#"{
  // devbox.json may carry comments
  "$schema": "https://raw.githubusercontent.com/jetify-com/devbox/0.13.0/.schema/devbox.schema.json",
  "packages": [
    "python@3.11",
    "nodejs@latest",
    "github:numtide/flake-utils",
    "github:NixOS/nixpkgs/nixos-24.05#hello",
    "path:/home/alice/flakes/tools#mytool",
    "git+https://ci:hunter2@git.corp.example/nix/tools.git",
    "ripgrep"
  ],
  "shell": { "init_hook": ["echo hi"], "scripts": { "test": "pytest" } },
}"#;
        let items = Nix.parse_file(&file("devbox.json", json)).unwrap();
        assert_eq!(items[0].detail, "Devbox-Umgebung · 7 Pakete · Skripte vorhanden (nicht ausgeführt)");
        let python = items.iter().find(|i| i.name == "python").unwrap();
        assert_eq!((python.detail.as_str(), python.evidence.context.as_str()), ("Spezifikation 3.11", "devbox"));
        assert_eq!(items.iter().find(|i| i.name == "numtide/flake-utils").unwrap().source, SourceClass::Git);
        let hello = items.iter().find(|i| i.name == "hello").unwrap();
        assert_eq!(
            (hello.detail.as_str(), hello.version.as_deref()),
            ("Flake-Paket · GitHub NixOS/nixpkgs", Some("nixos-24.05"))
        );
        assert_eq!(items.iter().find(|i| i.name == "mytool").unwrap().source, SourceClass::Path);
        let corp = items.iter().find(|i| i.name == "Flake").unwrap();
        assert_eq!((corp.source, corp.source_host.as_deref()), (SourceClass::Git, Some("git.corp.example")));
        assert!(items.iter().any(|i| i.name == "ripgrep" && i.detail == "ohne Versionsangabe"));
        let text = format!("{items:?}");
        for secret in ["/home/alice", "hunter2", "nix/tools", "flakes/tools"] {
            assert!(!text.contains(secret), "{secret} must not be stored");
        }
        let map = r#"{"packages": {"go": "latest", "nodejs": {"version": "20", "platforms": ["x86_64-linux"]}}}"#;
        let items = Nix.parse_file(&file("devbox.json", map)).unwrap();
        assert!(items.iter().any(|i| i.name == "nodejs" && i.detail == "Spezifikation 20"));

        let lock = r#"{
  "lockfile_version": "1",
  "packages": {
    "github:NixOS/nixpkgs/nixpkgs-unstable": {
      "resolved": "github:NixOS/nixpkgs/02032da4af073d0f6110540c8677f16d4be0117f?lastModified=1741037377&narHash=sha256-Svtv"
    },
    "go@latest": {
      "last_modified": "2025-02-12T00:10:52Z",
      "resolved": "github:NixOS/nixpkgs/83a2581c81ff5b06f7c1a4e7cc736a455dfcf7b4#go_1_24",
      "source": "devbox-search",
      "version": "1.24.0",
      "systems": { "x86_64-linux": { "store_path": "/nix/store/vh5d5bj1sljdhdypy80x1ydx2jx6rv2q-go-1.24.0" } }
    },
    "path:/home/alice/flakes/tools#mytool": { "resolved": "path:/home/alice/flakes/tools#mytool" }
  }
}"#;
        let items = Nix.parse_file(&file("devbox.lock", lock)).unwrap();
        let go = items.iter().find(|i| i.name == "go").unwrap();
        assert_eq!((go.version.as_deref(), go.install_state), (Some("1.24.0"), Some(InstallState::Resolved)));
        assert!(
            go.detail.contains("Spezifikation latest") && go.detail.ends_with("aus NixOS/nixpkgs"),
            "{}",
            go.detail
        );
        let nixpkgs = items.iter().find(|i| i.name == "NixOS/nixpkgs").unwrap();
        assert_eq!(nixpkgs.version.as_deref(), Some("02032da4af07"));
        let tool = items.iter().find(|i| i.name == "mytool").unwrap();
        assert_eq!((tool.source, tool.evidence.qualifier.as_str()), (SourceClass::Path, "mytool"));
        let text = format!("{items:?}");
        assert!(!text.contains("/nix/store") && !text.contains("narHash") && !text.contains("/home/alice"));
        assert!(Nix.parse_file(&file("devbox.lock", "{\"packages\": [")).is_err());
        assert!(Nix.claim_dir(".devbox", &["README.md".into()], &[]).is_none());
        let claim = Nix.claim_dir(".devbox", &["devbox.json".into()], &[]).unwrap();
        let items = Nix.inspect_dir(claim, ".devbox", &mut crate::adapters::testing::FakeDir::default()).unwrap();
        assert_eq!((items.len(), items[0].category), (1, ItemCategory::Environment));
    }

    #[test]
    fn git_hosts_and_refs_drop_credentials_and_paths() {
        assert_eq!(git_host("git@github.com:owner/repo.git").as_deref(), Some("github.com"));
        assert_eq!(git_host("git+ssh://user:pw@git.corp.example:2222/x/y").as_deref(), Some("git.corp.example"));
        assert_eq!(git_host("https://gitlab.com/group/project").as_deref(), Some("gitlab.com"));
        assert_eq!(git_host("C:\\repos\\x"), None);
        assert_eq!(git_host("../relative"), None);
        assert_eq!(short_ref("bed8e32d45c7dc98afb783b409edac98854d41a7").as_deref(), Some("bed8e32d45c7"));
        assert_eq!(short_ref("v1.2.0").as_deref(), Some("v1.2.0"));
        assert_eq!(short_ref("main; rm -rf /"), None);
    }

    #[test]
    fn nested_follows_stay_cheap() {
        // Each level follows the next one 16 times: 16^15 lookups unbounded.
        let mut inputs: Vec<String> = (0..15)
            .map(|level| {
                let path = vec![format!("\"a{}\"", level + 1); 16].join(",");
                format!("\"a{level}\": [{path}]")
            })
            .collect();
        inputs.push("\"a15\": \"root\"".into());
        let lock =
            format!(r#"{{"nodes": {{"root": {{"inputs": {{{}}}}}}}, "root": "root", "version": 7}}"#, inputs.join(","));
        let started = std::time::Instant::now();
        assert!(Nix.parse_file(&file("flake.lock", &lock)).is_ok());
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
        let local = flake_ref("./sub", "sub");
        assert_eq!((local.source, local.kind), (SourceClass::Path, "lokaler Pfad"));
    }
}
