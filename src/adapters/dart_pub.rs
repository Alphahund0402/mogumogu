//! pub (Dart/Flutter): pubspec.yaml and pubspec_overrides.yaml (declared),
//! pubspec.lock (resolved), `.dart_tool` (environment) and Flutter's `build`
//! folder (output). YAML is read as the block subset of `parsers::lines`;
//! no dart or flutter command runs. PUB_HOSTED_URL is not read, so a
//! dependency without a hosted URL keeps an unknown source, and
//! `.dart_tool/package_config.json` (absolute paths into the user's pub
//! cache) is never opened.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::lines::{self, YamlEntry};

pub struct Pub;

const PUBLIC: &[&str] = &["pub.dev", "pub.dartlang.org"];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "pub",
    name: "pub (Dart/Flutter)",
    markers: &["pubspec.yaml", "pubspec.lock", "pubspec_overrides.yaml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "pubspec.yaml (dependencies, dev_dependencies, dependency_overrides)",
        "pubspec_overrides.yaml",
        "pubspec.lock",
        ".dart_tool (Umgebung), build/ (Ausgabe)",
    ],
    limits: "Kein dart- oder flutter-Aufruf; YAML nur als Blockteilmenge; PUB_HOSTED_URL und .dart_tool/package_config.json (absolute Pfade) werden nicht gelesen – ohne Server-URL bleibt die Quelle unbekannt.",
};

const SECTIONS: &[&str] = &["dependencies", "dev_dependencies", "dependency_overrides"];

impl Adapter for Pub {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name.to_ascii_lowercase().as_str() {
            "pubspec.lock" => bounded(lockfile(file)?),
            "pubspec_overrides.yaml" => bounded(pubspec(file, true)),
            _ => bounded(pubspec(file, false)),
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], children: &[String]) -> Option<DirClaim> {
        if !siblings.iter().any(|s| s.eq_ignore_ascii_case("pubspec.yaml")) {
            return None;
        }
        match name {
            ".dart_tool" => Some(DirClaim { adapter: "pub", category: ItemCategory::Environment, label: ".dart_tool" }),
            // A `build` package of a pub workspace has its own pubspec.
            "build" if !children.iter().any(|c| c.eq_ignore_ascii_case("pubspec.yaml")) => {
                Some(DirClaim { adapter: "pub", category: ItemCategory::Output, label: "Flutter-Buildausgabe" })
            }
            _ => None,
        }
    }

    fn inspect_dir(&self, claim: DirClaim, rel_path: &str, _access: &mut dyn DirAccess) -> AdapterResult {
        Ok(vec![if claim.category == ItemCategory::Environment {
            InventoryItem::new(ItemCategory::Environment, "pub", ".dart_tool", rel_path)
                .detail("Werkzeugzustand von pub und Flutter; package_config.json nicht gelesen (absolute Pfade)")
        } else {
            InventoryItem::new(ItemCategory::Output, "pub", "build", rel_path)
                .detail("Generierte Buildausgabe; Größe erst nach Messung bekannt")
        }])
    }
}

/// Visits the entries of a block YAML document with the keys leading to
/// them, its own key last. Entries nested deeper than 16 keys are skipped.
fn walk<'a>(text: &'a str, mut visit: impl FnMut(&[&'a str], &YamlEntry<'a>)) {
    let mut stack: Vec<(usize, &'a str)> = Vec::new();
    let mut path: Vec<&'a str> = Vec::new();
    for entry in lines::yaml_entries(text) {
        while stack.last().is_some_and(|(indent, _)| *indent >= entry.indent) {
            stack.pop();
        }
        if let Some(key) = entry.key {
            stack.push((entry.indent, key));
        }
        if stack.len() > 16 {
            continue;
        }
        path.clear();
        path.extend(stack.iter().map(|(_, key)| *key));
        visit(&path, &entry);
    }
}

/// Pairs of a one-line flow mapping such as `{sdk: flutter}`; nested
/// collections are not split.
fn flow_map(value: &str) -> Option<Vec<(&str, &str)>> {
    let inner = value.trim().strip_prefix('{')?.strip_suffix('}')?;
    if inner.contains(['{', '[']) {
        return None;
    }
    Some(
        inner
            .split(',')
            .filter_map(|pair| pair.split_once(':'))
            .map(|(key, value)| (unquote(key.trim()), unquote(value.trim())))
            .collect(),
    )
}

fn unquote(value: &str) -> &str {
    ['"', '\''].iter().find_map(|quote| value.strip_prefix(*quote)?.strip_suffix(*quote)).unwrap_or(value)
}

/// One dependency of a pubspec section while its attributes are read.
struct Declared<'a> {
    section: &'a str,
    name: &'a str,
    constraint: Option<&'a str>,
    sdk: Option<&'a str>,
    path: bool,
    git: bool,
    hosted: Option<&'a str>,
    /// A form this reader does not interpret (nested flow mapping).
    raw: bool,
}

impl<'a> Declared<'a> {
    fn new(section: &'a str, name: &'a str, value: &'a str) -> Self {
        let mut declared =
            Self { section, name, constraint: None, sdk: None, path: false, git: false, hosted: None, raw: false };
        if value.starts_with('{') {
            match flow_map(value) {
                Some(pairs) => pairs.into_iter().for_each(|(key, value)| declared.attribute(key, None, value)),
                None => declared.raw = true,
            }
        } else if !value.is_empty() {
            declared.constraint = Some(value);
        }
        declared
    }

    /// `sdk: flutter`, `path: …`, `git: …` (or its map), `hosted: …` (or
    /// its map with `url`) and `version: …`.
    fn attribute(&mut self, key: &str, sub: Option<&str>, value: &'a str) {
        match (key, sub) {
            ("sdk", None) => self.sdk = Some(value),
            ("path", None) => self.path = true,
            ("git", _) => self.git = true,
            ("hosted", None) if !value.is_empty() => self.hosted = Some(value),
            ("hosted", Some("url")) => self.hosted = Some(value),
            ("version", None) => self.constraint = Some(value),
            _ => {}
        }
    }
}

fn pubspec(file: &FoundFile<'_>, overrides: bool) -> Vec<InventoryItem> {
    let mut declared: Vec<Declared> = Vec::new();
    let (mut name, mut version, mut sdk) = (None, None, None);
    let (mut flutter, mut members, mut member) = (false, 0, false);
    walk(file.content, |path, entry| match path {
        ["name"] => name = Some(entry.value),
        ["version"] => version = Some(entry.value),
        ["environment", "sdk"] => sdk = Some(entry.value),
        ["flutter"] => flutter = true,
        ["resolution"] => member |= entry.value == "workspace",
        ["workspace"] if entry.key.is_none() => members += 1,
        [section] if SECTIONS.contains(section) => {
            // `dependencies: {http: ^1.2.0}` in flow style.
            for (dependency, value) in flow_map(entry.value).into_iter().flatten() {
                declared.push(Declared::new(section, dependency, value));
            }
        }
        [section, dependency] if SECTIONS.contains(section) => {
            declared.push(Declared::new(section, dependency, entry.value));
        }
        [section, dependency, key, rest @ ..] if SECTIONS.contains(section) => {
            if let Some(last) = declared.last_mut().filter(|d| d.name == *dependency && d.section == *section) {
                last.attribute(key, rest.first().copied(), entry.value);
            }
        }
        _ => {}
    });
    let mut detail = if overrides {
        "Lokale Überschreibungen".to_string()
    } else {
        format!("Paket {}", name.filter(|n| !n.is_empty()).unwrap_or("ohne Namen"))
    };
    if let Some(version) = version.filter(|v| !v.is_empty()) {
        detail.push_str(&format!(" {version}"));
    }
    if flutter || declared.iter().any(|d| d.sdk == Some("flutter")) {
        detail.push_str(" · Flutter");
    }
    if members > 0 {
        detail.push_str(&format!(" · Workspace mit {members} Mitgliedern"));
    }
    if member {
        detail.push_str(" · Workspace-Mitglied");
    }
    if let Some(sdk) = sdk.filter(|s| !s.is_empty()) {
        detail.push_str(&format!(" · Dart-SDK {sdk}"));
    }
    let mut items = vec![manifest("pub", file, detail)];
    items.extend(declared.into_iter().map(|dependency| declared_item(file, dependency)));
    items
}

fn declared_item(file: &FoundFile<'_>, dependency: Declared<'_>) -> InventoryItem {
    let (source, host, origin) = if let Some(sdk) = dependency.sdk {
        (SourceClass::Unknown, None, format!("SDK {sdk}"))
    } else if dependency.path {
        (SourceClass::Path, None, "lokaler Pfad".to_string())
    } else if dependency.git {
        (SourceClass::Git, None, "Git".to_string())
    } else if let Some(url) = dependency.hosted {
        let (class, host) = classify_url(url, PUBLIC);
        let origin = if class == SourceClass::PublicRegistry { String::new() } else { "eigener Server".to_string() };
        (class, host, origin)
    } else {
        (SourceClass::Unknown, None, String::new())
    };
    let constraint = dependency.constraint.filter(|c| !matches!(*c, "" | "null" | "~"));
    let mut parts = Vec::new();
    if !origin.is_empty() {
        parts.push(origin);
    }
    match constraint {
        Some(constraint) => parts.push(format!("Spezifikation {constraint}")),
        None if dependency.raw => parts.push("Angabe nicht ausgewertet".to_string()),
        None if parts.is_empty() => parts.push("ohne Versionsangabe".to_string()),
        None => {}
    }
    let label = match dependency.section {
        "dev_dependencies" => "dev",
        "dependency_overrides" => "override",
        _ => "",
    };
    match label {
        "dev" => parts.push("dev".to_string()),
        "override" => parts.push("Override".to_string()),
        _ => {}
    }
    let mut item = InventoryItem::new(ItemCategory::Package, "pub", dependency.name, file.rel_path())
        .version(constraint.filter(|c| exact(c)).unwrap_or_default())
        .state(InstallState::Declared)
        .source(source, host)
        .detail(parts.join(" · "));
    item.evidence.context = if dependency.sdk.is_some() { "sdk" } else { label }.into();
    item
}

/// A constraint naming one version: `1.2.3`, `2.0.0-dev.1`, `1.0.0+4`.
fn exact(constraint: &str) -> bool {
    let mut numbers = constraint.split(['-', '+']).next().unwrap_or("").split('.');
    let release = numbers.by_ref().take(3).filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).count();
    release == 3
        && numbers.next().is_none()
        && constraint.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

/// One package of pubspec.lock while its fields are read.
#[derive(Default)]
struct Locked<'a> {
    name: &'a str,
    dependency: &'a str,
    description: &'a str,
    url: Option<&'a str>,
    source: Option<&'a str>,
    version: Option<&'a str>,
}

fn lockfile(file: &FoundFile<'_>) -> AdapterResult {
    if ["<<<<<<<", "=======", ">>>>>>>"].iter().any(|marker| file.content.contains(marker)) {
        return Err("pubspec.lock enthält Konfliktmarker".into());
    }
    let mut packages: Vec<Locked> = Vec::new();
    let mut sdks: Vec<(&str, &str)> = Vec::new();
    let mut known = false;
    walk(file.content, |path, entry| match path {
        ["packages" | "sdks"] => known = true,
        ["packages", name] => packages.push(Locked { name, ..Locked::default() }),
        ["packages", name, field, rest @ ..] => {
            let Some(package) = packages.last_mut().filter(|p| p.name == *name) else { return };
            match (*field, rest) {
                ("dependency", []) => package.dependency = entry.value,
                ("description", []) => {
                    package.description = entry.value;
                    let pairs = flow_map(entry.value).unwrap_or_default();
                    package.url = pairs.into_iter().find(|(key, _)| *key == "url").map(|(_, url)| url);
                }
                ("description", ["url"]) => package.url = Some(entry.value),
                ("source", []) => package.source = Some(entry.value),
                ("version", []) => package.version = Some(entry.value),
                _ => {}
            }
        }
        ["sdks", sdk] => sdks.push((*sdk, entry.value)),
        _ => {}
    });
    if !known {
        return Err("pubspec.lock ohne packages- und sdks-Abschnitt".into());
    }
    let mut detail = String::from("Lockfile");
    for (sdk, constraint) in &sdks {
        let sdk = match *sdk {
            "dart" => "Dart",
            "flutter" => "Flutter",
            other => other,
        };
        detail.push_str(&format!(" · {sdk} {constraint}"));
    }
    let mut items = vec![manifest("pub", file, detail)];
    for package in packages {
        let (Some(source), Some(version)) = (package.source, package.version) else {
            return Err(format!("pubspec.lock: Eintrag {} ohne source oder version", package.name));
        };
        let (class, host, origin) = match source {
            "hosted" => {
                let (class, host) = package.url.map_or((SourceClass::Unknown, None), |url| classify_url(url, PUBLIC));
                (class, host, String::new())
            }
            "git" => (SourceClass::Git, None, " · Git".to_string()),
            "path" => (SourceClass::Path, None, " · lokaler Pfad".to_string()),
            "sdk" => (SourceClass::Unknown, None, format!(" · SDK {}", package.description)),
            other => (SourceClass::Unknown, None, format!(" · Quelle {other}")),
        };
        let (context, relation) = match package.dependency {
            "direct main" => ("", " · direkt"),
            "direct dev" => ("dev", " · dev"),
            "direct overridden" => ("override", " · Override"),
            "transitive" => ("transitive", " · transitiv"),
            _ => ("", ""),
        };
        // An SDK package carries the placeholder version 0.0.0.
        let version = if source == "sdk" { "" } else { version };
        let mut item = InventoryItem::new(ItemCategory::Package, "pub", package.name, file.rel_path())
            .version(version)
            .state(InstallState::Resolved)
            .source(class, host)
            .detail(format!("aufgelöst laut pubspec.lock{relation}{origin}"));
        item.evidence.context = context.into();
        items.push(item);
    }
    bounded(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const PUBSPEC: &str = r#"name: my_app
description: "A new Flutter project."
publish_to: 'none'
version: 1.0.0+1

environment:
  sdk: '>=3.2.0 <4.0.0'
  flutter: ">=3.16.0"

dependencies:
  flutter:
    sdk: flutter
  flutter_localizations: {sdk: flutter}
  cupertino_icons: ^1.0.6
  http: '>=1.1.0 <2.0.0'
  provider: any
  intl:
  exact_pkg: 2.0.1
  transmogrify:
    hosted: https://deploy:s3cr3tpass@pub.corp.example
    version: ^1.4.0
  legacy_hosted:
    hosted:
      name: legacy_hosted
      url: https://pub.dev
    version: ^0.3.0
  kittens:
    git: https://github.com/munificent/kittens.git
  kittens2:
    git:
      url: git@github.com:munificent/kittens.git
      ref: some-branch
      path: path/to/kittens
  local_pkg:
    path: ../local_pkg

dev_dependencies:
  flutter_test:
    sdk: flutter
  flutter_lints: ^3.0.0
  build_runner: ^2.4.6 # code generation

dependency_overrides:
  transmogrify:
    path: ../transmogrify_patch/

flutter:
  uses-material-design: true
  assets:
    - images/a_dot_burr.jpeg
"#;

    #[test]
    fn pubspec_sections_map_forms_and_sdk_dependencies() {
        let items = Pub.parse_file(&file("pubspec.yaml", PUBSPEC)).unwrap();
        assert_eq!(items[0].detail, "Paket my_app 1.0.0+1 · Flutter · Dart-SDK >=3.2.0 <4.0.0");
        assert_eq!(items.len(), 17, "{items:#?}");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        let find = |name: &str, context: &str| {
            items
                .iter()
                .find(|i| i.name == name && i.evidence.context == context)
                .unwrap_or_else(|| panic!("{name} {context}"))
        };
        let flutter = find("flutter", "sdk");
        assert_eq!(
            (flutter.source, flutter.version.as_deref(), flutter.detail.as_str()),
            (SourceClass::Unknown, None, "SDK flutter")
        );
        assert_eq!(find("flutter_localizations", "sdk").detail, "SDK flutter");
        let icons = find("cupertino_icons", "");
        assert_eq!((icons.source, icons.detail.as_str()), (SourceClass::Unknown, "Spezifikation ^1.0.6"));
        assert_eq!(find("http", "").detail, "Spezifikation >=1.1.0 <2.0.0");
        assert_eq!(find("provider", "").detail, "Spezifikation any");
        assert_eq!(find("intl", "").detail, "ohne Versionsangabe");
        assert_eq!(find("exact_pkg", "").version.as_deref(), Some("2.0.1"));
        let hosted = find("transmogrify", "");
        assert_eq!(
            (hosted.source, hosted.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("pub.corp.example"))
        );
        assert_eq!(hosted.detail, "eigener Server · Spezifikation ^1.4.0");
        let legacy = find("legacy_hosted", "");
        assert_eq!((legacy.source, legacy.source_host.as_deref()), (SourceClass::PublicRegistry, Some("pub.dev")));
        assert_eq!((find("kittens", "").source, find("kittens2", "").source), (SourceClass::Git, SourceClass::Git));
        assert_eq!(find("local_pkg", "").source, SourceClass::Path);
        assert_eq!(find("flutter_test", "sdk").detail, "SDK flutter · dev");
        assert_eq!(find("flutter_lints", "dev").detail, "Spezifikation ^3.0.0 · dev");
        assert_eq!(find("build_runner", "dev").detail, "Spezifikation ^2.4.6 · dev");
        let override_ = find("transmogrify", "override");
        assert_eq!((override_.source, override_.detail.as_str()), (SourceClass::Path, "lokaler Pfad · Override"));
        assert!(!items.iter().any(|i| i.name == "sdk" || i.name == "uses-material-design" || i.name == "assets"));
        assert!(!format!("{items:?}").contains("s3cr3tpass"));
    }

    #[test]
    fn workspaces_and_local_overrides() {
        let root = "name: _\npublish_to: none\nenvironment:\n  sdk: ^3.6.0\nworkspace:\n  - packages/helper\n  - packages/client_package\n";
        let items = Pub.parse_file(&file("pubspec.yaml", root)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].detail, "Paket _ · Workspace mit 2 Mitgliedern · Dart-SDK ^3.6.0");
        let member = "name: helper\nresolution: workspace\ndependencies: {path: ^1.9.0, meta: ^1.15.0}\n";
        let items = Pub.parse_file(&file("pubspec.yaml", member)).unwrap();
        assert_eq!(items[0].detail, "Paket helper · Workspace-Mitglied");
        assert_eq!(items[1].name, "path");
        assert_eq!(items[2].detail, "Spezifikation ^1.15.0");
        let overrides = "dependency_overrides:\n  my_dep:\n    path: ../my_dep\n  pinned: '3.2.1'\n";
        let items = Pub.parse_file(&file("pubspec_overrides.yaml", overrides)).unwrap();
        assert_eq!(items[0].detail, "Lokale Überschreibungen");
        assert_eq!(items[1].evidence.context, "override");
        assert_eq!(items[2].version.as_deref(), Some("3.2.1"));
        let nested = "dependencies:\n  odd: {git: {url: x}}\n";
        let items = Pub.parse_file(&file("pubspec.yaml", nested)).unwrap();
        assert_eq!(items[1].detail, "Angabe nicht ausgewertet");
    }

    const LOCK: &str = r#"# Generated by pub
# See https://dart.dev/tools/pub/glossary#lockfile
packages:
  async:
    dependency: transitive
    description:
      name: async
      sha256: "947bfcf187f74dbc5e146c9eb9c0f10c9f8b30743e341481c1e2ed3ecc18c20c"
      url: "https://pub.dev"
    source: hosted
    version: "2.11.0"
  build_runner:
    dependency: "direct dev"
    description:
      name: build_runner
      sha256: "581bacf68f89ec8792f5e5a0b2c4decd1c948e97ce659dc783688c8a88fbec21"
      url: "https://pub.dev"
    source: hosted
    version: "2.4.8"
  flutter:
    dependency: "direct main"
    description: flutter
    source: sdk
    version: "0.0.0"
  kittens:
    dependency: "direct main"
    description:
      path: "."
      ref: some-branch
      resolved-ref: "0123456789abcdef0123456789abcdef01234567"
      url: "https://deploy:s3cr3tpass@git.corp.example/kittens.git"
    source: git
    version: "0.3.1"
  local_pkg:
    dependency: "direct main"
    description:
      path: "../local_pkg"
      relative: true
    source: path
    version: "1.0.0"
  old_hosted:
    dependency: transitive
    description:
      name: old_hosted
      url: "https://pub.dartlang.org"
    source: hosted
    version: "1.0.2"
  transmogrify:
    dependency: "direct overridden"
    description:
      name: transmogrify
      sha256: "4a1b2c"
      url: "https://deploy:s3cr3tpass@pub.corp.example"
    source: hosted
    version: "1.4.2"
sdks:
  dart: ">=3.2.0 <4.0.0"
  flutter: ">=3.16.0"
"#;

    #[test]
    fn lockfile_dependency_kinds_and_sources() {
        let items = Pub.parse_file(&file("pubspec.lock", LOCK)).unwrap();
        assert_eq!(items[0].detail, "Lockfile · Dart >=3.2.0 <4.0.0 · Flutter >=3.16.0");
        assert_eq!(items.len(), 8);
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Resolved)));
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let async_ = find("async");
        assert_eq!((async_.source, async_.source_host.as_deref()), (SourceClass::PublicRegistry, Some("pub.dev")));
        assert_eq!((async_.version.as_deref(), async_.evidence.context.as_str()), (Some("2.11.0"), "transitive"));
        assert_eq!(find("build_runner").evidence.context, "dev");
        let flutter = find("flutter");
        assert_eq!((flutter.version.as_deref(), flutter.source), (None, SourceClass::Unknown));
        assert_eq!(flutter.detail, "aufgelöst laut pubspec.lock · direkt · SDK flutter");
        let kittens = find("kittens");
        assert_eq!((kittens.source, kittens.source_host.as_deref()), (SourceClass::Git, None));
        assert_eq!(find("local_pkg").source, SourceClass::Path);
        assert_eq!(find("old_hosted").source_host.as_deref(), Some("pub.dartlang.org"));
        let overridden = find("transmogrify");
        assert_eq!(overridden.evidence.context, "override");
        assert_eq!(
            (overridden.source, overridden.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("pub.corp.example"))
        );
        assert!(!format!("{items:?}").contains("s3cr3tpass"));
    }

    #[test]
    fn broken_or_foreign_lockfiles_are_errors() {
        assert!(Pub.parse_file(&file("pubspec.lock", "<<<<<<< HEAD\npackages:\n")).is_err());
        assert!(Pub.parse_file(&file("pubspec.lock", "random: text\n")).is_err());
        let partial = "packages:\n  async:\n    dependency: transitive\n    source: hosted\n";
        assert!(Pub.parse_file(&file("pubspec.lock", partial)).is_err());
        let empty = "# Generated by pub\npackages: {}\nsdks:\n  dart: \">=3.0.0 <4.0.0\"\n";
        assert_eq!(Pub.parse_file(&file("pubspec.lock", empty)).unwrap().len(), 1);
    }

    #[test]
    fn tool_folders_only_beside_a_pubspec() {
        assert!(Pub.claim_dir(".dart_tool", &["main.dart".into()], &[]).is_none());
        let pubspec = ["pubspec.yaml".to_string(), "pubspec.lock".to_string()];
        let tool = Pub.claim_dir(".dart_tool", &pubspec, &["package_config.json".into()]).unwrap();
        assert_eq!(tool.category, ItemCategory::Environment);
        let mut dir = FakeDir::default();
        dir.files.insert("package_config.json".into(), r#"{"rootUri": "file:///C:/Users/someone/AppData"}"#.into());
        let items = Pub.inspect_dir(tool, ".dart_tool", &mut dir).unwrap();
        assert_eq!(items.len(), 1);
        assert!(!format!("{items:?}").contains("someone"));
        let build = Pub.claim_dir("build", &pubspec, &["app".into(), ".last_build_id".into()]).unwrap();
        assert_eq!(build.category, ItemCategory::Output);
        assert!(Pub.claim_dir("build", &pubspec, &["pubspec.yaml".into(), "lib".into()]).is_none());
    }
}
