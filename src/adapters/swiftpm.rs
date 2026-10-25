//! SwiftPM: Package.resolved v1–v3 (resolved pins) and `.package(…)` calls
//! in Package.swift (declared). Package.swift is Swift code: it is matched
//! as text and never compiled, so dependencies from variables, `#if`
//! branches or helper functions stay unknown. `.build` beside a
//! Package.swift holds build output and source checkouts; only the names
//! of the checkouts are listed.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::json;
use regex::Regex;
use std::sync::LazyLock;

pub struct SwiftPm;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "swiftpm",
    name: "SwiftPM",
    markers: &["Package.swift", "Package.resolved"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["Package.swift: .package(…) als Text", "Package.resolved v1–v3", ".build/checkouts (Ordnernamen)"],
    limits: "Kein swift-Aufruf; Package.swift wird nicht ausgeführt – Abhängigkeiten aus Variablen, #if-Zweigen oder Hilfsfunktionen bleiben unbekannt; Registry-Konfiguration (.swiftpm) nicht gelesen, Registry-Quellen bleiben unbekannt; Versionen in .build unbekannt.",
};

/// `// swift-tools-version:5.9` at the top of the manifest.
static TOOLS_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*//\s*swift-tools-version\s*:\s*([0-9]+(?:\.[0-9]+){0,2})").expect("valid regex")
});
static PACKAGE_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.package\s*\(").expect("valid regex"));
/// `url:`, `path:` or `id:` with a string literal.
static LOCATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\b(url|path|id)\s*:\s*"([^"\\]*)""#).expect("valid regex"));
/// `.upToNextMajor/Minor(from:)`, `from:`, `exact:`, `branch:`, `revision:`
/// (also as `.exact("…")`) or a `"1.0.0"..<"2.0.0"` range.
static REQUIREMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"\.upToNext(Major|Minor)\s*\(\s*from\s*:\s*"([^"\\]*)""#,
        r#"|\b(from|exact|branch|revision)\s*[:(]\s*"([^"\\]*)""#,
        r#"|"([^"\\]*)"\s*(\.\.<|\.\.\.)\s*"([^"\\]*)""#,
    ))
    .expect("valid regex")
});

impl Adapter for SwiftPm {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("Package.resolved") {
            return bounded(resolved(file)?);
        }
        bounded(manifest_items(file))
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        (name == ".build" && siblings.iter().any(|s| s == "Package.swift")).then_some(DirClaim {
            adapter: "swiftpm",
            category: ItemCategory::Output,
            label: "SwiftPM-Buildordner",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let checkouts: Vec<String> = access
            .list(&["checkouts"])
            .into_iter()
            .filter(|(name, is_dir)| *is_dir && !name.starts_with('.'))
            .map(|(name, _)| name)
            .collect();
        let mut items = vec![InventoryItem::new(ItemCategory::Output, "swiftpm", ".build", rel_path).detail(format!(
            "Buildausgabe mit {} Checkouts; nicht ausgeführt, Größe erst nach Messung bekannt",
            checkouts.len()
        ))];
        for name in checkouts {
            items.push(
                InventoryItem::new(ItemCategory::Package, "swiftpm", name, format!("{rel_path}/checkouts"))
                    .state(InstallState::Installed)
                    .detail("Checkout vorhanden; Version unbekannt"),
            );
        }
        bounded(items)
    }
}

/// The first 12 characters of a full commit id; a complete hash would be
/// redacted as a token.
pub(super) fn commit(value: &str) -> Option<&str> {
    (matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())).then(|| &value[..12])
}

/// A revision as shown: at most 12 characters.
pub(super) fn short(value: &str) -> &str {
    value.char_indices().nth(12).map_or(value, |(index, _)| &value[..index])
}

/// A local path rather than a URL: `/`, `./`, `../`, `~`, `file:` or a drive.
pub(super) fn is_local(location: &str) -> bool {
    let bytes = location.as_bytes();
    location.starts_with(['/', '\\', '.', '~'])
        || location.get(..5).is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
        || (bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'/' | b'\\'))
}

/// The last path segment of a repository URL or path without `.git`. The
/// authority of a URL (and any credentials in it) is never part of it.
pub(super) fn repo_name(location: &str) -> Option<&str> {
    let location = location.split(['?', '#']).next()?.trim();
    let path = match location.split_once("://") {
        Some((_, rest)) => rest.split_once('/')?.1,
        None if is_local(location) => location,
        // `git@host:owner/repo.git`
        None => location.split_once(':').map_or(location, |(_, path)| path),
    };
    let segment = path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next()?;
    let segment = segment.strip_suffix(".git").unwrap_or(segment);
    (!segment.is_empty() && segment != "." && segment != ".." && !segment.contains('@')).then_some(segment)
}

/// Swift source without `//` and (nested) `/* */` comments; string
/// literals are kept, so `//` inside a URL survives.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let (mut string, mut escaped, mut block) = (false, false, 0_usize);
    while let Some(ch) = chars.next() {
        if block > 0 {
            match (ch, chars.peek()) {
                ('*', Some('/')) => {
                    chars.next();
                    block -= 1;
                }
                ('/', Some('*')) => {
                    chars.next();
                    block = (block + 1).min(64);
                }
                ('\n', _) => out.push('\n'),
                _ => {}
            }
            continue;
        }
        if string {
            out.push(ch);
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"' | '\n') => string = false,
                _ => {}
            }
            continue;
        }
        match (ch, chars.peek()) {
            ('"', _) => {
                string = true;
                out.push(ch);
            }
            ('/', Some('/')) => {
                while chars.peek().is_some_and(|c| *c != '\n') {
                    chars.next();
                }
            }
            ('/', Some('*')) => {
                chars.next();
                block = 1;
            }
            _ => out.push(ch),
        }
    }
    out
}

/// The argument text of a call up to its closing parenthesis, at most 2 KiB.
fn arguments(code: &str) -> &str {
    let (mut depth, mut string, mut escaped) = (1_usize, false, false);
    let mut end = code.len();
    for (index, ch) in code.char_indices() {
        if index >= 2048 {
            end = index;
            break;
        }
        if string {
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &code[..index];
                }
            }
            _ => {}
        }
    }
    &code[..end]
}

/// Version (only when exact) and description of a requirement.
fn requirement(args: &str) -> (Option<String>, String) {
    let Some(found) = REQUIREMENT.captures(args) else {
        return (None, "Spezifikation nicht erkannt (Variable oder andere Form, nicht ausgewertet)".into());
    };
    if let Some(kind) = found.get(1) {
        let scope = if kind.as_str() == "Major" { "Hauptversion" } else { "Nebenversion" };
        return (None, format!("Spezifikation ab {} (gleiche {scope})", &found[2]));
    }
    if let Some(label) = found.get(3) {
        let value = &found[4];
        return match label.as_str() {
            "from" => (None, format!("Spezifikation ab {value} (gleiche Hauptversion)")),
            "exact" => (Some(value.to_string()), format!("Spezifikation exakt {value}")),
            "branch" => (None, format!("Branch {value}")),
            _ => (None, format!("Revision {}", short(value))),
        };
    }
    (None, format!("Spezifikation {}{}{}", &found[5], &found[6], &found[7]))
}

fn manifest_items(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let tools = file.content.lines().take(5).find_map(|line| TOOLS_VERSION.captures(line)).map(|c| c[1].to_string());
    let code = strip_comments(file.content);
    let mut items = Vec::new();
    for call in PACKAGE_CALL.find_iter(&code).take(10_000) {
        let args = arguments(&code[call.end()..]);
        let Some(location) = LOCATION.captures(args) else { continue };
        let value = &location[2];
        let (name, source, kind) = match &location[1] {
            "url" if is_local(value) => (repo_name(value), SourceClass::Path, ""),
            "url" => (repo_name(value), SourceClass::Git, ""),
            "path" => (repo_name(value), SourceClass::Path, ""),
            // A registry identity `scope.name`; registries.json is not read.
            _ => (Some(value).filter(|v| !v.is_empty()), SourceClass::Unknown, " · Registry"),
        };
        let Some(name) = name else { continue };
        let (version, spec) =
            if &location[1] == "path" { (None, "lokaler Pfad".to_string()) } else { requirement(args) };
        items.push(
            InventoryItem::new(ItemCategory::Package, "swiftpm", name, file.rel_path())
                .version(version.unwrap_or_default())
                .state(InstallState::Declared)
                .source(source, None)
                .detail(format!("{spec}{kind}")),
        );
    }
    let mut detail = String::from("Swift-Paketmanifest (nicht ausgeführt)");
    if let Some(tools) = tools {
        detail.push_str(&format!(" · Tools-Version {tools}"));
    }
    detail.push_str(&format!(" · {} Abhängigkeiten", items.len()));
    items.insert(0, manifest("swiftpm", file, detail));
    items
}

fn resolved(file: &FoundFile<'_>) -> AdapterResult {
    let value = json::parse(file.content, 32).map_err(|e| e.to_string())?;
    let format = value["version"].as_i64().unwrap_or(0);
    // v1 nests the pins in `object`; v2 and v3 (with `originHash`) do not.
    let (pins, legacy) = match (value["pins"].as_array(), value["object"]["pins"].as_array()) {
        (Some(pins), _) => (pins, false),
        (None, Some(pins)) => (pins, true),
        _ => return Err(format!("Package.resolved v{format}: Format nicht erkannt")),
    };
    let mut items = vec![manifest("swiftpm", file, format!("Package.resolved v{format} · {} Pins", pins.len()))];
    for pin in pins {
        let (name, kind, location) = if legacy {
            // v1 names the manifest's package (`SwiftProtobuf`); the identity
            // SwiftPM derives from the URL (`swift-protobuf`) matches v2+.
            let url = pin["repositoryURL"].as_str().unwrap_or_default();
            (repo_name(url).or_else(|| pin["package"].as_str()), "sourceControl", url)
        } else {
            (pin["identity"].as_str(), pin["kind"].as_str().unwrap_or_default(), pin["location"].as_str().unwrap_or(""))
        };
        let Some(name) = name.filter(|n| !n.is_empty()) else { continue };
        let (source, host) = match kind {
            "remoteSourceControl" => (SourceClass::Git, None),
            "localSourceControl" => (SourceClass::Path, None),
            "sourceControl" if is_local(location) => (SourceClass::Path, None),
            "sourceControl" => (SourceClass::Git, None),
            // SwiftPM writes an empty location for registry pins.
            "registry" if location.contains("://") => classify_url(location, &[]),
            _ => (SourceClass::Unknown, None),
        };
        let state = &pin["state"];
        let text = |key: &str| state[key].as_str().filter(|v| !v.is_empty());
        let revision = text("revision").map(short);
        let mut detail = String::from("aufgelöst laut Package.resolved");
        if let Some(branch) = text("branch") {
            detail.push_str(&format!(" · Branch {branch}"));
        }
        if text("version").is_none()
            && let Some(revision) = revision
        {
            detail.push_str(&format!(" · Revision {revision}"));
        }
        let version = text("version").or(text("branch")).or(revision).unwrap_or_default();
        items.push(
            InventoryItem::new(ItemCategory::Package, "swiftpm", name, file.rel_path())
                .version(version)
                .state(InstallState::Resolved)
                .source(source, host)
                .detail(detail),
        );
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const MANIFEST: &str = r#"// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "App",
    platforms: [.macOS(.v13), .iOS(.v16)],
    dependencies: [
        .package(url: "https://github.com/apple/swift-argument-parser", from: "1.2.0"),
        .package(url: "https://github.com/apple/swift-nio.git", exact: "2.58.0"),
        .package(
            url: "https://github.com/apple/swift-log.git",
            .upToNextMinor(from: "1.5.0")
        ),
        .package(url: "https://github.com/pointfreeco/swift-snapshot-testing", branch: "main"),
        .package(url: "https://github.com/a/pinned.git", revision: "0123456789abcdef0123456789abcdef01234567"),
        .package(url: "https://github.com/a/ranged", "1.0.0"..<"2.0.0"),
        .package(name: "Legacy", url: "https://github.com/a/legacy.git", .exact("0.9.1")),
        .package(url: "https://deploy:s3cr3t-token@git.corp.example/team/private-kit.git", from: "3.0.0"),
        .package(url: "git@github.com:org/ssh-kit.git", .upToNextMajor(from: "4.1.0")),
        .package(path: "../LocalKit"),
        .package(id: "mona.LinkedList", from: "1.1.0"),
        .package(url: "https://github.com/a/variable.git", from: someVersion),
        // .package(url: "https://github.com/commented/out.git", from: "1.0.0"),
        /* .package(url: "https://github.com/block/out.git", from: "1.0.0"), */
    ],
    targets: [.target(name: "App", dependencies: [.product(name: "NIO", package: "swift-nio")])]
)
"#;

    #[test]
    fn package_swift_calls_are_matched_as_text() {
        let items = SwiftPm.parse_file(&file("Package.swift", MANIFEST)).unwrap();
        assert!(items[0].detail.contains("Tools-Version 5.9") && items[0].detail.contains("nicht ausgeführt"));
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let parser = find("swift-argument-parser");
        assert_eq!((parser.source, parser.version.as_deref()), (SourceClass::Git, None));
        assert_eq!(parser.detail, "Spezifikation ab 1.2.0 (gleiche Hauptversion)");
        assert_eq!(find("swift-nio").version.as_deref(), Some("2.58.0"));
        assert_eq!(find("swift-log").detail, "Spezifikation ab 1.5.0 (gleiche Nebenversion)");
        assert_eq!(find("swift-snapshot-testing").detail, "Branch main");
        assert_eq!(find("pinned").detail, "Revision 0123456789ab");
        assert_eq!(find("ranged").detail, "Spezifikation 1.0.0..<2.0.0");
        assert_eq!(find("legacy").version.as_deref(), Some("0.9.1"));
        assert_eq!(find("ssh-kit").detail, "Spezifikation ab 4.1.0 (gleiche Hauptversion)");
        assert_eq!(find("LocalKit").source, SourceClass::Path);
        let registry = find("mona.LinkedList");
        assert_eq!(registry.source, SourceClass::Unknown);
        assert!(registry.detail.ends_with("Registry"));
        assert!(find("variable").detail.contains("nicht erkannt"));
        assert!(find("private-kit").source == SourceClass::Git && find("private-kit").source_host.is_none());
        assert!(!items.iter().any(|i| i.name == "out"), "comments are not code");
        assert_eq!(items.len(), 13);
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        let debug = format!("{items:?}");
        assert!(!debug.contains("s3cr3t") && !debug.contains("git.corp.example") && !debug.contains("deploy"));
    }

    #[test]
    fn package_resolved_versions_one_to_three() {
        let v1 = r#"{
  "object": {
    "pins": [
      {
        "package": "ArgumentParser",
        "repositoryURL": "https://github.com/apple/swift-argument-parser",
        "state": { "branch": null, "revision": "e1465042f195f374b94f915ba8ca49de24300a0d", "version": "1.0.2" }
      },
      {
        "package": "Develop",
        "repositoryURL": "https://user:hunter2@git.corp.example/x/develop.git",
        "state": { "branch": "main", "revision": "0123456789abcdef0123456789abcdef01234567", "version": null }
      }
    ]
  },
  "version": 1
}"#;
        let items = SwiftPm.parse_file(&file("Package.resolved", v1)).unwrap();
        assert_eq!(items[0].detail, "Package.resolved v1 · 2 Pins");
        let parser = items.iter().find(|i| i.name == "swift-argument-parser").unwrap();
        assert_eq!((parser.version.as_deref(), parser.source), (Some("1.0.2"), SourceClass::Git));
        let develop = items.iter().find(|i| i.name == "develop").unwrap();
        assert_eq!(develop.version.as_deref(), Some("main"));
        assert_eq!(develop.detail, "aufgelöst laut Package.resolved · Branch main · Revision 0123456789ab");
        assert!(!format!("{items:?}").contains("hunter2"));

        let v3 = r#"{
  "originHash" : "5f0bd8b4b8e7e3a3c1b0e8f9a2d4c6e8b0a2c4e6f8a0b2c4d6e8f0a2b4c6d8e0",
  "pins" : [
    {
      "identity" : "swift-nio",
      "kind" : "remoteSourceControl",
      "location" : "https://github.com/apple/swift-nio.git",
      "state" : { "revision" : "fc63f0cf4e55a4597407a9fc95b16a2bc44b4982", "version" : "2.64.0" }
    },
    {
      "identity" : "localkit",
      "kind" : "localSourceControl",
      "location" : "C:\\Users\\dev\\LocalKit",
      "state" : { "revision" : "0123456789abcdef0123456789abcdef01234567" }
    },
    {
      "identity" : "mona.linkedlist",
      "kind" : "registry",
      "location" : "",
      "state" : { "version" : "1.2.0" }
    }
  ],
  "version" : 3
}"#;
        let items = SwiftPm.parse_file(&file("Package.resolved", v3)).unwrap();
        assert_eq!(items.len(), 4);
        let local = items.iter().find(|i| i.name == "localkit").unwrap();
        assert_eq!((local.source, local.version.as_deref()), (SourceClass::Path, Some("0123456789ab")));
        assert_eq!(local.detail, "aufgelöst laut Package.resolved · Revision 0123456789ab");
        let registry = items.iter().find(|i| i.name == "mona.linkedlist").unwrap();
        assert_eq!((registry.source, registry.version.as_deref()), (SourceClass::Unknown, Some("1.2.0")));
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Resolved)));
        assert!(!format!("{items:?}").contains("Users"), "local paths are never stored");

        let v2 = r#"{"pins":[{"identity":"swift-log","kind":"remoteSourceControl","location":"https://github.com/apple/swift-log.git","state":{"revision":"532d8b529501fb73a2455b179e0bbb6d49b652ed","version":"1.5.3"}}],"version":2}"#;
        let items = SwiftPm.parse_file(&file("Package.resolved", v2)).unwrap();
        assert_eq!(items[1].version.as_deref(), Some("1.5.3"));
    }

    #[test]
    fn broken_or_unknown_resolved_files_are_errors() {
        assert!(SwiftPm.parse_file(&file("Package.resolved", "{\"pins\": [")).is_err());
        assert!(SwiftPm.parse_file(&file("Package.resolved", "{\"version\": 9, \"graph\": {}}")).is_err());
    }

    #[test]
    fn build_folder_beside_a_manifest_lists_checkouts() {
        assert!(SwiftPm.claim_dir(".build", &[], &[]).is_none());
        assert!(SwiftPm.claim_dir("build", &["Package.swift".into()], &[]).is_none());
        let claim = SwiftPm.claim_dir(".build", &["Package.swift".into()], &["checkouts".into()]).unwrap();
        let mut dir = FakeDir::default();
        dir.dirs.insert(
            "checkouts".into(),
            vec![("swift-nio".into(), true), ("swift-log".into(), true), (".DS_Store".into(), false)],
        );
        let items = SwiftPm.inspect_dir(claim, ".build", &mut dir).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].category, ItemCategory::Output);
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Installed) && i.version.is_none()));
    }

    #[test]
    fn repository_names_never_include_credentials() {
        assert_eq!(repo_name("https://github.com/apple/swift-nio.git"), Some("swift-nio"));
        assert_eq!(repo_name("https://user:pw@host.example/org/kit/"), Some("kit"));
        assert_eq!(repo_name("https://user:pw@host.example"), None);
        assert_eq!(repo_name("git@github.com:org/ssh-kit.git"), Some("ssh-kit"));
        assert_eq!(repo_name("../Local"), Some("Local"));
        assert_eq!(repo_name("C:\\dev\\Kit"), Some("Kit"));
        assert_eq!(repo_name("../"), None);
        assert_eq!(commit("0123456789abcdef0123456789abcdef01234567"), Some("0123456789ab"));
        assert_eq!(commit("v1.2.3"), None);
    }
}
