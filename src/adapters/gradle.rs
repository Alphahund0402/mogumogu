//! Gradle: version catalogs `libs.versions.toml` and string coordinates in
//! build scripts (declared), settings scripts (included projects and builds
//! as edges, plugin versions, imported catalogs), dependency lockfiles
//! (resolved). Scripts are code: they are tokenized and matched as text,
//! never evaluated, so versions from variables, BOMs or plugins and
//! dynamic `include` calls stay unknown. Repositories appear as hosts only;
//! gradle.properties (often credentials) is never read. The `build` folder
//! beside a build or settings script is an output and is not walked.
use super::maven::Hosts;
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency};
use crate::parsers;
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

pub struct Gradle;

const PUBLIC: &[&str] = &[
    "repo.maven.apache.org",
    "repo1.maven.org",
    "maven.google.com",
    "dl.google.com",
    "plugins.gradle.org",
    "jcenter.bintray.com",
];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "gradle",
    name: "Gradle",
    markers: &[
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
        "libs.versions.toml",
        "gradle.lockfile",
        "buildscript-gradle.lockfile",
    ],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "libs.versions.toml",
        "build.gradle(.kts): Koordinaten als Text",
        "settings.gradle(.kts): include, includeBuild, Plugins, Repositories als Text",
        "gradle.lockfile",
    ],
    limits: "Kein Gradle-Aufruf; Build- und Settings-Skripte werden nicht ausgewertet – Versionen aus Variablen, BOMs oder Plugins und dynamische include-Aufrufe bleiben unbekannt; Repositories nur als Hosts; gradle.properties wird nie gelesen.",
};

/// A quoted `group:artifact[:version][@ext]` coordinate.
static COORDINATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"["']([A-Za-z0-9_.\-]+):([A-Za-z0-9_.\-]+)(?::([^"':@\s]+))?(?:@[A-Za-z0-9]+)?["']"#)
        .expect("valid regex")
});
/// The configuration a dependency line starts with, Groovy or Kotlin DSL.
static CONFIGURATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*"?([A-Za-z]+)"?\s*[( ]"#).expect("valid regex"));
/// `id("com.android.application") version "8.7.3"` in a plugins block.
static PLUGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bid\s*\(?\s*["']([A-Za-z0-9_.\-]+)["']\s*\)?\s+version\s*\(?\s*["']([^"']+)["']"#)
        .expect("valid regex")
});
/// `kotlin("jvm") version "2.1.0"`, short for `org.jetbrains.kotlin.jvm`.
static KOTLIN_PLUGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bkotlin\s*\(\s*"([A-Za-z0-9_.\-]+)"\s*\)\s+version\s*\(?\s*"([^"]+)""#).expect("valid regex")
});
static ROOT_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\brootProject\.name\s*=\s*["']([^"'$]+)["']"#).expect("valid regex"));

/// Repository shorthands and how they are shown.
const SHORTHANDS: &[(&str, &str)] = &[
    ("mavenCentral", "mavenCentral"),
    ("google", "google"),
    ("gradlePluginPortal", "gradlePluginPortal"),
    ("mavenLocal", "mavenLocal"),
    ("jcenter", "jcenter (abgekündigt)"),
];

impl Adapter for Gradle {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name {
            "libs.versions.toml" => bounded(catalog(file)?),
            "gradle.lockfile" | "buildscript-gradle.lockfile" => bounded(lockfile(file)),
            "settings.gradle" | "settings.gradle.kts" => bounded(settings(file)),
            _ => bounded(script(file)),
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let script = siblings.iter().any(|s| {
            matches!(s.as_str(), "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts")
        });
        (name == "build" && script).then_some(DirClaim {
            adapter: "gradle",
            category: ItemCategory::Output,
            label: "Gradle-Buildausgabe",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, _access: &mut dyn DirAccess) -> AdapterResult {
        Ok(vec![
            InventoryItem::new(ItemCategory::Output, "gradle", "build", rel_path)
                .detail("Generierte Buildausgabe; Größe erst nach Messung bekannt"),
        ])
    }
}

fn declared(file: &FoundFile<'_>, name: &str, version: Option<&str>, context: &str, detail: String) -> InventoryItem {
    let mut item = InventoryItem::new(ItemCategory::Package, "gradle", name, file.rel_path())
        .version(version.unwrap_or_default())
        .state(InstallState::Declared)
        .detail(detail);
    item.evidence.context = context.into();
    item
}

/// `implementation`, `testImplementation`, `kapt`, `classpath` …
fn is_configuration(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    matches!(lower.as_str(), "classpath" | "compile" | "runtime" | "lintchecks" | "corelibrarydesugaring")
        || ["implementation", "api", "compileonly", "runtimeonly", "kapt", "ksp", "annotationprocessor"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
}

/// A plugin with a version on this line: `id("x") version "1"` or
/// `kotlin("jvm") version "2.1.0"`.
fn plugin(line: &str) -> Option<(String, String)> {
    if let Some(plugin) = PLUGIN.captures(line) {
        return Some((plugin[1].to_string(), plugin[2].to_string()));
    }
    KOTLIN_PLUGIN.captures(line).map(|kotlin| (format!("org.jetbrains.kotlin.{}", &kotlin[1]), kotlin[2].to_string()))
}

fn script(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    let mut catalog_refs = 0;
    for line in file.content.lines().take(200_000) {
        let code = line.trim_start();
        if code.starts_with("//") || code.starts_with('*') || code.starts_with("/*") {
            continue;
        }
        catalog_refs += line.matches("libs.").count();
        if let Some((id, version)) = plugin(line) {
            let detail = format!("Gradle-Plugin · Spezifikation {version}");
            items.push(declared(file, &id, Some(&version), "plugin", detail));
            continue;
        }
        let platform = line.contains("platform(");
        let configuration = CONFIGURATION.captures(line).map(|c| c[1].to_string()).filter(|w| is_configuration(w));
        let Some(context) = configuration.or_else(|| platform.then(|| "platform".to_string())) else { continue };
        for coordinate in COORDINATE.captures_iter(line) {
            let name = format!("{}:{}", &coordinate[1], &coordinate[2]);
            let version = coordinate.get(3).map(|v| v.as_str()).filter(|v| !v.contains('$'));
            let spec = match (coordinate.get(3), version) {
                (_, Some(version)) => format!("Spezifikation {version}"),
                (Some(_), None) => "Version aus Variable".into(),
                (None, _) => "Version aus BOM oder Katalog".into(),
            };
            let kind = if platform { " · Plattform/BOM" } else { "" };
            items.push(declared(file, &name, version, &context, format!("{spec} · {context}{kind}")));
        }
    }
    let mut detail = String::from("Gradle-Buildskript (nicht ausgeführt)");
    if catalog_refs > 0 {
        detail.push_str(&format!(" · {catalog_refs} Verweise auf den Versionskatalog"));
    }
    if let Some(label) = walk(file.content).repositories() {
        detail.push_str(&format!(" · Repositories: {label}"));
    }
    items.insert(0, manifest("gradle", file, detail));
    items
}

/// One lexical token of Groovy or Kotlin code outside comments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token<'a> {
    Word(&'a str),
    /// The raw content of a string literal.
    Text(&'a str),
    Open(u8),
    Close(u8),
    Other(u8),
}

/// Tokens of one line; `comment` carries an open `/* … */` across lines.
fn tokens<'a>(line: &'a str, comment: &mut bool, out: &mut Vec<Token<'a>>) {
    out.clear();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if *comment {
            match line[i..].find("*/") {
                Some(end) => {
                    *comment = false;
                    i += end + 2;
                    continue;
                }
                None => return,
            }
        }
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => return,
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                *comment = true;
                i += 2;
            }
            quote @ (b'"' | b'\'') => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != quote {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                let end = end.min(bytes.len());
                out.push(Token::Text(&line[start..end]));
                i = end + 1;
            }
            b @ (b'(' | b'{') => {
                out.push(Token::Open(b));
                i += 1;
            }
            b @ (b')' | b'}') => {
                out.push(Token::Close(b));
                i += 1;
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(Token::Word(&line[start..i]));
            }
            b if b.is_ascii_punctuation() => {
                out.push(Token::Other(b));
                i += 1;
            }
            _ => i += 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Include {
    Project,
    Build,
}

/// Facts collected from one script by block structure.
#[derive(Default)]
struct Walk<'a> {
    hosts: Hosts,
    shorthands: Vec<&'static str>,
    includes: Vec<(Include, &'a str)>,
    plugins: Vec<(String, String, bool)>,
    catalogs: bool,
    catalog_entries: Vec<(&'a str, &'static str)>,
    root: Option<&'a str>,
}

/// Entries kept per list of a walk.
const MAX_ENTRIES: usize = 10_000;

impl Walk<'_> {
    fn repositories(&self) -> Option<String> {
        let hosts = self.hosts.label();
        if self.shorthands.is_empty() {
            return hosts;
        }
        let mut label = self.shorthands.join(", ");
        if let Some(hosts) = hosts {
            label.push_str(&format!(", {hosts}"));
        }
        Some(label)
    }
}

const MAX_NESTING: usize = 64;

/// Walks a script once: block names (`repositories { … }`), call names of
/// open parentheses and string literals in order.
fn walk(text: &str) -> Walk<'_> {
    let mut facts = Walk::default();
    let mut blocks: Vec<&str> = Vec::new();
    let mut calls: Vec<&str> = Vec::new();
    let (mut deeper_blocks, mut deeper_calls) = (0_usize, 0_usize);
    let mut comment = false;
    let mut line_tokens = Vec::new();
    // An `include ':a', ':b'` call without parentheses, continued after a comma.
    let mut bare: Option<Include> = None;
    for line in text.lines().take(200_000) {
        let commented = comment;
        tokens(line, &mut comment, &mut line_tokens);
        if !commented && let Some(code) = line.split("//").next() {
            if let Some((id, version)) = plugin(code).filter(|_| facts.plugins.len() < MAX_ENTRIES) {
                let managed = blocks.contains(&"pluginManagement");
                facts.plugins.push((id, version, managed));
            }
            if let Some(name) = ROOT_NAME.captures(code).and_then(|c| c.get(1)) {
                facts.root = Some(name.as_str());
            }
        }
        let mut last = "";
        for token in &line_tokens {
            let in_repositories = blocks.contains(&"repositories") && !blocks.contains(&"publishing");
            match *token {
                Token::Word(word) => {
                    bare = match word {
                        "include" | "includeFlat" => Some(Include::Project),
                        "includeBuild" => Some(Include::Build),
                        _ => None,
                    };
                    if word == "versionCatalogs" {
                        facts.catalogs = true;
                    }
                    last = word;
                }
                Token::Open(brace) => {
                    if in_repositories
                        && let Some((_, label)) = SHORTHANDS.iter().find(|(name, _)| *name == last)
                        && !facts.shorthands.contains(label)
                    {
                        facts.shorthands.push(label);
                    }
                    let (stack, deeper) =
                        if brace == b'{' { (&mut blocks, &mut deeper_blocks) } else { (&mut calls, &mut deeper_calls) };
                    if stack.len() < MAX_NESTING {
                        stack.push(last);
                    } else {
                        *deeper += 1;
                    }
                    bare = None;
                    last = "";
                }
                Token::Close(brace) => {
                    let (stack, deeper) =
                        if brace == b'}' { (&mut blocks, &mut deeper_blocks) } else { (&mut calls, &mut deeper_calls) };
                    let name = if *deeper > 0 {
                        *deeper -= 1;
                        ""
                    } else {
                        stack.pop().unwrap_or("")
                    };
                    bare = None;
                    // `create("libs") {` names its block after the call.
                    last = if brace == b')' { name } else { "" };
                }
                Token::Text(text) => {
                    let call = calls.iter().rev().find_map(|c| match *c {
                        "include" | "includeFlat" => Some(Include::Project),
                        "includeBuild" => Some(Include::Build),
                        _ => None,
                    });
                    if let Some(kind) = call.or(bare) {
                        if facts.includes.len() < MAX_ENTRIES {
                            facts.includes.push((kind, text));
                        }
                    } else if in_repositories && (text.starts_with("http://") || text.starts_with("https://")) {
                        facts.hosts.add(text, PUBLIC);
                    } else if blocks.contains(&"versionCatalogs") && facts.catalog_entries.len() < MAX_ENTRIES {
                        match calls.last().copied() {
                            Some("from") => facts.catalog_entries.push((text, "catalog-import")),
                            Some("library") => facts.catalog_entries.push((text, "catalog")),
                            _ => {}
                        }
                    }
                    last = "";
                }
                // Commas separate the arguments of a bare include.
                Token::Other(b',') if bare.is_some() => {}
                Token::Other(_) => {
                    bare = None;
                    last = "";
                }
            }
        }
        if !matches!(line_tokens.last(), Some(Token::Other(b','))) {
            bare = None;
        }
    }
    facts
}

/// `:app` for `app` and `:app`; dynamic names are not kept.
fn project_path(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains(['$', ' ', '/', '\\']) {
        return None;
    }
    Some(if raw.starts_with(':') { raw.to_string() } else { format!(":{raw}") })
}

fn settings(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let facts = walk(file.content);
    let mut edges: Vec<PackageDependency> = Vec::new();
    let mut seen = BTreeSet::new();
    let (mut projects, mut builds) = (0, 0);
    for (kind, raw) in facts.includes.iter().copied() {
        let (name, kind) = match kind {
            Include::Project => (project_path(raw), "include"),
            Include::Build => (super::maven::relative_path(raw).filter(|p| !p.contains('$')), "includeBuild"),
        };
        let Some(name) = name else { continue };
        if edges.len() >= 5_000 || !seen.insert((kind, name.clone())) {
            continue;
        }
        if kind == "include" {
            projects += 1;
        } else {
            builds += 1;
        }
        edges.push(PackageDependency { name, kind: kind.into(), ..PackageDependency::default() });
    }
    let mut detail = String::from("Gradle-Settings (nicht ausgeführt)");
    if let Some(root) = facts.root {
        detail.push_str(&format!(" · Root-Projekt {root}"));
    }
    detail.push_str(&format!(" · {projects} Teilprojekte"));
    if builds > 0 {
        detail.push_str(&format!(" · {builds} eingebundene Builds"));
    }
    if facts.catalogs {
        detail.push_str(" · Versionskataloge deklariert");
    }
    // Last: the longest part, cut first when the label is bounded.
    if let Some(label) = facts.repositories() {
        detail.push_str(&format!(" · Repositories: {label}"));
    }
    let mut items = vec![manifest("gradle", file, detail)];
    items[0].evidence.dependencies = edges;
    for (id, version, managed) in facts.plugins {
        let scope = if managed { "Pluginverwaltung" } else { "Settings-Plugin" };
        let detail = format!("Gradle-Plugin · Spezifikation {version} · {scope}");
        let mut item = declared(file, &id, Some(&version), "plugin", detail);
        if managed {
            item.evidence.qualifier = "pluginManagement".into();
        }
        items.push(item);
    }
    for (text, context) in facts.catalog_entries {
        let quoted = format!("\"{text}\"");
        let Some(coordinate) = COORDINATE.captures(&quoted).filter(|c| c[0].len() == quoted.len()) else { continue };
        let name = format!("{}:{}", &coordinate[1], &coordinate[2]);
        let version = coordinate.get(3).map(|v| v.as_str().to_string()).filter(|v| !v.contains('$'));
        let detail = match (context, &version) {
            ("catalog-import", Some(version)) => format!("Versionskatalog importiert · Spezifikation {version}"),
            ("catalog-import", None) => "Versionskatalog importiert · Version unbekannt".to_string(),
            (_, Some(version)) => format!("Spezifikation {version} · Katalog in den Settings"),
            (_, None) => "Version aus BOM · Katalog in den Settings".to_string(),
        };
        items.push(declared(file, &name, version.as_deref(), context, detail));
    }
    items
}

fn catalog(file: &FoundFile<'_>) -> AdapterResult {
    let table = parsers::toml_table(file.content).map_err(|e| e.to_string())?;
    let versions = table.get("versions").and_then(toml::Value::as_table);
    // `"1.2"`, `{ strictly = "1.2" }`, `{ ref = "kotlin" }` (from `version.ref`).
    let version = |value: Option<&toml::Value>| -> Option<String> {
        let mut value = value?;
        if let Some(reference) = value.get("ref").and_then(toml::Value::as_str) {
            value = versions?.get(reference)?;
        }
        match value {
            toml::Value::String(text) => Some(text.clone()),
            toml::Value::Table(rich) => ["strictly", "require", "prefer"]
                .iter()
                .find_map(|key| rich.get(*key).and_then(toml::Value::as_str))
                .map(str::to_string),
            _ => None,
        }
    };
    let mut items = vec![manifest("gradle", file, "Versionskatalog")];
    for (alias, library) in table.get("libraries").and_then(toml::Value::as_table).into_iter().flatten() {
        let (name, spec) = match library {
            toml::Value::String(text) => match text.rsplitn(2, ':').collect::<Vec<_>>().as_slice() {
                [version, name] if name.contains(':') => ((*name).to_string(), Some((*version).to_string())),
                _ => (text.clone(), None),
            },
            toml::Value::Table(entry) => {
                let module = entry.get("module").and_then(toml::Value::as_str).map(str::to_string).or_else(|| {
                    let group = entry.get("group").and_then(toml::Value::as_str)?;
                    let name = entry.get("name").and_then(toml::Value::as_str)?;
                    Some(format!("{group}:{name}"))
                });
                let Some(module) = module else { continue };
                (module, version(entry.get("version")))
            }
            _ => continue,
        };
        let detail = format!(
            "{} · Katalog {alias}",
            spec.as_deref().map_or("Version aus BOM".into(), |v| format!("Spezifikation {v}"))
        );
        items.push(declared(file, &name, spec.as_deref(), "catalog", detail));
    }
    for (alias, plugin) in table.get("plugins").and_then(toml::Value::as_table).into_iter().flatten() {
        let (id, spec) = match plugin {
            toml::Value::String(text) => match text.split_once(':') {
                Some((id, version)) => (id.to_string(), Some(version.to_string())),
                None => (text.clone(), None),
            },
            toml::Value::Table(entry) => {
                let Some(id) = entry.get("id").and_then(toml::Value::as_str) else { continue };
                (id.to_string(), version(entry.get("version")))
            }
            _ => continue,
        };
        let detail = format!("Gradle-Plugin · Katalog {alias}");
        items.push(declared(file, &id, spec.as_deref(), "plugin", detail));
    }
    Ok(items)
}

fn lockfile(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut items = vec![manifest("gradle", file, "Abhängigkeits-Lockfile")];
    for line in file.content.lines().take(200_000) {
        // `group:artifact:version=configuration,…`; `empty=` lists none.
        if line.starts_with('#') {
            continue;
        }
        let Some((coordinate, configurations)) = line.split_once('=') else { continue };
        let mut parts = coordinate.splitn(3, ':');
        let (Some(group), Some(artifact), Some(version)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let mut item =
            InventoryItem::new(ItemCategory::Package, "gradle", format!("{group}:{artifact}"), file.rel_path())
                .version(version)
                .state(InstallState::Resolved)
                .detail(format!("festgelegt laut {}", file.name));
        item.evidence.context = configurations.into();
        items.push(item);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    #[test]
    fn script_coordinates_in_both_dialects() {
        let kts = "plugins {\n    id(\"com.android.application\") version \"8.7.3\" apply false\n    id(\"org.jetbrains.kotlin.android\")\n}\ndependencies {\n    val composeBom = platform(\"androidx.compose:compose-bom:2025.01.00\")\n    implementation(composeBom)\n    implementation(\"androidx.core:core-ktx:1.15.0\")\n    implementation(\"androidx.compose.ui:ui\")\n    testImplementation(\"junit:junit:$junitVersion\")\n    // implementation(\"commented:out:1.0\")\n    implementation(libs.androidx.activity)\n    implementation(project(\":plugin-api\"))\n}\n";
        let items = Gradle.parse_file(&file("build.gradle.kts", kts)).unwrap();
        assert!(items[0].detail.contains("1 Verweise auf den Versionskatalog"), "{}", items[0].detail);
        let core = items.iter().find(|i| i.name == "androidx.core:core-ktx").unwrap();
        assert_eq!((core.version.as_deref(), core.evidence.context.as_str()), (Some("1.15.0"), "implementation"));
        let ui = items.iter().find(|i| i.name == "androidx.compose.ui:ui").unwrap();
        assert!(ui.version.is_none() && ui.detail.starts_with("Version aus BOM"));
        let bom = items.iter().find(|i| i.name == "androidx.compose:compose-bom").unwrap();
        assert_eq!(bom.evidence.context, "platform");
        let junit = items.iter().find(|i| i.name == "junit:junit").unwrap();
        assert!(junit.version.is_none() && junit.detail.starts_with("Version aus Variable"));
        let plugin = items.iter().find(|i| i.name == "com.android.application").unwrap();
        assert_eq!((plugin.version.as_deref(), plugin.evidence.context.as_str()), (Some("8.7.3"), "plugin"));
        assert!(!items.iter().any(|i| i.name.starts_with("commented") || i.name.contains("plugin-api")));

        let groovy = "dependencies {\n    implementation 'com.squareup.okhttp3:okhttp:4.12.0'\n    debugImplementation \"androidx.compose.ui:ui-tooling\"\n    classpath 'com.android.tools.build:gradle:8.7.3'\n    def notADependency = 'a:b:c'\n}\n";
        let items = Gradle.parse_file(&file("build.gradle", groovy)).unwrap();
        let names: Vec<&str> = items.iter().skip(1).map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            ["com.squareup.okhttp3:okhttp", "androidx.compose.ui:ui-tooling", "com.android.tools.build:gradle"]
        );
    }

    #[test]
    fn catalog_and_lockfile() {
        let toml = "[versions]\nkotlin = \"2.1.0\"\nok = { strictly = \"4.12.0\" }\n\n[libraries]\nkotlin-stdlib = { module = \"org.jetbrains.kotlin:kotlin-stdlib\", version.ref = \"kotlin\" }\nokhttp = { group = \"com.squareup.okhttp3\", name = \"okhttp\", version.ref = \"ok\" }\njunit = \"junit:junit:4.13.2\"\ncompose-ui = { module = \"androidx.compose.ui:ui\" }\n\n[plugins]\nkotlin-android = { id = \"org.jetbrains.kotlin.android\", version.ref = \"kotlin\" }\n";
        let items = Gradle.parse_file(&file("libs.versions.toml", toml)).unwrap();
        let version = |name: &str| items.iter().find(|i| i.name == name).unwrap().version.clone();
        assert_eq!(version("org.jetbrains.kotlin:kotlin-stdlib").as_deref(), Some("2.1.0"));
        assert_eq!(version("com.squareup.okhttp3:okhttp").as_deref(), Some("4.12.0"));
        assert_eq!(version("junit:junit").as_deref(), Some("4.13.2"));
        assert_eq!(version("androidx.compose.ui:ui"), None);
        assert_eq!(version("org.jetbrains.kotlin.android").as_deref(), Some("2.1.0"));
        assert!(Gradle.parse_file(&file("libs.versions.toml", "[libraries\n")).is_err());

        let lock = "# This is a Gradle generated file\ncom.squareup.okhttp3:okhttp:4.12.0=debugRuntimeClasspath,releaseRuntimeClasspath\nempty=annotationProcessor\n";
        let items = Gradle.parse_file(&file("gradle.lockfile", lock)).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].install_state, Some(InstallState::Resolved));
        assert_eq!(items[1].evidence.context, "debugRuntimeClasspath,releaseRuntimeClasspath");
    }

    #[test]
    fn kotlin_settings_with_includes_plugins_repositories_and_catalogs() {
        let kts = r#"pluginManagement {
    includeBuild("build-logic")
    repositories {
        google {
            content { includeGroupByRegex("com\\.android.*") }
        }
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        id("com.android.application") version "8.7.3"
        kotlin("android") version "2.1.0"
    }
}
plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "0.9.0"
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        maven { url = uri("https://jitpack.io") }
        maven("https://maven.pkg.jetbrains.space/public/p/compose/dev")
        maven {
            url = uri("https://maven.corp.example/releases")
            credentials {
                username = "ci-user"
                password = "s3cret-pass"
            }
        }
    }
    versionCatalogs {
        create("libs") { from(files("gradle/libs.versions.toml")) }
        create("shared") { from("com.example:shared-catalog:1.4.0") }
        create("extra") { library("groovy-core", "org.codehaus.groovy:groovy:3.0.22") }
    }
}
buildCache { remote<HttpBuildCache> { url = uri("https://cache.corp.example/") } }
rootProject.name = "nowinandroid"
include(":app")
include(
    ":core:data",
    ":core:model",
)
include(":feature:${name}")
// include(":old")
"#;
        let items = Gradle.parse_file(&file("settings.gradle.kts", kts)).unwrap();
        let detail = &items[0].detail;
        assert!(
            detail.starts_with("Gradle-Settings (nicht ausgeführt) · Root-Projekt nowinandroid · 3 Teilprojekte · 1 eingebundene Builds · Versionskataloge deklariert · Repositories: google, mavenCentral, gradlePluginPortal"),
            "{detail}"
        );
        assert_eq!(
            walk(kts).repositories().as_deref(),
            Some(
                "google, mavenCentral, gradlePluginPortal, privat: jitpack.io, maven.pkg.jetbrains.space, maven.corp.example"
            )
        );
        let debug = format!("{items:?}");
        for secret in ["s3cret", "ci-user", "cache.corp", "/releases", ":old", "feature"] {
            assert!(!debug.contains(secret), "{secret} leaked");
        }
        let edges: Vec<(&str, &str)> =
            items[0].evidence.dependencies.iter().map(|d| (d.kind.as_str(), d.name.as_str())).collect();
        assert_eq!(
            edges,
            [
                ("includeBuild", "build-logic"),
                ("include", ":app"),
                ("include", ":core:data"),
                ("include", ":core:model")
            ]
        );
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let android = find("com.android.application");
        assert_eq!(
            (android.version.as_deref(), android.evidence.qualifier.as_str()),
            (Some("8.7.3"), "pluginManagement")
        );
        assert_eq!(find("org.jetbrains.kotlin.android").version.as_deref(), Some("2.1.0"));
        let foojay = find("org.gradle.toolchains.foojay-resolver-convention");
        assert!(foojay.detail.ends_with("Settings-Plugin") && foojay.evidence.qualifier.is_empty());
        let shared = find("com.example:shared-catalog");
        assert_eq!((shared.version.as_deref(), shared.evidence.context.as_str()), (Some("1.4.0"), "catalog-import"));
        assert_eq!(find("org.codehaus.groovy:groovy").evidence.context, "catalog");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
    }

    #[test]
    fn groovy_settings_and_repositories_in_build_scripts() {
        let groovy = "pluginManagement {\n    repositories {\n        maven { url 'https://repo1.maven.org/maven2' }\n        mavenLocal()\n    }\n}\nrootProject.name = 'legacy'\ninclude 'app', ':lib',\n        'tools:cli'\ninclude ':docs'\nincludeFlat 'sibling'\nincludeBuild '../shared-build'\nincludeBuild 'C:/elsewhere/build'\n";
        let items = Gradle.parse_file(&file("settings.gradle", groovy)).unwrap();
        assert_eq!(
            items[0].detail,
            "Gradle-Settings (nicht ausgeführt) · Root-Projekt legacy · 5 Teilprojekte · 1 eingebundene Builds · Repositories: mavenLocal, 1 öffentlich"
        );
        let names: Vec<&str> = items[0].evidence.dependencies.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, [":app", ":lib", ":tools:cli", ":docs", ":sibling", "../shared-build"]);

        let script = "buildscript {\n  repositories { google() }\n}\nrepositories {\n  maven { url = uri(\"https://user:tok3n@nexus.corp.example/repository/maven-public/\") }\n}\npublishing {\n  repositories { maven { url = uri(\"https://upload.corp.example/\") } }\n}\ndependencies {\n  implementation(\"com.google.guava:guava:33.3.1-jre\")\n}\n";
        let items = Gradle.parse_file(&file("build.gradle.kts", script)).unwrap();
        assert_eq!(
            items[0].detail,
            "Gradle-Buildskript (nicht ausgeführt) · Repositories: google, privat: nexus.corp.example"
        );
        assert!(!format!("{items:?}").contains("tok3n"));
        assert!(Gradle.claim_dir("build", &["settings.gradle.kts".into()], &[]).is_some());
        assert!(Gradle.claim_dir("build", &["README.md".into()], &[]).is_none());
    }
}
