//! Bazel modules (Bzlmod): MODULE.bazel (declared `bazel_dep`, overrides
//! and `http_archive` / `git_repository` repository rules, matched as text)
//! and MODULE.bazel.lock (resolved versions: the registry files of Bazel
//! 7.2+, the module graph of older lockfiles). Starlark is never evaluated
//! and Bazel is never started; WORKSPACE files are recognised only.
//! `bazel-*` output links are never followed.
use super::nix::{git_host, short_ref};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::json;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

pub struct Bazel;

const PUBLIC: &[&str] = &["bcr.bazel.build"];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "bazel",
    name: "Bazel (Bzlmod)",
    markers: &["MODULE.bazel", "MODULE.bazel.lock", "WORKSPACE", "WORKSPACE.bazel"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "MODULE.bazel (bazel_dep, *_override, http_archive, git_repository als Text)",
        "MODULE.bazel.lock (registryFileHashes; moduleDepGraph bis Bazel 7.1)",
        "WORKSPACE (nur erkannt)",
    ],
    limits: "Kein bazel-Aufruf; Starlark wird nicht ausgewertet – Werte aus Variablen, include(), Modulerweiterungen und umbenannte Repository-Regeln bleiben offen; WORKSPACE-Dateien werden nicht ausgewertet; das Lockfile kann neben den ausgewählten weitere Versionen nennen.",
};

/// A top-level call that names the module, a dependency or an override.
static CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?m)^[ \t]*(bazel_dep|single_version_override|multiple_version_override|git_override",
        r"|archive_override|local_path_override|module|include|http_archive|http_file|git_repository)[ \t]*\("
    ))
    .expect("valid regex")
});
/// `key = value` with a string, a list, a boolean, a number or a name.
static ARGUMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([A-Za-z_]\w*)\s*=\s*("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|\[[^\]]*\]|[A-Za-z_][\w.]*|-?\d+)"#)
        .expect("valid regex")
});
static STRING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""((?:[^"\\\n]|\\.)*)"|'((?:[^'\\\n]|\\.)*)'"#).expect("valid regex"));

impl Adapter for Bazel {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name.to_ascii_lowercase().as_str() {
            "module.bazel" => bounded(module_file(file)),
            "module.bazel.lock" => bounded(lockfile(file)?),
            _ => bounded(workspace(file)),
        }
    }
}

enum Arg {
    Text(String),
    List(Vec<String>),
    Flag(bool),
    /// A name or number: a variable or constant, no literal string.
    Other,
}

/// The text between a call's parentheses; strings and comments may hold
/// brackets. `None` when the call does not close within 64 KiB.
fn arguments(text: &str) -> Option<&str> {
    // Every search stays inside the window: a file of unclosed calls would
    // otherwise rescan its whole tail once per call.
    let bytes = &text.as_bytes()[..text.len().min(64 * 1024)];
    let (mut depth, mut index) = (1_usize, 0);
    while index < bytes.len() {
        match bytes[index] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[..index]);
                }
            }
            b'#' => index += bytes[index..].iter().position(|&b| b == b'\n')?,
            quote @ (b'"' | b'\'') => {
                let triple = [quote; 3];
                if bytes[index..].starts_with(&triple) {
                    index += 3 + bytes.get(index + 3..)?.windows(3).position(|w| w == triple)? + 2;
                } else {
                    index += 1;
                    while index < bytes.len() && bytes[index] != quote && bytes[index] != b'\n' {
                        index += if bytes[index] == b'\\' { 2 } else { 1 };
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// Argument text without `#` comments outside strings.
fn uncommented(text: &str) -> String {
    let lines = text.lines().map(|line| {
        let mut quote = None;
        for (index, ch) in line.char_indices() {
            match (quote, ch) {
                (None, '"' | '\'') => quote = Some(ch),
                (Some(open), _) if ch == open => quote = None,
                (None, '#') => return &line[..index],
                _ => {}
            }
        }
        line
    });
    lines.collect::<Vec<_>>().join("\n")
}

fn kwargs(arguments: &str) -> Vec<(String, Arg)> {
    ARGUMENT
        .captures_iter(arguments)
        .take(64)
        .map(|c| {
            let raw = &c[2];
            let value = match raw.as_bytes()[0] {
                b'"' | b'\'' => Arg::Text(raw[1..raw.len() - 1].to_string()),
                b'[' => Arg::List(
                    STRING
                        .captures_iter(raw)
                        .take(64)
                        .filter_map(|s| s.get(1).or_else(|| s.get(2)))
                        .map(|m| m.as_str().to_string())
                        .collect(),
                ),
                _ if raw == "True" => Arg::Flag(true),
                _ if raw == "False" => Arg::Flag(false),
                _ => Arg::Other,
            };
            (c[1].to_string(), value)
        })
        .collect()
}

fn arg_text<'a>(args: &'a [(String, Arg)], key: &str) -> Option<&'a str> {
    match args.iter().find(|(k, _)| k == key) {
        Some((_, Arg::Text(text))) => Some(text),
        _ => None,
    }
}

fn arg_list<'a>(args: &'a [(String, Arg)], key: &str) -> &'a [String] {
    match args.iter().find(|(k, _)| k == key) {
        Some((_, Arg::List(list))) => list,
        _ => &[],
    }
}

fn declared(file: &FoundFile<'_>, name: &str, version: Option<&str>, detail: String) -> InventoryItem {
    InventoryItem::new(ItemCategory::Package, "bazel", name, file.rel_path())
        .version(version.unwrap_or_default())
        .state(InstallState::Declared)
        .detail(detail)
}

fn module_file(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let (mut module, mut includes) = (None, 0);
    let mut items = Vec::new();
    for call in CALL.captures_iter(file.content).take(10_000) {
        if items.len() > crate::limits::ITEMS_PER_FILE {
            break;
        }
        let start = call.get(0).map_or(0, |m| m.end());
        let Some(inner) = arguments(&file.content[start..]) else { continue };
        let args = kwargs(&uncommented(inner));
        let text = |key: &str| arg_text(&args, key);
        let list = |key: &str| arg_list(&args, key);
        let function = &call[1];
        match function {
            "module" => {
                module = Some((text("name").unwrap_or("?").to_string(), text("version").map(str::to_string)));
                continue;
            }
            "include" => {
                includes += 1;
                continue;
            }
            _ => {}
        }
        let overrides = function.ends_with("_override");
        let Some(name) = text(if overrides { "module_name" } else { "name" }).filter(|n| !n.is_empty()) else {
            continue;
        };
        let mut item = match function {
            "bazel_dep" => {
                let dev = args.iter().any(|(k, v)| k == "dev_dependency" && matches!(v, Arg::Flag(true)));
                let version = text("version").filter(|v| !v.is_empty());
                let variable = text("version").is_none() && args.iter().any(|(k, _)| k == "version");
                let mut detail = match version {
                    Some(version) => format!("Mindestversion {version}"),
                    None if variable => "Version aus Variable".into(),
                    None => "Version per Override".into(),
                };
                if dev {
                    detail.push_str(" · dev");
                }
                if let Some(repo) = text("repo_name").filter(|r| !r.is_empty() && *r != name) {
                    detail.push_str(&format!(" · Repo-Name {repo}"));
                }
                let mut item = declared(file, name, version, detail);
                item.evidence.context = if dev { "dev" } else { "" }.into();
                items.push(item);
                continue;
            }
            "single_version_override" => {
                let version = text("version").filter(|v| !v.is_empty());
                let mut detail = format!("Override · Version {}", version.unwrap_or("unverändert"));
                if !list("patches").is_empty() {
                    detail.push_str(&format!(" · {} Patches", list("patches").len()));
                }
                let (source, host) = text("registry").map_or((SourceClass::Unknown, None), |r| classify_url(r, PUBLIC));
                declared(file, name, version, detail).source(source, host)
            }
            "multiple_version_override" => {
                let (source, host) = text("registry").map_or((SourceClass::Unknown, None), |r| classify_url(r, PUBLIC));
                for version in list("versions") {
                    let mut item = declared(file, name, Some(version), "Override · mehrere Versionen erlaubt".into())
                        .source(source, host.clone());
                    item.evidence.context = "override".into();
                    item.evidence.qualifier = version.clone();
                    items.push(item);
                }
                continue;
            }
            "git_override" | "git_repository" => {
                let pin = [("tag", "Tag"), ("commit", "Commit"), ("branch", "Branch")]
                    .into_iter()
                    .find_map(|(key, label)| Some((label, text(key).and_then(short_ref)?)));
                let mut detail = String::from(if overrides { "Git-Override" } else { "Repository-Regel (Git)" });
                if let Some((label, reference)) = &pin {
                    detail.push_str(&format!(" · {label} {reference}"));
                }
                let host = text("remote").and_then(git_host);
                declared(file, name, pin.as_ref().map(|(_, r)| r.as_str()), detail).source(SourceClass::Git, host)
            }
            "archive_override" | "http_archive" | "http_file" => {
                let url = text("url").or_else(|| list("urls").first().map(String::as_str));
                let (source, host) = url.map_or((SourceClass::Unknown, None), |u| classify_url(u, PUBLIC));
                let detail = if overrides { "Archiv-Override" } else { "Repository-Regel (Archiv)" };
                declared(file, name, None, detail.into()).source(source, host)
            }
            _ => declared(file, name, None, "Override · lokaler Pfad".into()).source(SourceClass::Path, None),
        };
        item.evidence.context = if overrides { "override" } else { "repo_rule" }.into();
        items.push(item);
    }
    let mut detail = match module {
        Some((name, Some(version))) => format!("Modul {name} {version}"),
        Some((name, None)) => format!("Modul {name}"),
        None => "MODULE.bazel ohne module()".to_string(),
    };
    detail.push_str(" (nicht ausgewertet)");
    if includes > 0 {
        detail.push_str(&format!(" · {includes} include() nicht gefolgt"));
    }
    items.insert(0, manifest("bazel", file, detail));
    items
}

fn lockfile(file: &FoundFile<'_>) -> AdapterResult {
    let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("MODULE.bazel.lock ist kein JSON-Objekt".into());
    }
    let yanked = &value["selectedYankedVersions"];
    let (mut items, mut considered, mut roots) = (Vec::new(), 0, Vec::new());
    if let Some(hashes) = value["registryFileHashes"].as_object() {
        // MODULE.bazel files of every version considered during resolution;
        // source.json only of the versions whose sources were looked up.
        for url in hashes.keys().take(200_000) {
            if url.ends_with("/MODULE.bazel") {
                considered += 1;
                continue;
            }
            let Some((base, name, version)) = url.strip_suffix("/source.json").and_then(registry_module) else {
                continue;
            };
            let (source, host) = classify_url(base, PUBLIC);
            items.push(resolved(file, name, version, source, host, yanked));
        }
    } else if let Some(graph) = value["moduleDepGraph"].as_object() {
        let registries: Vec<&str> =
            value["flags"]["cmdRegistries"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let (source, host) = match registries.as_slice() {
            [only] => classify_url(only, PUBLIC),
            _ => (SourceClass::Unknown, None),
        };
        roots = graph.get("<root>").map(|root| edges(&root["deps"])).unwrap_or_default();
        for (key, module) in graph.iter().take(20_000) {
            // `<root>` is the project, `name@_` a built-in or overridden module.
            if key == "<root>" || key.ends_with("@_") {
                continue;
            }
            let (Some(name), Some(version)) = (module["name"].as_str(), module["version"].as_str()) else { continue };
            let mut item = resolved(file, name, version, source, host.clone(), yanked);
            item.evidence.dependencies = edges(&module["deps"]);
            items.push(item);
        }
    }
    let version = value["lockFileVersion"].as_i64().unwrap_or(0);
    let mut detail = format!("Lockfile v{version} · {} Module aufgelöst", items.len());
    if considered > 0 {
        detail.push_str(&format!(" · {considered} Moduldateien betrachtet"));
    }
    let mut lock = manifest("bazel", file, detail);
    lock.evidence.dependencies = roots;
    items.insert(0, lock);
    Ok(items)
}

/// (`registry base`, name, version) of `…/modules/<name>/<version>`.
fn registry_module(url: &str) -> Option<(&str, &str, &str)> {
    let mut parts = url.rsplitn(4, '/');
    let (version, name, modules, base) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    (modules == "modules" && !name.is_empty() && !version.is_empty()).then_some((base, name, version))
}

fn resolved(
    file: &FoundFile<'_>,
    name: &str,
    version: &str,
    source: SourceClass,
    host: Option<String>,
    yanked: &Value,
) -> InventoryItem {
    let reason = yanked[format!("{name}@{version}")].as_str();
    let mut detail = String::from("aufgelöst laut MODULE.bazel.lock");
    if reason.is_some() {
        detail.push_str(" · zurückgezogen (yanked)");
    }
    let mut item = InventoryItem::new(ItemCategory::Package, "bazel", name, file.rel_path())
        .version(version)
        .state(InstallState::Resolved)
        .source(source, host)
        .detail(detail);
    item.evidence.qualifier = format!("{version}|{}", item.source_host.as_deref().unwrap_or(""));
    item.evidence.deprecated = reason.map(|reason| format!("Zurückgezogen: {reason}"));
    item
}

/// `deps` of the old module graph: repository name → `module@version`.
fn edges(deps: &Value) -> Vec<PackageDependency> {
    let targets = deps.as_object().into_iter().flatten().take(1_000).filter_map(|(_, target)| target.as_str());
    targets
        .filter_map(|target| {
            let (name, version) = target.split_once('@')?;
            let exact = !version.is_empty() && version != "_";
            let requirement = if exact { version.to_string() } else { String::new() };
            Some(PackageDependency { name: name.to_string(), requirement, kind: "bazel_dep".into(), exact })
        })
        .collect()
}

/// A WORKSPACE is Starlark: empty, comments or calls such as `http_archive(`.
fn workspace(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let (mut code, mut calls, mut repositories) = (0, 0, 0);
    for line in file.content.lines().take(100_000) {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        code += 1;
        let Some((function, _)) = line.split_once('(') else { continue };
        let repository = function.ends_with("_repository")
            || matches!(function, "http_archive" | "http_file" | "http_jar" | "maven_install");
        let setup = matches!(function, "load" | "workspace" | "register_toolchains" | "bind")
            || ["_dependencies", "_repositories", "_toolchains", "_deps"].iter().any(|s| function.ends_with(s));
        if function.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && (repository || setup) {
            calls += 1;
            repositories += usize::from(repository);
        }
    }
    if code > 0 && calls == 0 {
        return Vec::new();
    }
    let mut detail = String::from("WORKSPACE (nicht ausgewertet)");
    if repositories > 0 {
        detail.push_str(&format!(" · {repositories} Repository-Regeln"));
    }
    vec![manifest("bazel", file, detail)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    const MODULE: &str = r#""""The my_project module (docstring with bazel_dep(name = "fake"))."""

module(
    name = "my_project",
    version = "1.2.0",
    compatibility_level = 1,
)

bazel_dep(name = "rules_go", version = "0.41.0", repo_name = "io_bazel_rules_go")
bazel_dep(name = "gazelle", version = "0.32.0")
bazel_dep(
    name = "protobuf",
    # version = "3.19.0",
    version = "21.7",
    repo_name = "com_google_protobuf",  # (old name)
)
bazel_dep(name = "rules_testing", version = "0.6.0", dev_dependency = True)
bazel_dep(name = "bazel_skylib", version = SKYLIB_VERSION)
bazel_dep(name = "rules_foo", version = "")
bazel_dep(name = "rules_bar")
bazel_dep(name = "local_lib")

single_version_override(
    module_name = "bazel_skylib",
    version = "1.4.2",
    registry = "https://bcr.bazel.build",
    patches = ["//third_party:skylib.patch"],
)

multiple_version_override(
    module_name = "rules_cc",
    versions = ["0.0.1", "0.0.4"],
)

git_override(
    module_name = "rules_foo",
    remote = "https://ci:hunter2@git.corp.example/bazel/rules_foo.git",
    commit = "6a2c2e22849b3e6b33d5ea9aa72222d4803a986a",
)

archive_override(
    module_name = "rules_bar",
    urls = ["https://github.com/bar/rules_bar/archive/v1.0.tar.gz"],
    integrity = "sha256-0000000000000000000000000000000000000000000=",
    strip_prefix = "rules_bar-1.0",
)

local_path_override(
    module_name = "local_lib",
    path = "../local_lib",
)

include("//:deps.MODULE.bazel")

http_archive = use_repo_rule("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")
git_repository = use_repo_rule("@bazel_tools//tools/build_defs/repo:git.bzl", "git_repository")

http_archive(
    name = "zlib",
    urls = ["https://zlib.net/zlib-1.3.1.tar.gz"],
    sha256 = "9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23",
)

git_repository(
    name = "corp_tools",
    remote = "git@git.corp.example:tools/corp_tools.git",
    tag = "v2.3.0",
)

go_deps = use_extension("@gazelle//:extensions.bzl", "go_deps")
go_deps.from_file(go_mod = "//:go.mod")
use_repo(go_deps, "com_github_google_uuid")
"#;

    #[test]
    fn module_declares_dependencies_and_overrides() {
        let items = Bazel.parse_file(&file("MODULE.bazel", MODULE)).unwrap();
        assert_eq!(items[0].detail, "Modul my_project 1.2.0 (nicht ausgewertet) · 1 include() nicht gefolgt");
        assert_eq!(items.len(), 17, "{items:#?}");
        let find = |name: &str, context: &str| {
            items
                .iter()
                .find(|i| i.name == name && i.evidence.context == context)
                .unwrap_or_else(|| panic!("{name} {context}"))
        };
        let go = find("rules_go", "");
        assert_eq!((go.version.as_deref(), go.install_state), (Some("0.41.0"), Some(InstallState::Declared)));
        assert_eq!(go.detail, "Mindestversion 0.41.0 · Repo-Name io_bazel_rules_go");
        assert_eq!(find("protobuf", "").version.as_deref(), Some("21.7"), "commented arguments are ignored");
        assert_eq!(find("rules_testing", "dev").detail, "Mindestversion 0.6.0 · dev");
        assert_eq!(find("bazel_skylib", "").detail, "Version aus Variable");
        assert_eq!(find("rules_foo", "").detail, "Version per Override");
        let skylib = find("bazel_skylib", "override");
        assert_eq!(skylib.detail, "Override · Version 1.4.2 · 1 Patches");
        assert_eq!(
            (skylib.source, skylib.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("bcr.bazel.build"))
        );
        let cc: Vec<_> = items.iter().filter(|i| i.name == "rules_cc").map(|i| i.version.as_deref()).collect();
        assert_eq!(cc, [Some("0.0.1"), Some("0.0.4")]);
        let foo = find("rules_foo", "override");
        assert_eq!((foo.source, foo.source_host.as_deref()), (SourceClass::Git, Some("git.corp.example")));
        assert_eq!(
            (foo.version.as_deref(), foo.detail.as_str()),
            (Some("6a2c2e22849b"), "Git-Override · Commit 6a2c2e22849b")
        );
        let bar = find("rules_bar", "override");
        assert_eq!((bar.source, bar.source_host.as_deref()), (SourceClass::PrivateRegistry, Some("github.com")));
        assert_eq!(find("local_lib", "override").source, SourceClass::Path);
        let zlib = find("zlib", "repo_rule");
        assert_eq!((zlib.source, zlib.source_host.as_deref()), (SourceClass::PrivateRegistry, Some("zlib.net")));
        assert_eq!(zlib.detail, "Repository-Regel (Archiv)");
        let tools = find("corp_tools", "repo_rule");
        assert_eq!((tools.source, tools.source_host.as_deref()), (SourceClass::Git, Some("git.corp.example")));
        assert_eq!(
            (tools.version.as_deref(), tools.detail.as_str()),
            (Some("v2.3.0"), "Repository-Regel (Git) · Tag v2.3.0")
        );
        assert!(!items.iter().any(|i| i.name == "fake" || i.name.contains("uuid")));
        let text = format!("{items:?}");
        for secret in [
            "hunter2",
            "bazel/rules_foo",
            "sha256-",
            "rules_bar/archive",
            "../local_lib",
            "skylib.patch",
            "9a93b2b7",
            "tools/corp_tools",
        ] {
            assert!(!text.contains(secret), "{secret} must not be stored");
        }
    }

    #[test]
    fn lockfile_with_registry_file_hashes() {
        let lock = r#"{
  "lockFileVersion": 11,
  "registryFileHashes": {
    "https://bcr.bazel.build/bazel_registry.json": "8a28e4aff06ee60aed2a8c281907fb8bcbf3b753c91fb5a5c57da3215d5b3497",
    "https://bcr.bazel.build/modules/abseil-cpp/20210324.2/MODULE.bazel": "7cd0312e064fde87c8d1cd79ba06c876bd23630c83466e9500321be55c96ace2",
    "https://bcr.bazel.build/modules/abseil-cpp/20230802.0/MODULE.bazel": "d253ae36a8bd9ee3c5955384096ccb6baf16a1b1e93e858370da0a3b94f77c16",
    "https://bcr.bazel.build/modules/abseil-cpp/20230802.0/source.json": "9e3f6a6bb8f1ab3a4f0b0f5b1e0c8a2f3d4c5b6a7980a1b2c3d4e5f60718293a",
    "https://bcr.bazel.build/modules/platforms/0.0.10/MODULE.bazel": "8cb8efaf200bdeb2150d93e162c40f388529a25852b332cec879373771e48ed5",
    "https://bcr.bazel.build/modules/platforms/0.0.10/source.json": "f22828ff4cf021a6b577f1bf6341cb9dcd7965092a439f64fc1bb3b7a5ae4bd5",
    "https://bcr.bazel.build/modules/foo/2.0/source.json": "0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c",
    "https://reader:t0ken@registry.corp.example/bzl/modules/internal_rules/1.4.0/source.json": "1a2b3c4d",
    "https://registry.corp.example/bzl/modules/missing/1.0/MODULE.bazel": "not found"
  },
  "selectedYankedVersions": { "foo@2.0": "Yanked for demo purposes" },
  "moduleExtensions": {
    "//:extension.bzl%lockfile_ext": {
      "general": { "bzlTransitiveDigest": "abc", "usagesDigest": "def", "generatedRepoSpecs": { "hello": { "attributes": { "urls": ["https://x.example/y"] } } } }
    }
  }
}"#;
        let items = Bazel.parse_file(&file("MODULE.bazel.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v11 · 4 Module aufgelöst · 4 Moduldateien betrachtet");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let abseil = find("abseil-cpp");
        assert_eq!(
            (abseil.version.as_deref(), abseil.install_state),
            (Some("20230802.0"), Some(InstallState::Resolved))
        );
        assert_eq!(
            (abseil.source, abseil.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("bcr.bazel.build"))
        );
        assert_eq!(items.iter().filter(|i| i.name == "abseil-cpp").count(), 1, "considered versions are not resolved");
        let foo = find("foo");
        assert_eq!(foo.evidence.deprecated.as_deref(), Some("Zurückgezogen: Yanked for demo purposes"));
        let internal = find("internal_rules");
        assert_eq!(
            (internal.source, internal.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("registry.corp.example"))
        );
        assert!(!items.iter().any(|i| i.name == "missing" || i.name == "hello"));
        let text = format!("{items:?}");
        assert!(!text.contains("t0ken") && !text.contains("/bzl/") && !text.contains("8a28e4af"));
    }

    #[test]
    fn older_lockfile_with_module_graph() {
        let lock = r#"{
  "lockFileVersion": 6,
  "moduleFileHash": "05bf91c2e944619f7e056374017d45d1c56a3ff867cead8e07f810e02d1d8ecd",
  "flags": { "cmdRegistries": ["https://bcr.bazel.build/"], "cmdModuleOverrides": {}, "allowedYankedVersions": [] },
  "localOverrideHashes": { "bazel_tools": "1ae69322ac3823527337acf02016e8ee95813d8d356f47060255b8956fa642f0" },
  "moduleDepGraph": {
    "<root>": {
      "name": "smoke", "version": "0.0.1", "key": "<root>", "repoName": "smoke",
      "deps": { "bazel_skylib": "bazel_skylib@1.5.0", "toolchains_arm_gnu": "toolchains_arm_gnu@_", "bazel_tools": "bazel_tools@_" }
    },
    "bazel_skylib@1.5.0": {
      "name": "bazel_skylib", "version": "1.5.0", "key": "bazel_skylib@1.5.0", "repoName": "bazel_skylib",
      "deps": { "platforms": "platforms@0.0.8", "bazel_tools": "bazel_tools@_" },
      "repoSpec": { "ruleClassName": "http_archive", "attributes": { "urls": ["https://github.com/bazelbuild/bazel-skylib/releases/download/1.5.0/bazel-skylib-1.5.0.tar.gz"] } }
    },
    "platforms@0.0.8": { "name": "platforms", "version": "0.0.8", "key": "platforms@0.0.8", "deps": {} },
    "toolchains_arm_gnu@_": { "name": "toolchains_arm_gnu", "version": "1.0.0", "key": "toolchains_arm_gnu@_", "deps": {} },
    "bazel_tools@_": { "name": "bazel_tools", "version": "", "key": "bazel_tools@_", "deps": {} }
  }
}"#;
        let items = Bazel.parse_file(&file("MODULE.bazel.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v6 · 2 Module aufgelöst");
        assert_eq!(items[0].evidence.dependencies.len(), 3);
        assert!(
            items[0]
                .evidence
                .dependencies
                .iter()
                .any(|d| d.name == "bazel_skylib" && d.exact && d.requirement == "1.5.0")
        );
        let skylib = items.iter().find(|i| i.name == "bazel_skylib").unwrap();
        assert_eq!(
            (skylib.source, skylib.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("bcr.bazel.build"))
        );
        assert!(skylib.evidence.dependencies.iter().any(|d| d.name == "platforms" && d.requirement == "0.0.8"));
        assert!(!items.iter().any(|i| i.name == "smoke" || i.name == "toolchains_arm_gnu" || i.name == "bazel_tools"));
        assert!(!format!("{items:?}").contains("bazel-skylib-1.5.0.tar.gz"));
    }

    #[test]
    fn broken_lockfile_is_an_error_and_workspace_is_recognised_only() {
        assert!(Bazel.parse_file(&file("MODULE.bazel.lock", "{\"lockFileVersion\": 11,")).is_err());
        assert!(Bazel.parse_file(&file("MODULE.bazel.lock", "[]")).is_err());
        let empty = Bazel.parse_file(&file("WORKSPACE", "")).unwrap();
        assert_eq!(empty[0].detail, "WORKSPACE (nicht ausgewertet)");
        let workspace = "workspace(name = \"app\")\n\nload(\"@bazel_tools//tools/build_defs/repo:http.bzl\", \"http_archive\")\n\nhttp_archive(\n    name = \"rules_python\",\n    url = \"https://github.com/bazelbuild/rules_python/releases/download/0.31.0/rules_python-0.31.0.tar.gz\",\n)\n\nlocal_repository(name = \"lib\", path = \"../lib\")\n";
        let items = Bazel.parse_file(&file("WORKSPACE.bazel", workspace)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].detail, "WORKSPACE (nicht ausgewertet) · 2 Repository-Regeln");
        assert!(
            Bazel.parse_file(&file("workspace", "Notes about my workspace.\nNothing to see.\n")).unwrap().is_empty()
        );
        assert!(Bazel.parse_file(&file("Workspace", "print(\"not bazel\")\n")).unwrap().is_empty());
    }

    #[test]
    fn call_arguments_skip_strings_and_comments() {
        assert_eq!(arguments("name = \")\", x = 1) rest"), Some("name = \")\", x = 1"));
        assert_eq!(arguments("a = [1, (2)], # )\n b = 3)"), Some("a = [1, (2)], # )\n b = 3"));
        assert_eq!(arguments("doc = \"\"\"a ) b\"\"\")"), Some("doc = \"\"\"a ) b\"\"\""));
        assert_eq!(arguments("never closed"), None);
    }
}
