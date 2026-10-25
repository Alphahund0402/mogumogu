//! Paket: paket.dependencies (declared NuGet packages, GitHub and Gist
//! files, Git repositories and HTTP files per group), paket.lock (resolved,
//! with remotes and dependency edges) and paket.references (package names
//! per project, as manifest edges). Paket is never started; source
//! credentials (`username:`/`password:`) and `build:` commands of Git
//! dependencies are neither run nor kept. The `packages` folder beside
//! paket.dependencies and paket.lock is inspected by folder names, with
//! versions from .nupkg names or one .nuspec per package; `paket-files`
//! (downloaded remote files) is not read at all.
use super::maven::Hosts;
use super::nuget::PUBLIC;
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::xml;
use std::collections::{BTreeMap, BTreeSet};

pub struct Paket;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "paket",
    name: "Paket",
    markers: &["paket.dependencies", "paket.lock", "*paket.references"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "paket.dependencies (nuget, github, gist, git, http, Gruppen)",
        "paket.lock (NUGET, GITHUB, GIST, GIT, HTTP, Gruppen)",
        "paket.references",
        "packages/ (Ordnernamen, Version aus .nupkg/.nuspec)",
    ],
    limits: "Kein Paket-Aufruf; Quellen nur als Hosts, Zugangsdaten werden nie gelesen; Versionsbedingungen und Framework-Einschränkungen nicht ausgewertet; Build-Befehle von Git-Abhängigkeiten und paket-files nicht gelesen.",
};

/// Package folders inspected below `packages`.
const MAX_PACKAGES: usize = 5_000;

impl Adapter for Paket {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name.to_ascii_lowercase().as_str() {
            "paket.dependencies" => bounded(dependencies(file)),
            "paket.lock" => bounded(lock(file)?),
            _ => bounded(references(file)),
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let has = |file: &str| siblings.iter().any(|s| s == file);
        let label = match name {
            "packages" if has("paket.dependencies") && has("paket.lock") => "Paket-Pakete",
            "paket-files" if has("paket.dependencies") => "paket-files",
            _ => return None,
        };
        Some(DirClaim { adapter: "paket", category: ItemCategory::Environment, label })
    }

    fn inspect_dir(&self, claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        if claim.label == "paket-files" {
            return Ok(vec![
                InventoryItem::new(ItemCategory::Environment, "paket", "paket-files", rel_path)
                    .detail("Heruntergeladene GitHub-/HTTP-Dateien; nicht gelesen"),
            ]);
        }
        let mut packages = Vec::new();
        for (name, is_dir) in access.list(&[]) {
            if !is_dir || name.starts_with('.') || packages.len() >= MAX_PACKAGES {
                continue;
            }
            if let Some(version) = installed_version(access, &[&name]) {
                packages.push(installed(rel_path, &name, version, ""));
                continue;
            }
            // Packages of a non-main group lie in `packages/<group>/<name>`.
            for (child, child_dir) in access.list(&[&name]) {
                if child_dir
                    && packages.len() < MAX_PACKAGES
                    && let Some(version) = installed_version(access, &[&name, &child])
                {
                    packages.push(installed(rel_path, &child, version, &name));
                }
            }
        }
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, "paket", "packages", rel_path)
                .detail(format!("Von Paket entpackte Pakete · {} Pakete · nicht ausgeführt", packages.len())),
        ];
        items.extend(packages);
        bounded(items)
    }
}

/// `Some(version)` for a package folder: from `<name>.<version>.nupkg`, or
/// from the `<version>` of its .nuspec; `None` for anything else.
fn installed_version(access: &mut dyn DirAccess, path: &[&str]) -> Option<Option<String>> {
    let name = path.last()?.to_ascii_lowercase();
    let entries = access.list(path);
    let files = || entries.iter().filter(|(_, dir)| !dir).map(|(f, _)| f);
    let from_nupkg = files().find_map(|file| {
        let lower = file.to_ascii_lowercase();
        let version = lower.strip_suffix(".nupkg")?.strip_prefix(&name)?.strip_prefix('.')?;
        version
            .starts_with(|c: char| c.is_ascii_digit())
            .then(|| file[name.len() + 1..name.len() + 1 + version.len()].to_string())
    });
    if from_nupkg.is_some() {
        return Some(from_nupkg);
    }
    let nuspec = files().find(|file| file.to_ascii_lowercase().ends_with(".nuspec"))?.clone();
    let mut nuspec_path = path.to_vec();
    nuspec_path.push(&nuspec);
    let version = access.read_text(&nuspec_path, 64 * 1024).and_then(|text| {
        let elements = xml::elements(&text, 2_000, 16).ok()?;
        elements.into_iter().find(|e| e.name == "version" && e.parent == "metadata").map(|e| e.text)
    });
    Some(version.filter(|v| !v.is_empty()))
}

fn installed(rel_path: &str, name: &str, version: Option<String>, group: &str) -> InventoryItem {
    let detail = if version.is_some() { "von Paket entpackt" } else { "Ordner vorhanden; Version unbekannt" };
    let mut item = InventoryItem::new(ItemCategory::Package, "paket", name, rel_path)
        .version(version.unwrap_or_default())
        .state(InstallState::Installed)
        .detail(detail);
    item.evidence.context = group_context(group);
    item
}

/// `Main` is Paket's default group and stays without context.
fn group_context(name: &str) -> String {
    if name.eq_ignore_ascii_case("main") { String::new() } else { name.to_string() }
}

/// The line without a `//` comment; `//` inside a URL is kept.
fn code(line: &str) -> &str {
    let cut = line.match_indices("//").map(|(i, _)| i).find(|i| *i == 0 || line[..*i].ends_with([' ', '\t']));
    cut.map_or(line, |i| &line[..i]).trim()
}

/// A NuGet source: a feed URL or a local folder.
fn source_class(source: &str) -> (SourceClass, Option<String>) {
    if source.contains("://") && !source.to_ascii_lowercase().starts_with("file:") {
        classify_url(source, PUBLIC)
    } else {
        (SourceClass::Path, None)
    }
}

/// A full commit id is shortened, so it is not taken for a secret token.
fn commit(reference: &str) -> String {
    let hex = reference.len() >= 32 && reference.bytes().all(|b| b.is_ascii_hexdigit());
    if hex { reference[..12].to_string() } else { reference.to_string() }
}

/// The repository name of a Git URL (`…/Paket.git` → `Paket`). Query and
/// fragment can carry signatures (`?sig=…`) and are dropped first.
fn repository_name(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or(url);
    let last = url.trim_end_matches('/').rsplit(['/', ':', '\\']).next().unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

fn dependencies(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut group = String::new();
    let mut groups = 0;
    // The feed of each group; `None` once a group names different ones.
    let mut feeds: BTreeMap<String, Option<(SourceClass, Option<String>)>> = BTreeMap::new();
    let mut hosts = Hosts::default();
    let (mut frameworks, mut storage) = (None, None);
    let mut items: Vec<InventoryItem> = Vec::new();
    let mut packages: Vec<usize> = Vec::new();
    for line in file.content.lines().take(50_000) {
        let line = code(line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let keyword = words.next().unwrap_or_default().to_ascii_lowercase();
        let rest: Vec<&str> = words.take(32).collect();
        let first = rest.first().copied().unwrap_or_default();
        let item = |name: &str| {
            let mut item =
                InventoryItem::new(ItemCategory::Package, "paket", name, file.rel_path()).state(InstallState::Declared);
            item.evidence.context = group.clone();
            item
        };
        match keyword.as_str() {
            "source" if !first.is_empty() => {
                hosts.add(first, PUBLIC);
                let feed = source_class(first);
                feeds
                    .entry(group.clone())
                    .and_modify(|known| {
                        if known.as_ref() != Some(&feed) {
                            *known = None;
                        }
                    })
                    .or_insert(Some(feed));
            }
            "group" if !first.is_empty() => {
                group = group_context(first);
                groups += 1;
            }
            "nuget" | "clitool" if !first.is_empty() => {
                // Version tokens end where the first `option:` starts.
                let options = rest.iter().skip(1).position(|w| w.ends_with(':')).map_or(rest.len(), |p| p + 1);
                let spec = rest[1..options].join(" ");
                let exact = match rest[1..options] {
                    [version] if version.starts_with(|c: char| c.is_ascii_digit()) => Some(version),
                    ["=" | "==", version] => Some(version),
                    _ => None,
                };
                let mut detail =
                    if spec.is_empty() { "ohne Versionsangabe".to_string() } else { format!("Spezifikation {spec}") };
                if options < rest.len() {
                    detail.push_str(" · Optionen nicht ausgewertet");
                }
                let mut item = item(first).version(exact.unwrap_or_default()).detail(detail);
                if keyword == "clitool" {
                    item.evidence.qualifier = "clitool".into();
                }
                packages.push(items.len());
                items.push(item);
            }
            "github" | "gist" if !first.is_empty() => {
                let (repository, reference) = first.split_once(':').map_or((first, None), |(r, f)| (r, Some(f)));
                let path = rest.get(1).copied().unwrap_or_default();
                let mut detail = match (keyword.as_str(), path) {
                    ("gist", _) => format!("Gist-Datei {path}"),
                    (_, "") => "GitHub-Repository".to_string(),
                    _ => format!("GitHub-Datei {path}"),
                };
                if let Some(reference) = reference {
                    detail.push_str(&format!(" · Ref {}", commit(reference)));
                }
                let mut item = item(repository).source(SourceClass::Git, None).detail(detail.trim().to_string());
                item.evidence.qualifier = path.to_string();
                items.push(item);
            }
            "git" if !first.is_empty() => {
                let reference = rest.get(1).filter(|w| !w.ends_with(':')).map(|r| commit(r));
                let local =
                    first.to_ascii_lowercase().starts_with("file:") || (!first.contains("://") && !first.contains('@'));
                let source = if local { SourceClass::Path } else { SourceClass::Git };
                let mut detail = String::from("Git-Repository");
                if let Some(reference) = &reference {
                    detail.push_str(&format!(" · Ref {reference}"));
                }
                if rest.iter().any(|w| w.eq_ignore_ascii_case("build:")) {
                    detail.push_str(" · Build-Befehl (nicht ausgeführt)");
                }
                let mut item = item(&repository_name(first)).source(source, None).detail(detail);
                item.evidence.qualifier = reference.unwrap_or_default();
                items.push(item);
            }
            "http" if !first.is_empty() => {
                let host = classify_url(first, PUBLIC).1;
                let name = rest.get(1).map_or_else(|| repository_name(first), |file| file.to_string());
                let detail = format!("HTTP-Datei von {}", host.as_deref().unwrap_or("unbekannter Quelle"));
                items.push(item(&name).detail(detail));
            }
            _ if keyword.starts_with("framework:") => {
                frameworks = Some(line["framework:".len()..].trim().to_string());
            }
            _ if keyword.starts_with("storage:") => storage = Some(line["storage:".len()..].trim().to_string()),
            _ => {}
        }
    }
    // A package's feed is known only when its group names exactly one.
    for index in packages {
        let item = &mut items[index];
        if let Some(Some((class, host))) = feeds.get(&item.evidence.context) {
            item.source = *class;
            item.source_host = host.clone();
        }
    }
    let mut detail = String::from("Paket-Abhängigkeiten (nicht ausgeführt)");
    if groups > 0 {
        detail.push_str(&format!(" · {groups} weitere Gruppen"));
    }
    if let Some(frameworks) = frameworks {
        detail.push_str(&format!(" · Frameworks {frameworks}"));
    }
    if let Some(storage) = storage {
        detail.push_str(&format!(" · Speicher {storage}"));
    }
    if let Some(label) = hosts.label() {
        detail.push_str(&format!(" · Quellen: {label}"));
    }
    items.insert(0, manifest("paket", file, detail));
    items
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    NuGet,
    GitHub,
    Gist,
    Git,
    Http,
}

/// `Name (1.2.3) - options` → name and version; `(commit)` has no name.
fn entry(text: &str) -> (&str, Option<&str>) {
    let main = text.split(" - ").next().unwrap_or(text).trim();
    if let Some(inner) = main.strip_prefix('(') {
        return ("", inner.split(')').next());
    }
    match main.split_once(" (") {
        Some((name, rest)) => (name.trim(), rest.split(')').next()),
        None => (main.split_whitespace().next().unwrap_or(main), None),
    }
}

fn lock(file: &FoundFile<'_>) -> AdapterResult {
    let mut items: Vec<InventoryItem> = Vec::new();
    let (mut group, mut groups) = (String::new(), 0);
    let mut section: Option<Section> = None;
    let mut remote: Option<String> = None;
    let mut entry_indent: Option<usize> = None;
    // Whether the last entry became an item that dependency lines belong to.
    let mut attached = false;
    for (number, line) in file.content.lines().enumerate().take(2_000_000) {
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            entry_indent = None;
            attached = false;
            match text {
                "NUGET" => section = Some(Section::NuGet),
                "GITHUB" => section = Some(Section::GitHub),
                "GIST" => section = Some(Section::Gist),
                "GIT" => section = Some(Section::Git),
                "HTTP" => section = Some(Section::Http),
                _ if text.starts_with("GROUP ") => {
                    group = group_context(text["GROUP ".len()..].trim());
                    groups += 1;
                    section = None;
                }
                // `RESTRICTION: …`, `STORAGE: NONE`, `REFERENCES: STRICT` …
                _ if text.split_once(':').is_some_and(|(key, _)| {
                    !key.is_empty() && key.bytes().all(|b| b.is_ascii_uppercase() || b == b'-' || b == b'_')
                }) => {}
                _ => return Err(format!("paket.lock: unbekannter Abschnitt in Zeile {}", number + 1)),
            }
            remote = None;
            continue;
        }
        let Some(kind) = section else {
            return Err(format!("paket.lock: Eintrag ohne Abschnitt in Zeile {}", number + 1));
        };
        if let Some(url) = text.strip_prefix("remote:") {
            remote = Some(url.trim().to_string());
            entry_indent = None;
            attached = false;
            continue;
        }
        if entry_indent.is_some_and(|level| indent > level) {
            // A dependency of the entry above, or `build:`/`path:`/`os:` of a remote file.
            let (name, requirement) = entry(text);
            let setting = ["build:", "path:", "os:"].iter().any(|s| text.to_ascii_lowercase().starts_with(s));
            if let Some(item) = items.last_mut().filter(|_| attached && !setting && !name.is_empty())
                && item.evidence.dependencies.len() < 1_000
            {
                item.evidence.dependencies.push(PackageDependency {
                    name: name.to_string(),
                    requirement: requirement.unwrap_or_default().to_string(),
                    kind: "lock".into(),
                    exact: false,
                });
            }
            continue;
        }
        entry_indent = Some(indent);
        let (name, version) = entry(text);
        let remote = remote.as_deref().unwrap_or_default();
        let (item_name, source, detail, qualifier) = match kind {
            Section::NuGet => {
                let (class, host) = source_class(remote);
                (name.to_string(), (class, host), "aufgelöst laut paket.lock".to_string(), String::new())
            }
            Section::GitHub | Section::Gist => {
                let label = if kind == Section::Gist { "Gist-Datei" } else { "GitHub-Datei" };
                (
                    remote.to_string(),
                    (SourceClass::Git, None),
                    format!("{label} {name} · aufgelöst laut paket.lock"),
                    name.to_string(),
                )
            }
            Section::Git => {
                let local = remote.to_ascii_lowercase().starts_with("file:")
                    || (!remote.contains("://") && !remote.contains('@'));
                let class = if local { SourceClass::Path } else { SourceClass::Git };
                (
                    repository_name(remote),
                    (class, None),
                    "Git-Repository · aufgelöst laut paket.lock".into(),
                    name.to_string(),
                )
            }
            Section::Http => {
                let host = classify_url(remote, PUBLIC).1;
                let detail = format!("HTTP-Datei von {}", host.as_deref().unwrap_or("unbekannter Quelle"));
                (repository_name(name), (SourceClass::Unknown, None), detail, name.to_string())
            }
        };
        attached = !item_name.is_empty();
        if !attached {
            continue;
        }
        // HTTP entries repeat their path in parentheses, no version.
        let version = match kind {
            Section::NuGet => version.map(str::to_string),
            Section::Http => None,
            _ => version.map(commit),
        };
        let mut item = InventoryItem::new(ItemCategory::Package, "paket", item_name, file.rel_path())
            .version(version.unwrap_or_default())
            .state(InstallState::Resolved)
            .source(source.0, source.1)
            .detail(detail);
        item.evidence.context = group.clone();
        item.evidence.qualifier = qualifier;
        items.push(item);
    }
    let mut detail = String::from("Paket-Lockfile");
    if groups > 0 {
        detail.push_str(&format!(" · {groups} weitere Gruppen"));
    }
    items.insert(0, manifest("paket", file, detail));
    Ok(items)
}

fn references(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut group = String::new();
    let mut edges: Vec<PackageDependency> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut files = 0;
    for line in file.content.lines().take(20_000) {
        let text = code(line);
        let mut words = text.split_whitespace();
        let Some(first) = words.next() else { continue };
        let lower = first.to_ascii_lowercase();
        if lower.starts_with('#') {
            continue;
        }
        if lower == "group" {
            group = group_context(words.next().unwrap_or_default());
            continue;
        }
        if lower.starts_with("file:") {
            files += 1;
            continue;
        }
        let nested = line.starts_with([' ', '\t']);
        let valid = first.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if (nested && matches!(lower.as_str(), "exclude" | "alias")) || !valid || edges.len() >= 5_000 {
            continue;
        }
        let kind = if group.is_empty() { "paket.references".to_string() } else { format!("paket.references:{group}") };
        if seen.insert((lower, kind.clone())) {
            edges.push(PackageDependency { name: first.to_string(), kind, ..PackageDependency::default() });
        }
    }
    let mut detail = format!("Paketverweise des Projekts · {} Pakete", edges.len());
    if files > 0 {
        detail.push_str(&format!(" · {files} Dateiverweise"));
    }
    let mut manifest = manifest("paket", file, detail);
    manifest.evidence.dependencies = edges;
    vec![manifest]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const DEPENDENCIES: &str = r#"source https://api.nuget.org/v3/index.json
storage: none
framework: net8.0, netstandard2.0

// the default group
nuget FSharp.Core ~> 8.0
nuget Newtonsoft.Json 13.0.3
nuget Argu
nuget xunit >= 2.9 restriction: >= net8.0
github forki/FsUnit:7623fc13439f0e60bd05c1ed3b5f6dcb937fe468 FsUnit.fs
github fsharp/FAKE src/app/FakeLib/Globbing/Globbing.fs
gist Thorium/1972349 timestamp.fs
git https://github.com/fsprojects/Paket.git master build: "build.cmd", OS: windows
git file:///C:/repos/local-lib
http http://www.fssnip.net/1n decrypt.fs

group Build
  source https://user:hunter2-feed@nuget.corp.example/v3/index.json username: "ci" password: "p4ss-word"
  nuget FAKE 5.23.1
  nuget Newtonsoft.Json == 12.0.3

group Local
  source ../nugets
  source https://api.nuget.org/v3/index.json
  nuget Local.Tool
"#;

    #[test]
    fn dependencies_with_groups_sources_and_remote_files() {
        let items = Paket.parse_file(&file("paket.dependencies", DEPENDENCIES)).unwrap();
        assert_eq!(
            items[0].detail,
            "Paket-Abhängigkeiten (nicht ausgeführt) · 2 weitere Gruppen · Frameworks net8.0, netstandard2.0 · Speicher none · Quellen: 2 öffentlich, 1 lokal, privat: nuget.corp.example"
        );
        let debug = format!("{items:?}");
        for secret in ["hunter2", "p4ss", "\"ci\"", "build.cmd", "nugets", "C:/repos"] {
            assert!(!debug.contains(secret), "{secret} leaked");
        }
        let find = |name: &str, group: &str| {
            items
                .iter()
                .find(|i| i.name == name && i.evidence.context == group)
                .unwrap_or_else(|| panic!("{name} {group}"))
        };
        let core = find("FSharp.Core", "");
        assert_eq!((core.version.as_deref(), core.detail.as_str()), (None, "Spezifikation ~> 8.0"));
        assert_eq!((core.source, core.source_host.as_deref()), (SourceClass::PublicRegistry, Some("api.nuget.org")));
        assert_eq!(find("Newtonsoft.Json", "").version.as_deref(), Some("13.0.3"), "a bare version is a pin");
        assert_eq!(find("Argu", "").detail, "ohne Versionsangabe");
        assert_eq!(find("xunit", "").detail, "Spezifikation >= 2.9 · Optionen nicht ausgewertet");
        let fsunit = find("forki/FsUnit", "");
        assert_eq!((fsunit.source, fsunit.evidence.qualifier.as_str()), (SourceClass::Git, "FsUnit.fs"));
        assert_eq!(fsunit.detail, "GitHub-Datei FsUnit.fs · Ref 7623fc13439f");
        assert_eq!(find("fsharp/FAKE", "").evidence.qualifier, "src/app/FakeLib/Globbing/Globbing.fs");
        assert_eq!(find("Thorium/1972349", "").detail, "Gist-Datei timestamp.fs");
        let paket = find("Paket", "");
        assert_eq!(
            (paket.source, paket.detail.as_str()),
            (SourceClass::Git, "Git-Repository · Ref master · Build-Befehl (nicht ausgeführt)")
        );
        assert_eq!(find("local-lib", "").source, SourceClass::Path);
        let snippet = find("decrypt.fs", "");
        assert_eq!((snippet.source, snippet.detail.as_str()), (SourceClass::Unknown, "HTTP-Datei von www.fssnip.net"));

        let fake = find("FAKE", "Build");
        assert_eq!(
            (fake.source, fake.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("nuget.corp.example"))
        );
        assert_eq!(find("Newtonsoft.Json", "Build").version.as_deref(), Some("12.0.3"));
        assert_eq!(find("Local.Tool", "Local").source, SourceClass::Unknown, "two feeds: the source stays open");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
    }

    const LOCK: &str = "STORAGE: NONE
RESTRICTION: || (== net8.0) (== netstandard2.0)
NUGET
  remote: https://api.nuget.org/v3/index.json
    Argu (6.1.1)
      FSharp.Core (>= 4.3.2) - restriction: >= netstandard2.0
      System.Configuration.ConfigurationManager (>= 4.4) - restriction: >= netstandard2.0
    Chessie (0.6)
      FSharp.Core  - restriction: < netstandard1.6
    DotNet.ReproducibleBuilds (0.1.66) - copy_local: true
    FSharp.Core (8.0.400)
  remote: https://deploy:t0ken-value@nuget.corp.example/v3/index.json
    Corp.Logging (2.1.0)
GITHUB
  remote: forki/FsUnit
    FsUnit.fs (7623fc13439f0e60bd05c1ed3b5f6dcb937fe468)
  remote: fsharp/FAKE
    modules/Octokit/Octokit.fsx (a25c2f256a99242c1106b5a3478aae6bb68c7a93)
      Octokit (>= 0.20)
GIT
  remote: https://github.com/fsprojects/Paket.git
     (05366e390e7552a569f3f328a0f3094249f3b93b)
      build: build.cmd --secret-flag
HTTP
  remote: http://www.fssnip.net
    raw/1n (/raw/1n)
GROUP Build
RESTRICTION: >= net461
NUGET
  remote: https://api.nuget.org/v3/index.json
    FAKE (5.23.1)
    FSharp.Core (4.7.2)
";

    #[test]
    fn lock_with_sections_groups_and_edges() {
        let items = Paket.parse_file(&file("paket.lock", LOCK)).unwrap();
        assert_eq!(items[0].detail, "Paket-Lockfile · 1 weitere Gruppen");
        let find = |name: &str, group: &str| {
            items
                .iter()
                .find(|i| i.name == name && i.evidence.context == group)
                .unwrap_or_else(|| panic!("{name} {group}"))
        };
        let argu = find("Argu", "");
        assert_eq!((argu.version.as_deref(), argu.install_state), (Some("6.1.1"), Some(InstallState::Resolved)));
        assert_eq!(argu.source_host.as_deref(), Some("api.nuget.org"));
        let edges: Vec<(&str, &str)> =
            argu.evidence.dependencies.iter().map(|d| (d.name.as_str(), d.requirement.as_str())).collect();
        assert_eq!(edges, [("FSharp.Core", ">= 4.3.2"), ("System.Configuration.ConfigurationManager", ">= 4.4")]);
        assert_eq!(find("Chessie", "").evidence.dependencies[0].requirement, "");
        assert_eq!(find("DotNet.ReproducibleBuilds", "").version.as_deref(), Some("0.1.66"));
        let corp = find("Corp.Logging", "");
        assert_eq!(
            (corp.source, corp.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("nuget.corp.example"))
        );
        let octokit = find("fsharp/FAKE", "");
        assert_eq!((octokit.version.as_deref(), octokit.source), (Some("a25c2f256a99"), SourceClass::Git));
        assert_eq!(octokit.evidence.dependencies[0].name, "Octokit");
        let git = find("Paket", "");
        assert_eq!((git.version.as_deref(), git.evidence.dependencies.len()), (Some("05366e390e75"), 0));
        assert_eq!(find("1n", "").detail, "HTTP-Datei von www.fssnip.net");
        assert_eq!(find("FSharp.Core", "Build").version.as_deref(), Some("4.7.2"));
        assert_eq!(find("FSharp.Core", "").version.as_deref(), Some("8.0.400"));
        let debug = format!("{items:?}");
        assert!(!debug.contains("t0ken") && !debug.contains("secret-flag"));

        assert!(Paket.parse_file(&file("paket.lock", "{\"not\": \"paket\"}")).is_err());
        assert!(Paket.parse_file(&file("paket.lock", "    Argu (6.1.1)\n")).is_err());
        assert_eq!(Paket.parse_file(&file("paket.lock", "")).unwrap().len(), 1);
    }

    #[test]
    fn references_are_manifest_edges_per_group() {
        let text = "Newtonsoft.Json\nFSharp.Core copy_local: false\nDapper\n  exclude Dapper.Contrib.dll\n// comment\nFile:FsUnit.fs\nFile: Globbing.fs Tests\\Globbing\n\ngroup Test\n  NUnit\n  NUnit3TestAdapter // runner\n  newtonsoft.json\n";
        let items = Paket.parse_file(&file("App.fsproj.paket.references", text)).unwrap();
        assert_eq!(items.len(), 1, "references are no packages of their own");
        assert_eq!(items[0].detail, "Paketverweise des Projekts · 6 Pakete · 2 Dateiverweise");
        let edges: Vec<(&str, &str)> =
            items[0].evidence.dependencies.iter().map(|d| (d.name.as_str(), d.kind.as_str())).collect();
        assert_eq!(
            edges,
            [
                ("Newtonsoft.Json", "paket.references"),
                ("FSharp.Core", "paket.references"),
                ("Dapper", "paket.references"),
                ("NUnit", "paket.references:Test"),
                ("NUnit3TestAdapter", "paket.references:Test"),
                ("newtonsoft.json", "paket.references:Test"),
            ]
        );
    }

    #[test]
    fn packages_folder_only_beside_paket_files() {
        assert!(Paket.claim_dir("packages", &["paket.dependencies".into()], &[]).is_none(), "nothing restored yet");
        assert!(Paket.claim_dir("packages", &["package.json".into(), "paket.lock".into()], &[]).is_none());
        let claim = Paket.claim_dir("packages", &["paket.dependencies".into(), "paket.lock".into()], &[]).unwrap();
        let mut dir = FakeDir::default();
        dir.dirs.insert(
            "".into(),
            vec![
                ("Newtonsoft.Json".into(), true),
                ("FSharp.Core".into(), true),
                ("build".into(), true),
                (".paket".into(), true),
            ],
        );
        dir.dirs.insert(
            "Newtonsoft.Json".into(),
            vec![("Newtonsoft.Json.13.0.3.nupkg".into(), false), ("lib".into(), true)],
        );
        dir.dirs.insert("FSharp.Core".into(), vec![("FSharp.Core.nuspec".into(), false)]);
        dir.files.insert(
            "FSharp.Core/FSharp.Core.nuspec".into(),
            "<package><metadata><id>FSharp.Core</id><version>8.0.400</version></metadata></package>".into(),
        );
        dir.dirs.insert("build".into(), vec![("FAKE".into(), true)]);
        dir.dirs.insert("build/FAKE".into(), vec![("fake.5.23.1.nupkg".into(), false)]);
        let items = Paket.inspect_dir(claim, "packages", &mut dir).unwrap();
        assert_eq!(items[0].detail, "Von Paket entpackte Pakete · 3 Pakete · nicht ausgeführt");
        let version = |name: &str| items.iter().find(|i| i.name == name).unwrap().version.clone();
        assert_eq!(version("Newtonsoft.Json").as_deref(), Some("13.0.3"));
        assert_eq!(version("FSharp.Core").as_deref(), Some("8.0.400"));
        let fake = items.iter().find(|i| i.name == "FAKE").unwrap();
        assert_eq!((fake.version.as_deref(), fake.evidence.context.as_str()), (Some("5.23.1"), "build"));
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Installed)));

        let files = Paket.claim_dir("paket-files", &["paket.dependencies".into()], &[]).unwrap();
        assert_eq!(Paket.inspect_dir(files, "paket-files", &mut FakeDir::default()).unwrap().len(), 1);
    }
}
