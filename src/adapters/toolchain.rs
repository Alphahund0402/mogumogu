//! Runtime and tool version pins as declared requirements: dedicated files
//! (`.nvmrc`, `.python-version`, `.tool-versions`, `mise.toml`,
//! `rust-toolchain.toml`, `global.json`, the Gradle and Maven wrappers …)
//! and pins inside manifests whose packages other adapters report
//! (`engines`, `packageManager`, `go`/`toolchain`, `rust-version`,
//! `requires-python` …). A pin stays as written: ranges and aliases such as
//! `lts/*`, `stable` or `latest` are not resolved, nothing is installed or
//! started and installed versions are not compared. Gemfile, mix.exs,
//! Package.swift, build.zig.zon and Terraform files are code; single lines
//! are matched, never evaluated. Paths, download addresses, commits and
//! checksums are never stored.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::{json, lines, toml_table};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub struct Toolchain;

const ID: &str = "toolchain";
/// Pins kept per file; more are left out without failing the file.
const MAX_PINS: usize = 500;
const EMBEDDED: &str = "Eingebettete Versionsvorgaben";
const GRADLE_PUBLIC: &[&str] = &["services.gradle.org", "downloads.gradle.org", "downloads.gradle-dn.com"];
const MAVEN_PUBLIC: &[&str] = &[
    "repo.maven.apache.org",
    "repo1.maven.org",
    "repository.apache.org",
    "archive.apache.org",
    "dlcdn.apache.org",
    "downloads.apache.org",
];
const ROLL_FORWARD: &[&str] =
    &["patch", "feature", "minor", "major", "latestPatch", "latestFeature", "latestMinor", "latestMajor", "disable"];

/// Files naming one version of one tool: file name, tool, kind.
const VERSION_FILES: &[(&str, &str, &str)] = &[
    (".nvmrc", "node", "Node-Versionsdatei (nvm)"),
    (".node-version", "node", "Node-Versionsdatei"),
    (".ruby-version", "ruby", "Ruby-Versionsdatei"),
    (".java-version", "java", "Java-Versionsdatei (jEnv)"),
    (".go-version", "go", "Go-Versionsdatei"),
    (".bun-version", "bun", "Bun-Versionsdatei"),
    (".swift-version", "swift", "Swift-Versionsdatei"),
    (".terraform-version", "terraform", "Terraform-Versionsdatei (tfenv)"),
    (".bazelversion", "bazel", "Bazel-Versionsdatei (Bazelisk)"),
    (".dvmrc", "deno", "Deno-Versionsdatei (dvm)"),
];

static DESCRIPTOR: Descriptor = Descriptor {
    id: ID,
    name: "Laufzeit- und Werkzeugversionen",
    markers: &[
        ".nvmrc",
        ".node-version",
        ".python-version",
        ".python-versions",
        ".ruby-version",
        ".java-version",
        ".go-version",
        ".bun-version",
        ".swift-version",
        ".terraform-version",
        ".bazelversion",
        ".dvmrc",
        ".sdkmanrc",
        ".tool-versions",
        "mise.toml",
        "mise.*.toml",
        ".mise.toml",
        ".mise.*.toml",
        "rust-toolchain",
        "rust-toolchain.toml",
        "global.json",
        "gradle-wrapper.properties",
        "maven-wrapper.properties",
        "build.properties",
        "runtime.txt",
        ".yarnrc.yml",
        ".fvmrc",
        "fvm_config.json",
        "package.json",
        "go.mod",
        "go.work",
        "Cargo.toml",
        "pyproject.toml",
        "uv.toml",
        "Pipfile",
        "composer.json",
        "pubspec.yaml",
        "Gemfile",
        "gems.rb",
        "Gemfile.lock",
        "gems.locked",
        "mix.exs",
        "Package.swift",
        "build.zig.zon",
        "Project.toml",
        "JuliaProject.toml",
        "renv.lock",
        "*.tf",
        "*.tofu",
    ],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        ".nvmrc, .node-version, .python-version(s), .ruby-version, .java-version, .go-version, .bun-version, .swift-version, .terraform-version, .bazelversion, .dvmrc",
        ".tool-versions (asdf), .sdkmanrc, mise.toml (tools, min_version)",
        "rust-toolchain(.toml), global.json (sdk, msbuild-sdks), .fvmrc",
        "Gradle- und Maven-Wrapper (distributionUrl), project/build.properties (sbt), runtime.txt, .yarnrc.yml (yarnPath)",
        "package.json (engines, packageManager, volta, devEngines), go.mod/go.work (go, toolchain), Cargo.toml (rust-version)",
        "pyproject.toml (requires-python, Poetry, uv), uv.toml, Pipfile, composer.json (php), pubspec.yaml (environment)",
        "Gemfile/Gemfile.lock (ruby, BUNDLED WITH), mix.exs (elixir), Package.swift, build.zig.zon, Project.toml (julia), renv.lock (R), *.tf (required_version)",
    ],
    limits: "Nur Vorgaben wie geschrieben: Bereiche und Aliase (lts/*, stable, latest) werden nicht aufgelöst, nichts wird installiert oder gestartet, installierte Versionen werden nicht abgeglichen; Gemfile, mix.exs, Package.swift und Terraform-Dateien nur als Text gelesen; Pfade, Download-Adressen, Commits und Prüfsummen werden nicht gespeichert.",
};

static PYTHON_RUNTIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^python-(\d+(?:\.\d+){0,2})$").expect("valid regex"));
static YARN_RELEASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^yarn-(\d[0-9A-Za-z.+\-]*)\.c?js$").expect("valid regex"));
static GRADLE_DIST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^gradle-(.+)-(bin|all)\.zip$").expect("valid regex"));
static MAVEN_DIST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^apache-maven-(.+)-bin\.(?:zip|tar\.gz)$").expect("valid regex"));
static MVND_DIST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:maven-)?mvnd-(.+?)-(?:windows|linux|darwin)-[A-Za-z0-9_]+\.(?:zip|tar\.gz)$").expect("valid regex")
});
static WRAPPER_JAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^maven-wrapper-(.+)\.jar$").expect("valid regex"));
/// Bundler's `ruby` directive; `ruby_version = …` is an assignment.
static RUBY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*ruby(?:\s+|\s*\()(.*)$").expect("valid regex"));
static QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*(?:"([^"]*)"|'([^']*)')\s*,?"#).expect("valid regex"));
static ENGINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:\bengine:|:engine\s*=>)\s*["']([^"']+)["']"#).expect("valid regex"));
static ENGINE_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:\bengine_version:|:engine_version\s*=>)\s*["']([^"']+)["']"#).expect("valid regex")
});
static ELIXIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\belixir:\s*"([^"]+)""#).expect("valid regex"));
static SWIFT_TOOLS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*//\s*swift-tools-version\s*:\s*([0-9][0-9A-Za-z.\-]*)").expect("valid regex"));
static ZIG_MINIMUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*\.minimum_zig_version\s*=\s*"([^"]+)""#).expect("valid regex"));
static TERRAFORM_BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*terraform\s*\{").expect("valid regex"));
static REQUIRED_VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\brequired_version\s*=\s*"([^"]*)""#).expect("valid regex"));
static HEREDOC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^<<-?\s*([A-Za-z_][A-Za-z0-9_]*)").expect("valid regex"));

impl Adapter for Toolchain {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let name = file.name.to_ascii_lowercase();
        let mut pins = Pins { file, items: Vec::new(), seen: BTreeMap::new() };
        if let Some((origin, tool, kind)) = VERSION_FILES.iter().find(|(known, ..)| *known == name) {
            single_version(&mut pins, tool, origin);
            return pins.finish(kind);
        }
        let kind = match name.as_str() {
            ".python-version" | ".python-versions" => {
                python_versions(&mut pins, &name);
                "Python-Versionsdatei (pyenv, uv)"
            }
            ".sdkmanrc" => {
                for (tool, version) in lines::key_values(file.content) {
                    if plausible(&version) {
                        pins.add(&tool, &version, ".sdkmanrc", None, Vec::new());
                    }
                }
                "SDKMAN-Umgebung"
            }
            ".tool-versions" => {
                tool_versions(&mut pins);
                "asdf-Werkzeugliste"
            }
            "rust-toolchain" | "rust-toolchain.toml" => {
                rust_toolchain(&mut pins, name == "rust-toolchain")?;
                "rustup-Toolchaindatei"
            }
            "global.json" => {
                global_json(&mut pins);
                ".NET-SDK-Auswahl"
            }
            "gradle-wrapper.properties" if in_dir(file.rel_dir, &["gradle", "wrapper"]) => {
                gradle_wrapper(&mut pins);
                "Gradle-Wrapper"
            }
            "maven-wrapper.properties" if in_dir(file.rel_dir, &[".mvn", "wrapper"]) => {
                maven_wrapper(&mut pins);
                "Maven-Wrapper"
            }
            // sbt reads `project/build.properties`; the name is common elsewhere.
            "build.properties" if in_dir(file.rel_dir, &["project"]) => {
                let properties = properties(file.content);
                if let Some((_, version)) = properties.iter().find(|(key, _)| key == "sbt.version")
                    && plausible(version)
                {
                    pins.add("sbt", version, "build.properties", None, Vec::new());
                }
                "sbt-Version"
            }
            "runtime.txt" => {
                let first = file.content.lines().map(str::trim).find(|line| !line.is_empty());
                if let Some(runtime) = first.and_then(|line| PYTHON_RUNTIME.captures(line)) {
                    pins.add("python", &runtime[1], "runtime.txt", None, Vec::new());
                }
                "Laufzeitangabe"
            }
            ".yarnrc.yml" => {
                let path =
                    lines::yaml_entries(file.content).find(|e| e.indent == 0 && !e.item && e.key == Some("yarnPath"));
                if let Some(release) = path.and_then(|entry| YARN_RELEASE.captures(file_name(entry.value))) {
                    pins.add("yarn", &release[1], "yarnPath", None, vec!["Version aus dem Dateinamen".into()]);
                }
                "Yarn-Konfiguration"
            }
            ".fvmrc" => {
                fvm(&mut pins, "flutter")?;
                "FVM-Konfiguration"
            }
            "fvm_config.json" if in_dir(file.rel_dir, &[".fvm"]) => {
                fvm(&mut pins, "flutterSdkVersion")?;
                "FVM-Konfiguration"
            }
            "package.json" => {
                package_json(&mut pins);
                EMBEDDED
            }
            "go.mod" | "go.work" => {
                go_directives(&mut pins);
                EMBEDDED
            }
            "cargo.toml" => {
                cargo(&mut pins);
                EMBEDDED
            }
            "pyproject.toml" => {
                pyproject(&mut pins);
                EMBEDDED
            }
            "uv.toml" => {
                if let Ok(table) = toml_table(file.content)
                    && let Some(spec) = toml_str(&table, &["required-version"])
                {
                    pins.add("uv", spec, "required-version", None, Vec::new());
                }
                EMBEDDED
            }
            "pipfile" => {
                if let Ok(table) = toml_table(file.content) {
                    for key in ["python_version", "python_full_version"] {
                        if let Some(spec) = toml_str(&table, &["requires", key]) {
                            pins.add("python", spec, key, Some("requires"), Vec::new());
                        }
                    }
                }
                EMBEDDED
            }
            "composer.json" => {
                composer(&mut pins);
                EMBEDDED
            }
            "pubspec.yaml" => {
                pubspec(&mut pins);
                EMBEDDED
            }
            "gemfile" | "gems.rb" => {
                gemfile(&mut pins);
                EMBEDDED
            }
            "gemfile.lock" | "gems.locked" => {
                gemfile_lock(&mut pins);
                EMBEDDED
            }
            "mix.exs" => {
                if file.content.contains("Mix.Project")
                    && let Some(spec) = first_capture(file.content, &ELIXIR, "#")
                {
                    pins.add("elixir", spec, "elixir", Some("project"), Vec::new());
                }
                EMBEDDED
            }
            // The tools version must be the manifest's first line.
            "package.swift" => {
                let first = file.content.lines().find(|line| !line.trim().is_empty());
                if let Some(tools) = first.and_then(|line| SWIFT_TOOLS.captures(line)) {
                    pins.add("swift", &tools[1], "swift-tools-version", Some("swift-tools-version"), Vec::new());
                }
                EMBEDDED
            }
            "build.zig.zon" => {
                if let Some(spec) = first_capture(file.content, &ZIG_MINIMUM, "//") {
                    pins.add("zig", spec, "minimum_zig_version", Some("minimum_zig_version"), Vec::new());
                }
                EMBEDDED
            }
            "project.toml" | "juliaproject.toml" => {
                if let Ok(table) = toml_table(file.content)
                    && let Some(spec) = toml_str(&table, &["compat", "julia"])
                {
                    pins.add("julia", spec, "compat", Some("compat"), Vec::new());
                }
                EMBEDDED
            }
            "renv.lock" => {
                if let Ok(value) = json::parse(file.content, 64)
                    && let Some(spec) = value.pointer("/R/Version").and_then(Value::as_str)
                {
                    pins.add("r", spec, "R.Version", Some("R"), Vec::new());
                }
                EMBEDDED
            }
            _ if (name.starts_with("mise.") || name.starts_with(".mise.")) && name.ends_with(".toml") => {
                mise(&mut pins)?;
                "mise-Konfiguration"
            }
            _ if name.ends_with(".tf") => {
                terraform(&mut pins, "terraform");
                EMBEDDED
            }
            _ if name.ends_with(".tofu") => {
                terraform(&mut pins, "opentofu");
                EMBEDDED
            }
            _ => return Ok(Vec::new()),
        };
        pins.finish(kind)
    }
}

/// The pins of one file. A tool named twice for one origin gets its
/// position as qualifier, so `python 3.12 3.11` keeps both entries.
struct Pins<'a> {
    file: &'a FoundFile<'a>,
    items: Vec<InventoryItem>,
    seen: BTreeMap<(String, String), usize>,
}

impl Pins<'_> {
    fn add(
        &mut self,
        tool: &str,
        spec: &str,
        origin: &str,
        label: Option<&str>,
        notes: Vec<String>,
    ) -> Option<&mut InventoryItem> {
        let tool = tool_name(tool)?;
        self.push(tool, spec, origin, label, notes)
    }

    /// `origin` becomes the context; `label` names the field inside a shared
    /// manifest (`engines` …) for the detail.
    fn push(
        &mut self,
        tool: String,
        spec: &str,
        origin: &str,
        label: Option<&str>,
        notes: Vec<String>,
    ) -> Option<&mut InventoryItem> {
        if self.items.len() >= MAX_PINS {
            return None;
        }
        let written = written(spec);
        let position = self.seen.entry((tool.clone(), origin.to_string())).or_insert(0);
        *position += 1;
        let qualifier = if *position == 1 { String::new() } else { position.to_string() };
        let mut detail = match label {
            Some(label) => format!("Vorgabe laut {} ({label})", self.file.name),
            None => format!("Vorgabe laut {}", self.file.name),
        };
        for note in written.note.map(str::to_string).into_iter().chain(notes) {
            detail.push_str(" · ");
            detail.push_str(&note);
        }
        let mut item = InventoryItem::new(ItemCategory::Package, ID, tool, self.file.rel_path())
            .version(written.version.unwrap_or_default())
            .state(InstallState::Declared)
            .source(written.source, None)
            .detail(detail);
        item.evidence.context = origin.to_string();
        item.evidence.qualifier = qualifier;
        self.items.push(item);
        self.items.last_mut()
    }

    /// Manifest first, then the pins; a file without pins reports nothing.
    fn finish(self, kind: &str) -> AdapterResult {
        if self.items.is_empty() {
            return Ok(Vec::new());
        }
        let mut tools: Vec<&str> = Vec::new();
        for item in &self.items {
            if !tools.contains(&item.name.as_str()) {
                tools.push(&item.name);
            }
        }
        let mut listed = tools.iter().take(8).copied().collect::<Vec<_>>().join(", ");
        if tools.len() > 8 {
            listed.push_str(", …");
        }
        let detail = format!("{kind} · Vorgabe{} für {listed}", if tools.len() == 1 { "" } else { "n" });
        let mut items = vec![manifest(ID, self.file, detail)];
        items.extend(self.items);
        bounded(items)
    }
}

/// What may be stored of a written version request.
struct Written {
    version: Option<String>,
    note: Option<&'static str>,
    source: SourceClass,
}

fn omitted(note: &'static str, source: SourceClass) -> Written {
    Written { version: None, note: Some(note), source }
}

fn written(spec: &str) -> Written {
    let spec = spec.trim();
    let lower = spec.to_ascii_lowercase();
    if spec.is_empty() {
        return Written { version: None, note: None, source: SourceClass::Unknown };
    }
    if lower.contains("://") {
        return omitted("Download-Adresse nicht gespeichert", SourceClass::Unknown);
    }
    if lower.starts_with("path:") || is_path(spec) {
        return omitted("lokaler Pfad nicht gespeichert", SourceClass::Path);
    }
    let reference = lower.strip_prefix("ref:");
    let source = if reference.is_some() { SourceClass::Git } else { SourceClass::Unknown };
    if is_commit(reference.unwrap_or(&lower)) {
        return omitted("Commit nicht gespeichert", source);
    }
    if spec.chars().count() > 64 || spec.chars().any(char::is_control) {
        return omitted("Angabe nicht gespeichert", SourceClass::Unknown);
    }
    Written { version: Some(spec.to_string()), note: nature(spec), source }
}

fn is_path(spec: &str) -> bool {
    let bytes = spec.as_bytes();
    spec.starts_with(['/', '\\'])
        || ["~/", "~\\", "./", "../", ".\\", "..\\"].iter().any(|prefix| spec.starts_with(prefix))
        || (bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'))
}

/// A Git commit id: twelve or more hex digits with at least one letter.
fn is_commit(text: &str) -> bool {
    text.len() >= 12 && text.chars().all(|c| c.is_ascii_hexdigit()) && text.chars().any(|c| c.is_ascii_alphabetic())
}

/// How a request reads: a range, an alias that something else resolves or
/// the system installation. An exact version needs no note, also behind `=`
/// or `==`. `*` and `x` are wildcards, not names.
fn nature(spec: &str) -> Option<&'static str> {
    let named = spec.chars().any(|c| c.is_ascii_alphabetic() && !c.eq_ignore_ascii_case(&'x'));
    let exact = spec.trim_start_matches('=').trim_start();
    if spec.eq_ignore_ascii_case("system") {
        Some("Systeminstallation")
    } else if named && !spec.chars().any(|c| c.is_ascii_digit()) {
        Some("Alias, nicht aufgelöst")
    } else if exact.contains(['^', '~', '<', '>', '=', '*', '|', ','])
        || exact.contains(" - ")
        || exact.split(['.', '-']).any(|part| part.eq_ignore_ascii_case("x"))
    {
        Some("Bereich")
    } else {
        None
    }
}

/// A tool name, lowercase; asdf plugin names map to the names mise and the
/// version files use, backend prefixes such as `npm:` stay.
fn tool_name(raw: &str) -> Option<String> {
    let lower = raw.trim().to_ascii_lowercase();
    let name = match lower.strip_prefix("core:").unwrap_or(&lower) {
        "nodejs" | "node.js" => "node",
        "golang" => "go",
        "dotnet" | "dotnet-core" => "dotnet-sdk",
        other => other,
    };
    // A backend key may be a URL (`cargo:https://host/team/tool`): only the
    // backend and the last path segment are kept.
    let shortened;
    let name = match name.split_once(':') {
        Some((backend, target)) if target.contains("://") || target.contains('@') => {
            let last = target.trim_end_matches('/').rsplit(['/', '@']).next().unwrap_or_default();
            shortened = format!("{backend}:{}", last.strip_suffix(".git").unwrap_or(last));
            shortened.as_str()
        }
        _ => name,
    };
    let valid = !name.is_empty()
        && name.len() <= 100
        && !name.starts_with(['.', '/'])
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-+@:/".contains(c));
    valid.then(|| name.to_string())
}

/// One bounded word of version characters, so prose or binary content in a
/// version file is not taken for a version.
fn plausible(word: &str) -> bool {
    word.len() <= 64
        && word.chars().any(|c| c.is_ascii_alphanumeric())
        && word.chars().all(|c| c.is_ascii_alphanumeric() || "._-+*/~^<>=!|:@,".contains(c))
}

/// A package or flavor name: letters, digits, `.`, `_` and `-`.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 100 && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// The first meaningful line holds exactly one version, as rbenv, nodenv or
/// Bazelisk expect it; `#` starts a comment and `.nvmrc` ignores
/// `key=value` lines.
fn single_version(pins: &mut Pins<'_>, tool: &str, origin: &str) {
    let content = pins.file.content;
    for line in content.lines().take(100) {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() || (origin == ".nvmrc" && line.contains('=')) {
            continue;
        }
        let mut words = line.split_whitespace();
        if let (Some(word), None) = (words.next(), words.next())
            && plausible(word)
        {
            pins.add(tool, word, origin, None, Vec::new());
        }
        return;
    }
}

/// pyenv reads the first word of every line and skips `..` and anything
/// with a `/`, which would name a path; uv's `.python-versions` lists one
/// version per line.
fn python_versions(pins: &mut Pins<'_>, origin: &str) {
    let content = pins.file.content;
    for line in content.lines().take(1_000) {
        let Some(word) = line.split('#').next().unwrap_or_default().split_whitespace().next() else { continue };
        if word == ".." || word.contains('/') || !plausible(word) {
            continue;
        }
        pins.add("python", word, origin, None, Vec::new());
    }
}

/// asdf: `tool version [version …]` per line in order of preference.
fn tool_versions(pins: &mut Pins<'_>) {
    let content = pins.file.content;
    for line in content.lines().take(2_000) {
        let mut words = line.split('#').next().unwrap_or_default().split_whitespace();
        let Some(tool) = words.next() else { continue };
        for version in words.take(20) {
            if plausible(version) || version.starts_with("path:") {
                pins.add(tool, version, "tool-versions", None, Vec::new());
            }
        }
    }
}

/// mise: `[tools]` entries as a string, a list or a table with `version`,
/// and `min_version` for mise itself. Environment, tasks and plugin sources
/// are not read.
fn mise(pins: &mut Pins<'_>) -> Result<(), String> {
    let table = toml_table(pins.file.content).map_err(|e| e.to_string())?;
    let version = |value: &toml::Value| -> Option<String> {
        match value {
            toml::Value::String(spec) => Some(spec.clone()),
            toml::Value::Table(options) => options.get("version")?.as_str().map(str::to_string),
            _ => None,
        }
    };
    for (tool, value) in table.get("tools").and_then(toml::Value::as_table).into_iter().flatten().take(500) {
        let specs: Vec<String> = match value {
            toml::Value::Array(list) => list.iter().take(20).filter_map(version).collect(),
            other => version(other).into_iter().collect(),
        };
        for spec in specs {
            pins.add(tool, &spec, "mise", None, Vec::new());
        }
    }
    let minimum = table.get("min_version");
    let hard = minimum.and_then(|m| m.as_str().or_else(|| m.get("hard")?.as_str()));
    let soft = minimum.and_then(|m| m.get("soft")?.as_str());
    match (hard, soft) {
        (Some(hard), soft) => {
            let mut notes = vec!["Mindestversion".to_string()];
            notes.extend(soft.map(|soft| format!("empfohlen {soft}")));
            pins.add("mise", hard, "min_version", None, notes);
        }
        (None, Some(soft)) => {
            pins.add("mise", soft, "min_version", None, vec!["empfohlene Mindestversion".into()]);
        }
        (None, None) => {}
    }
    Ok(())
}

/// rustup: `[toolchain]` with `channel` or a custom `path`; the older
/// `rust-toolchain` file may hold just the channel name.
fn rust_toolchain(pins: &mut Pins<'_>, legacy: bool) -> Result<(), String> {
    let content = pins.file.content;
    if legacy && !content.contains("[toolchain]") {
        let channel = content.lines().map(str::trim).find(|line| !line.is_empty());
        if let Some(channel) = channel.filter(|channel| plausible(channel)) {
            pins.add("rust", channel, "rust-toolchain", None, Vec::new());
        }
        return Ok(());
    }
    let table = toml_table(content).map_err(|e| e.to_string())?;
    let Some(toolchain) = table.get("toolchain").and_then(toml::Value::as_table) else { return Ok(()) };
    let mut notes = Vec::new();
    if let Some(profile) = toolchain.get("profile").and_then(toml::Value::as_str).filter(|p| plain_name(p)) {
        notes.push(format!("Profil {profile}"));
    }
    for (key, label) in [("components", "Komponenten"), ("targets", "Ziele")] {
        let values: Vec<&str> = toolchain
            .get(key)
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_str)
            .filter(|value| plain_name(value))
            .take(20)
            .collect();
        if !values.is_empty() {
            notes.push(format!("{label} {}", values.join(", ")));
        }
    }
    if let Some(channel) = toolchain.get("channel").and_then(toml::Value::as_str) {
        pins.add("rust", channel, "rust-toolchain", None, notes);
    } else if toolchain.contains_key("path") {
        notes.insert(0, "eigene Toolchain, Pfad nicht gespeichert".into());
        if let Some(item) = pins.add("rust", "", "rust-toolchain", None, notes) {
            item.source = SourceClass::Path;
        }
    }
    Ok(())
}

/// .NET SDK selection and MSBuild project SDKs. `paths` (.NET 10) name
/// local folders and are never stored. The file name is generic, so a
/// foreign or broken file reports nothing.
fn global_json(pins: &mut Pins<'_>) {
    let Ok(value) = json::parse_jsonc(pins.file.content, 32) else { return };
    if let Some(sdk) = value.get("sdk").and_then(Value::as_object) {
        let mut notes = Vec::new();
        if let Some(policy) = sdk.get("rollForward").and_then(Value::as_str).filter(|p| ROLL_FORWARD.contains(p)) {
            notes.push(format!("rollForward {policy}"));
        }
        match sdk.get("allowPrerelease").and_then(Value::as_bool) {
            Some(true) => notes.push("Vorabversionen erlaubt".into()),
            Some(false) => notes.push("ohne Vorabversionen".into()),
            None => {}
        }
        if sdk.get("paths").and_then(Value::as_array).is_some_and(|paths| !paths.is_empty()) {
            notes.push("eigene SDK-Suchpfade (nicht gespeichert)".into());
        }
        let version = sdk.get("version").and_then(Value::as_str);
        if version.is_some() || !notes.is_empty() {
            pins.add("dotnet-sdk", version.unwrap_or_default(), "global.json", None, notes);
        }
    }
    for (name, version) in value.get("msbuild-sdks").and_then(Value::as_object).into_iter().flatten().take(100) {
        if let Some(version) = version.as_str()
            && plain_name(name)
        {
            pins.push(format!("msbuild-sdk:{name}"), version, "msbuild-sdks", None, Vec::new());
        }
    }
}

/// `distributionUrl=https\://services.gradle.org/distributions/gradle-8.5-bin.zip`:
/// version and type come from the file name, only the host is kept.
fn gradle_wrapper(pins: &mut Pins<'_>) {
    let properties = properties(pins.file.content);
    let get = |key: &str| properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
    let Some(url) = get("distributionUrl") else { return };
    let distribution = GRADLE_DIST.captures(file_name(url));
    let version = distribution.as_ref().and_then(|c| c.get(1)).map(|m| m.as_str()).filter(|v| plausible(v));
    let mut notes = Vec::new();
    match (distribution.as_ref().and_then(|c| c.get(2)), version) {
        (Some(kind), Some(_)) => notes.push(format!("Distribution {}", kind.as_str())),
        _ => notes.push("Version aus dem Verteilungsort nicht erkennbar".into()),
    }
    if get("distributionSha256Sum").is_some() {
        notes.push("Prüfsumme festgelegt".into());
    }
    wrapper_pin(pins, "gradle", version.unwrap_or_default(), url, GRADLE_PUBLIC, notes);
}

/// `distributionUrl` names Maven (`apache-maven-3.9.9-bin.zip`) or mvnd;
/// `wrapperVersion` (3.x) or the jar in `wrapperUrl` (0.5.x) the wrapper.
fn maven_wrapper(pins: &mut Pins<'_>) {
    let properties = properties(pins.file.content);
    let get = |key: &str| properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
    let Some(url) = get("distributionUrl") else { return };
    let name = file_name(url);
    let (tool, version) = match (MAVEN_DIST.captures(name), MVND_DIST.captures(name)) {
        (Some(maven), _) => ("maven", maven.get(1).map(|m| m.as_str())),
        (None, Some(mvnd)) => ("mvnd", mvnd.get(1).map(|m| m.as_str())),
        (None, None) => ("maven", None),
    };
    let version = version.filter(|v| plausible(v));
    let mut notes = Vec::new();
    if version.is_none() {
        notes.push("Version aus dem Verteilungsort nicht erkennbar".into());
    }
    let jar = || get("wrapperUrl").and_then(|u| WRAPPER_JAR.captures(file_name(u))?.get(1)).map(|m| m.as_str());
    if let Some(wrapper) = get("wrapperVersion").or_else(jar).filter(|w| plausible(w)) {
        notes.push(format!("Wrapper {wrapper}"));
    }
    if get("distributionSha256Sum").is_some() {
        notes.push("Prüfsumme festgelegt".into());
    }
    wrapper_pin(pins, tool, version.unwrap_or_default(), url, MAVEN_PUBLIC, notes);
}

/// A wrapper pin with its source class; the address is never stored and a
/// host only when it is a public distribution server.
fn wrapper_pin(pins: &mut Pins<'_>, tool: &str, version: &str, url: &str, public: &[&str], mut notes: Vec<String>) {
    let (source, host) = distribution(url, public);
    match source {
        SourceClass::PrivateRegistry => notes.push("eigener Verteilungsort".into()),
        SourceClass::Path => notes.push("lokale Distribution".into()),
        _ => {}
    }
    if let Some(item) = pins.add(tool, version, "wrapper", None, notes) {
        item.source = source;
        item.source_host = host;
    }
}

/// Like `classify_url`, but an own distribution server stays unnamed.
fn distribution(url: &str, public: &[&str]) -> (SourceClass, Option<String>) {
    let (source, host) = classify_url(url, public);
    (source, host.filter(|_| source == SourceClass::PublicRegistry))
}

/// FVM: `.fvmrc` with `flutter` and `flavors`, or the older
/// `.fvm/fvm_config.json` with `flutterSdkVersion`.
fn fvm(pins: &mut Pins<'_>, key: &str) -> Result<(), String> {
    let value = json::parse(pins.file.content, 32).map_err(|e| e.to_string())?;
    if let Some(spec) = value.get(key).and_then(Value::as_str) {
        pins.add("flutter", spec, "fvm", None, Vec::new());
    }
    for (flavor, spec) in value.get("flavors").and_then(Value::as_object).into_iter().flatten().take(50) {
        if let Some(spec) = spec.as_str()
            && plain_name(flavor)
            && let Some(item) = pins.add("flutter", spec, "fvm", None, vec![format!("Flavor {flavor}")])
        {
            item.evidence.qualifier = format!("flavor:{flavor}");
        }
    }
    Ok(())
}

/// package.json: `engines`, `packageManager` (Corepack), `volta` and npm's
/// `devEngines`. A broken manifest is npm's to report.
fn package_json(pins: &mut Pins<'_>) {
    let Ok(value) = json::parse(pins.file.content, 64) else { return };
    for (tool, spec) in value.get("engines").and_then(Value::as_object).into_iter().flatten().take(50) {
        if let Some(spec) = spec.as_str() {
            pins.add(tool, spec, "engines", Some("engines"), Vec::new());
        }
    }
    if let Some(spec) = value.get("packageManager").and_then(Value::as_str) {
        package_manager(pins, spec);
    }
    // `volta.extends` points to another file and is no pin.
    for (tool, spec) in value.get("volta").and_then(Value::as_object).into_iter().flatten().take(10) {
        if matches!(tool.as_str(), "node" | "npm" | "yarn" | "pnpm")
            && let Some(spec) = spec.as_str()
        {
            pins.add(tool, spec, "volta", Some("volta"), Vec::new());
        }
    }
    let Some(dev) = value.get("devEngines").and_then(Value::as_object) else { return };
    for (key, label) in [("runtime", "devEngines.runtime"), ("packageManager", "devEngines.packageManager")] {
        let entries: Vec<&Value> = match dev.get(key) {
            Some(Value::Array(list)) => list.iter().take(10).collect(),
            Some(entry) => vec![entry],
            None => Vec::new(),
        };
        for entry in entries {
            let Some(name) = entry.get("name").and_then(Value::as_str) else { continue };
            let spec = entry.get("version").and_then(Value::as_str).unwrap_or_default();
            let notes = if spec.trim().is_empty() { vec!["ohne Version".to_string()] } else { Vec::new() };
            pins.add(name, spec, "devEngines", Some(label), notes);
        }
    }
}

/// `pnpm@9.1.0+sha512.…`: what follows `+` is a checksum and is not kept.
/// Corepack also accepts a download address instead of a version.
fn package_manager(pins: &mut Pins<'_>, spec: &str) {
    let Some((name, reference)) = spec.split_once('@').filter(|(name, _)| !name.is_empty()) else { return };
    if reference.contains("://") {
        let (source, host) = distribution(reference, super::npm::PUBLIC);
        let note = "Version aus einer Download-Adresse, nicht gespeichert".to_string();
        if let Some(item) = pins.add(name, "", "packageManager", Some("packageManager"), vec![note]) {
            item.source = source;
            item.source_host = host;
        }
        return;
    }
    let (version, notes) = match reference.split_once('+') {
        Some((version, _)) => (version, vec!["Prüfsumme festgelegt".to_string()]),
        None => (reference, Vec::new()),
    };
    pins.add(name, version, "packageManager", Some("packageManager"), notes);
}

/// The `go` and `toolchain` directives of go.mod and go.work, outside blocks.
fn go_directives(pins: &mut Pins<'_>) {
    let content = pins.file.content;
    let mut block = false;
    for raw in content.lines().take(200_000) {
        let code = raw.split("//").next().unwrap_or_default().trim();
        if block {
            block = code != ")";
            continue;
        }
        if code.ends_with('(') {
            block = true;
            continue;
        }
        let (verb, rest) = code.split_once(char::is_whitespace).unwrap_or((code, ""));
        let rest = rest.trim();
        if !plausible(rest) {
            continue;
        }
        match verb {
            "go" => {
                pins.add("go", rest, "go", Some("go"), Vec::new());
            }
            // `go1.22.4` or a custom name such as `go1.22.4-corp`.
            "toolchain" => {
                let version = rest.strip_prefix("go").filter(|v| v.starts_with(|c: char| c.is_ascii_digit()));
                pins.add("go", version.unwrap_or(rest), "toolchain", Some("toolchain"), Vec::new());
            }
            _ => {}
        }
    }
}

/// `rust-version` of the package and of `[workspace.package]`. A member's
/// `rust-version.workspace = true` only inherits and is no pin of its own.
fn cargo(pins: &mut Pins<'_>) {
    let Ok(table) = toml_table(pins.file.content) else { return };
    if let Some(spec) = toml_str(&table, &["package", "rust-version"]) {
        pins.add("rust", spec, "rust-version", Some("rust-version"), Vec::new());
    }
    if let Some(spec) = toml_str(&table, &["workspace", "package", "rust-version"]) {
        pins.add("rust", spec, "workspace.rust-version", Some("workspace.package.rust-version"), Vec::new());
    }
}

/// `requires-python`, Poetry's `python` dependency and `requires-poetry`,
/// uv's `required-version`.
fn pyproject(pins: &mut Pins<'_>) {
    let Ok(table) = toml_table(pins.file.content) else { return };
    if let Some(spec) = toml_str(&table, &["project", "requires-python"]) {
        pins.add("python", spec, "requires-python", Some("requires-python"), Vec::new());
    }
    let poetry = table.get("tool").and_then(|tool| tool.get("poetry"));
    let python = poetry.and_then(|p| p.get("dependencies")).and_then(|d| d.get("python"));
    if let Some(spec) = python.and_then(|v| v.as_str().or_else(|| v.get("version")?.as_str())) {
        pins.add("python", spec, "poetry", Some("tool.poetry.dependencies"), Vec::new());
    }
    if let Some(spec) = poetry.and_then(|p| p.get("requires-poetry")).and_then(toml::Value::as_str) {
        pins.add("poetry", spec, "requires-poetry", Some("tool.poetry"), Vec::new());
    }
    if let Some(spec) = toml_str(&table, &["tool", "uv", "required-version"]) {
        pins.add("uv", spec, "required-version", Some("tool.uv"), Vec::new());
    }
}

/// `php` in `require`/`require-dev` and the platform assumed for resolving.
fn composer(pins: &mut Pins<'_>) {
    let Ok(value) = json::parse(pins.file.content, 64) else { return };
    for section in ["require", "require-dev"] {
        if let Some(spec) = value.get(section).and_then(|s| s.get("php")).and_then(Value::as_str) {
            pins.add("php", spec, section, Some(section), Vec::new());
        }
    }
    if let Some(spec) = value.pointer("/config/platform/php").and_then(Value::as_str) {
        let note = "nur für die Auflösung angenommen".to_string();
        pins.add("php", spec, "config.platform", Some("config.platform"), vec![note]);
    }
}

/// `environment:` with `sdk` (Dart) and `flutter`.
fn pubspec(pins: &mut Pins<'_>) {
    let content = pins.file.content;
    let (mut inside, mut child) = (false, None);
    for entry in lines::yaml_entries(content).take(100_000) {
        if entry.indent == 0 {
            inside = !entry.item && entry.key == Some("environment");
            child = None;
            continue;
        }
        if !inside || entry.item || *child.get_or_insert(entry.indent) != entry.indent {
            continue;
        }
        let tool = match entry.key {
            Some("sdk") => "dart",
            Some("flutter") => "flutter",
            _ => continue,
        };
        pins.add(tool, entry.value, "environment", Some("environment"), Vec::new());
    }
}

/// Bundler's `ruby` directive: requirements as strings, optionally an
/// engine, or `file:` naming a version file that is read on its own.
fn gemfile(pins: &mut Pins<'_>) {
    let content = pins.file.content;
    let Some(args) = content
        .lines()
        .take(20_000)
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| RUBY.captures(line).and_then(|c| c.get(1)).map(|m| m.as_str()))
    else {
        return;
    };
    let mut rest = args;
    let mut requirements = Vec::new();
    while requirements.len() < 5
        && let Some(quoted) = QUOTED.captures(rest)
    {
        requirements.push(quoted.get(1).or_else(|| quoted.get(2)).map_or("", |m| m.as_str()));
        rest = &rest[quoted.get(0).map_or(rest.len(), |m| m.end())..];
    }
    let mut notes = Vec::new();
    if let Some(engine) = ENGINE.captures(args).filter(|c| plain_name(&c[1])) {
        let version = ENGINE_VERSION.captures(args).filter(|c| plausible(&c[1])).map(|c| format!(" {}", &c[1]));
        notes.push(format!("Engine {}{}", &engine[1], version.unwrap_or_default()));
    }
    let args = args.trim_start();
    if !requirements.is_empty() {
        pins.add("ruby", &requirements.join(", "), "ruby", Some("ruby"), notes);
    } else if args.starts_with("file:") || args.starts_with(":file") {
        notes.insert(0, "Version aus einer Versionsdatei, hier nicht gelesen".into());
        pins.add("ruby", "", "ruby", Some("ruby"), notes);
    }
}

/// The `RUBY VERSION` and `BUNDLED WITH` sections of a Bundler lockfile.
fn gemfile_lock(pins: &mut Pins<'_>) {
    let content = pins.file.content;
    let mut section = "";
    for line in content.lines().take(200_000) {
        if !line.starts_with(' ') {
            section = line.trim();
            continue;
        }
        let value = line.trim();
        match section {
            // `ruby 3.2.2p53`, for JRuby `ruby 3.1.4p0 (jruby 9.4.5.0)`.
            "RUBY VERSION" => {
                let mut words = value.strip_prefix("ruby ").unwrap_or_default().splitn(2, ' ');
                let version = words.next().unwrap_or_default();
                let engine = words.next().map(|e| e.trim_matches(['(', ')']).trim());
                let notes = engine
                    .filter(|e| e.len() <= 64 && e.split_whitespace().all(plausible))
                    .map(|e| vec![format!("Engine {e}")])
                    .unwrap_or_default();
                if plausible(version) {
                    pins.add("ruby", version, "RUBY VERSION", Some("RUBY VERSION"), notes);
                }
            }
            "BUNDLED WITH" if plausible(value) => {
                pins.add("bundler", value, "BUNDLED WITH", Some("BUNDLED WITH"), Vec::new());
            }
            _ => continue,
        }
        section = "";
    }
}

/// `required_version` directly inside a top-level `terraform` block. Braces
/// in strings, comments and heredocs are not counted.
fn terraform(pins: &mut Pins<'_>, tool: &str) {
    let content = pins.file.content;
    let (mut depth, mut inside, mut comment) = (0_usize, false, false);
    let mut heredoc: Option<String> = None;
    for line in content.lines().take(200_000) {
        if let Some(marker) = &heredoc {
            if line.trim() == marker {
                heredoc = None;
            }
            continue;
        }
        let parsed = hcl_line(line, &mut comment);
        let opens = depth == 0 && TERRAFORM_BLOCK.is_match(&parsed.code);
        if (opens || (inside && depth == 1))
            && let Some(version) = REQUIRED_VERSION.captures(&parsed.code)
        {
            pins.add(tool, &version[1], "required_version", Some("terraform"), Vec::new());
        }
        inside |= opens;
        depth = (depth + parsed.open).saturating_sub(parsed.close);
        inside &= depth > 0;
        heredoc = parsed.heredoc;
    }
}

struct HclLine {
    code: String,
    open: usize,
    close: usize,
    heredoc: Option<String>,
}

/// One HCL line without comments, its braces outside strings and the end
/// marker of a heredoc it opens.
fn hcl_line(line: &str, comment: &mut bool) -> HclLine {
    let mut parsed = HclLine { code: String::new(), open: 0, close: 0, heredoc: None };
    let (mut string, mut escaped) = (false, false);
    let mut heredoc_at = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        let next = chars.peek().copied();
        if *comment {
            if c == '*' && next == Some('/') {
                chars.next();
                *comment = false;
            }
            continue;
        }
        if string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => string = false,
                _ => {}
            }
            parsed.code.push(c);
            continue;
        }
        match (c, next) {
            ('#', _) | ('/', Some('/')) => break,
            ('/', Some('*')) => {
                chars.next();
                *comment = true;
                continue;
            }
            ('"', _) => string = true,
            ('{', _) => parsed.open += 1,
            ('}', _) => parsed.close += 1,
            ('<', Some('<')) if heredoc_at.is_none() => heredoc_at = Some(parsed.code.len()),
            _ => {}
        }
        parsed.code.push(c);
    }
    parsed.heredoc = heredoc_at.and_then(|at| HEREDOC.captures(&parsed.code[at..])).map(|c| c[1].to_string());
    parsed
}

/// `key=value` pairs of a Java properties file: `#`/`!` comments, `=`, `:`
/// or a blank as separator, backslash escapes (`https\://`), continued lines.
fn properties(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut logical = String::new();
    for raw in text.lines().take(10_000) {
        let line = raw.trim_start();
        if logical.is_empty() && (line.is_empty() || line.starts_with(['#', '!'])) {
            continue;
        }
        let continued = line.chars().rev().take_while(|c| *c == '\\').count() % 2 == 1;
        logical.push_str(if continued { &line[..line.len() - 1] } else { line });
        if continued && logical.len() < 64 * 1024 {
            continue;
        }
        pairs.extend(property(&logical));
        logical.clear();
    }
    pairs.extend(property(&logical));
    pairs
}

fn property(line: &str) -> Option<(String, String)> {
    let mut escaped = false;
    let end = line
        .char_indices()
        .find(|&(_, c)| {
            let separator = !escaped && matches!(c, '=' | ':' | ' ' | '\t' | '\u{c}');
            escaped = !escaped && c == '\\';
            separator
        })
        .map_or(line.len(), |(index, _)| index);
    let key = unescape(&line[..end]);
    let rest = line[end..].trim_start_matches([' ', '\t', '\u{c}']);
    let rest = rest.strip_prefix(['=', ':']).unwrap_or(rest).trim_start_matches([' ', '\t', '\u{c}']);
    (!key.is_empty()).then(|| (key, unescape(rest).trim_end().to_string()))
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// The first capture of `pattern` on a line that is not a comment.
fn first_capture<'t>(text: &'t str, pattern: &Regex, comment: &str) -> Option<&'t str> {
    text.lines()
        .take(20_000)
        .filter(|line| !line.trim_start().starts_with(comment))
        .find_map(|line| pattern.captures(line)?.get(1).map(|m| m.as_str()))
}

fn toml_str<'t>(table: &'t toml::Table, path: &[&str]) -> Option<&'t str> {
    let (first, rest) = path.split_first()?;
    let mut value = table.get(*first)?;
    for key in rest {
        value = value.get(*key)?;
    }
    value.as_str()
}

/// The last segment of an address or path, without query or fragment.
fn file_name(location: &str) -> &str {
    let location = location.split(['?', '#']).next().unwrap_or_default();
    location.rsplit(['/', '\\']).next().unwrap_or_default()
}

/// Whether `rel_dir` ends with these folder names (case-insensitive).
fn in_dir(rel_dir: &str, tail: &[&str]) -> bool {
    let parts: Vec<&str> = rel_dir.split('/').filter(|part| !part.is_empty()).collect();
    parts.len() >= tail.len()
        && parts[parts.len() - tail.len()..].iter().zip(tail).all(|(part, name)| part.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    fn parse(name: &str, rel_dir: &str, content: &str) -> Vec<InventoryItem> {
        Toolchain.parse_file(&FoundFile { name, rel_dir, content, siblings: &[] }).unwrap()
    }

    fn pin<'a>(items: &'a [InventoryItem], name: &str, context: &str) -> &'a InventoryItem {
        items
            .iter()
            .find(|i| i.category == ItemCategory::Package && i.name == name && i.evidence.context == context)
            .unwrap_or_else(|| panic!("{name}/{context}: {items:#?}"))
    }

    fn version<'a>(items: &'a [InventoryItem], name: &str, context: &str) -> Option<&'a str> {
        pin(items, name, context).version.as_deref()
    }

    #[test]
    fn version_files_keep_requests_as_written() {
        let items = parse(".nvmrc", "", "# team default\nlts/*  # newest LTS\n");
        assert_eq!((items[0].category, items[0].ecosystem.as_str()), (ItemCategory::Manifest, "toolchain"));
        assert_eq!(items[0].detail, "Node-Versionsdatei (nvm) · Vorgabe für node");
        let node = pin(&items, "node", ".nvmrc");
        assert_eq!((node.version.as_deref(), node.install_state), (Some("lts/*"), Some(InstallState::Declared)));
        assert_eq!(node.detail, "Vorgabe laut .nvmrc · Alias, nicht aufgelöst");
        let skipped = parse(".nvmrc", "", "node_mirror=ignored\nv20.10.0\n");
        assert_eq!(version(&skipped, "node", ".nvmrc"), Some("v20.10.0"));
        for (name, tool, content, expected) in [
            (".node-version", "node", "20\n", "20"),
            (".ruby-version", "ruby", "ruby-3.2.2 # rvm style\n", "ruby-3.2.2"),
            (".java-version", "java", "17.0\n", "17.0"),
            (".go-version", "go", "1.22.4\n", "1.22.4"),
            (".bun-version", "bun", "1.1.30\n", "1.1.30"),
            (".swift-version", "swift", "5.10\n", "5.10"),
            (".terraform-version", "terraform", "latest:^1.5\n", "latest:^1.5"),
            (".bazelversion", "bazel", "7.4.1\n", "7.4.1"),
            (".dvmrc", "deno", "1.46.3\n", "1.46.3"),
        ] {
            assert_eq!(version(&parse(name, "", content), tool, name), Some(expected), "{name}");
        }
        let range = parse(".terraform-version", "", "latest:^1.5\n");
        assert!(pin(&range, "terraform", ".terraform-version").detail.ends_with("· Bereich"));
        let system = parse(".ruby-version", "", "system\n");
        assert_eq!(pin(&system, "ruby", ".ruby-version").detail, "Vorgabe laut .ruby-version · Systeminstallation");
        let commit = parse(".bazelversion", "", "8f3b2c1d9e0a7b6c5d4e3f2a1b0c9d8e7f6a5b4c\n");
        let bazel = pin(&commit, "bazel", ".bazelversion");
        assert!(bazel.version.is_none() && bazel.detail.contains("Commit nicht gespeichert"));
        assert!(!format!("{commit:?}").contains("8f3b2c1d9e0a"));
        assert!(parse(".nvmrc", "", "\n# only a comment\n").is_empty());
        assert!(parse(".node-version", "", "Bitte die Version aus dem Wiki nehmen\n").is_empty());
    }

    #[test]
    fn python_version_lists_keep_every_entry() {
        let items = parse(".python-version", "", "3.12.1\n# fallback\n3.11.7 # older\n../escape\npypy3.10-7.3.12\n");
        let pythons: Vec<(Option<&str>, &str)> = items
            .iter()
            .filter(|i| i.name == "python")
            .map(|i| (i.version.as_deref(), i.evidence.qualifier.as_str()))
            .collect();
        assert_eq!(pythons, [(Some("3.12.1"), ""), (Some("3.11.7"), "2"), (Some("pypy3.10-7.3.12"), "3")]);
        assert_eq!(version(&parse(".python-versions", "", "3.13\n3.12\n"), "python", ".python-versions"), Some("3.13"));
        assert!(parse(".python-version", "", "../../etc\n").is_empty());
    }

    #[test]
    fn asdf_and_sdkman_lists_map_plugin_names() {
        let text = "# asdf\nnodejs 20.10.0\npython 3.11.4 3.10.13 system # fallbacks\ngolang 1.21.5\nruby path:C:\\src\\ruby\nelixir ref:v1.16.0\nerlang ref:4f0e2a7c9b1d3e5f\nterraform latest\n";
        let items = parse(".tool-versions", "", text);
        assert!(
            items[0].detail.starts_with("asdf-Werkzeugliste · Vorgaben für node, python, go"),
            "{}",
            items[0].detail
        );
        assert_eq!(version(&items, "node", "tool-versions"), Some("20.10.0"));
        assert_eq!(version(&items, "go", "tool-versions"), Some("1.21.5"));
        let pythons: Vec<(Option<&str>, &str)> = items
            .iter()
            .filter(|i| i.name == "python")
            .map(|i| (i.version.as_deref(), i.evidence.qualifier.as_str()))
            .collect();
        assert_eq!(pythons, [(Some("3.11.4"), ""), (Some("3.10.13"), "2"), (Some("system"), "3")]);
        let ruby = pin(&items, "ruby", "tool-versions");
        assert_eq!((ruby.version.as_deref(), ruby.source), (None, SourceClass::Path));
        assert!(!format!("{items:?}").contains("src"));
        let elixir = pin(&items, "elixir", "tool-versions");
        assert_eq!((elixir.version.as_deref(), elixir.source), (Some("ref:v1.16.0"), SourceClass::Git));
        let erlang = pin(&items, "erlang", "tool-versions");
        assert!(erlang.version.is_none() && erlang.detail.contains("Commit"));
        assert!(pin(&items, "terraform", "tool-versions").detail.ends_with("Alias, nicht aufgelöst"));

        let sdkman =
            "# Enable auto-env through the sdkman_auto_env config\njava=21.0.1-tem\ngradle=8.5\nmaven = 3.9.6\n";
        let items = parse(".sdkmanrc", "", sdkman);
        assert_eq!(version(&items, "java", ".sdkmanrc"), Some("21.0.1-tem"));
        assert_eq!(version(&items, "gradle", ".sdkmanrc"), Some("8.5"));
        assert_eq!(version(&items, "maven", ".sdkmanrc"), Some("3.9.6"));
    }

    #[test]
    fn mise_tools_lists_tables_and_minimum() {
        let text = r#"min_version = { hard = "2024.9.0", soft = "2025.1.0" }
[env]
API_TOKEN = "s3cr3t-value"
[tools]
node = "20"
python = ["3.11", "3.12"]
"npm:prettier" = "latest"
ruby = { version = "3.3", postinstall = "gem install bundler" }
go = { os = ["linux"] }
"core:java" = "21"
[plugins]
custom = "https://user:pw@git.corp.example/mise-custom.git"
"#;
        let items = parse("mise.toml", "", text);
        assert_eq!(version(&items, "node", "mise"), Some("20"));
        let pythons: Vec<Option<&str>> =
            items.iter().filter(|i| i.name == "python").map(|i| i.version.as_deref()).collect();
        assert_eq!(pythons, [Some("3.11"), Some("3.12")]);
        assert_eq!(version(&items, "npm:prettier", "mise"), Some("latest"));
        assert_eq!(version(&items, "ruby", "mise"), Some("3.3"));
        assert_eq!(version(&items, "java", "mise"), Some("21"));
        assert!(items.iter().all(|i| i.name != "go"), "options without a version are no pin");
        let mise = pin(&items, "mise", "min_version");
        assert_eq!(mise.version.as_deref(), Some("2024.9.0"));
        assert!(mise.detail.contains("Mindestversion · empfohlen 2025.1.0"), "{}", mise.detail);
        let debug = format!("{items:?}");
        assert!(!debug.contains("s3cr3t") && !debug.contains("corp.example") && !debug.contains("gem install"));
        assert!(Toolchain.parse_file(&file("mise.toml", "[tools\nnode = 1")).is_err());
        let local = parse("mise.local.toml", ".config", "min_version = \"2025.2.0\"\n");
        assert_eq!(version(&local, "mise", "min_version"), Some("2025.2.0"));
        assert!(DESCRIPTOR.wants(".mise.local.toml") && DESCRIPTOR.wants("mise.production.toml"));
        assert!(!DESCRIPTOR.wants("config.toml"), "config.toml stays with the AI profiles");
    }

    #[test]
    fn rust_toolchain_files_new_and_legacy() {
        let text = "[toolchain]\nchannel = \"nightly-2024-01-01\"\ncomponents = [\"rustfmt\", \"clippy\"]\ntargets = [\"wasm32-unknown-unknown\"]\nprofile = \"minimal\"\n";
        let items = parse("rust-toolchain.toml", "", text);
        let rust = pin(&items, "rust", "rust-toolchain");
        assert_eq!(rust.version.as_deref(), Some("nightly-2024-01-01"));
        assert_eq!(
            rust.detail,
            "Vorgabe laut rust-toolchain.toml · Profil minimal · Komponenten rustfmt, clippy · Ziele wasm32-unknown-unknown"
        );
        assert_eq!(version(&parse("rust-toolchain", "", "1.75.0\n"), "rust", "rust-toolchain"), Some("1.75.0"));
        let legacy_toml = parse("rust-toolchain", "", "[toolchain]\nchannel = \"stable\"\n");
        assert_eq!(version(&legacy_toml, "rust", "rust-toolchain"), Some("stable"));
        let custom = parse("rust-toolchain.toml", "", "[toolchain]\npath = 'C:\\Users\\someone\\toolchains\\custom'\n");
        let rust = pin(&custom, "rust", "rust-toolchain");
        assert_eq!((rust.version.as_deref(), rust.source), (None, SourceClass::Path));
        assert!(!format!("{custom:?}").contains("someone"));
        assert!(Toolchain.parse_file(&file("rust-toolchain.toml", "[toolchain\n")).is_err());
    }

    #[test]
    fn global_json_sdk_and_msbuild_sdks() {
        let text = r#"{
  // pinned for CI
  "sdk": { "version": "8.0.302", "rollForward": "latestFeature", "allowPrerelease": false, "paths": [".dotnet", "$host$"] },
  "msbuild-sdks": { "Microsoft.Build.Traversal": "4.1.0", "MSBuild.Sdk.Extras": "3.0.44" },
}"#;
        let items = parse("global.json", "", text);
        let sdk = pin(&items, "dotnet-sdk", "global.json");
        assert_eq!(sdk.version.as_deref(), Some("8.0.302"));
        assert_eq!(
            sdk.detail,
            "Vorgabe laut global.json · rollForward latestFeature · ohne Vorabversionen · eigene SDK-Suchpfade (nicht gespeichert)"
        );
        assert!(!format!("{items:?}").contains("$host$"));
        assert_eq!(version(&items, "msbuild-sdk:Microsoft.Build.Traversal", "msbuild-sdks"), Some("4.1.0"));
        assert_eq!(version(&items, "msbuild-sdk:MSBuild.Sdk.Extras", "msbuild-sdks"), Some("3.0.44"));
        assert!(parse("global.json", "", r#"{"name": "web-app", "port": 8080}"#).is_empty());
        assert!(parse("global.json", "", "{broken").is_empty());
    }

    #[test]
    fn wrapper_properties_keep_public_hosts_only_and_never_the_address() {
        let gradle = "distributionBase=GRADLE_USER_HOME\ndistributionPath=wrapper/dists\ndistributionUrl=https\\://services.gradle.org/distributions/gradle-8.5-bin.zip\ndistributionSha256Sum=9d926787066a081739e8200858338b4a69e837c3a821a33aca9db09dd4a41026\nnetworkTimeout=10000\n";
        let items = parse("gradle-wrapper.properties", "gradle/wrapper", gradle);
        let item = pin(&items, "gradle", "wrapper");
        assert_eq!(item.version.as_deref(), Some("8.5"));
        assert_eq!(
            (item.source, item.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("services.gradle.org"))
        );
        assert_eq!(item.detail, "Vorgabe laut gradle-wrapper.properties · Distribution bin · Prüfsumme festgelegt");
        assert!(!format!("{items:?}").contains("9d926787"));
        let private = "distributionUrl=https\\://ci-user\\:s3cret@gradle.corp.example/mirror/gradle-8.10.2-all.zip\n";
        let items = parse("gradle-wrapper.properties", "app/gradle/wrapper", private);
        let item = pin(&items, "gradle", "wrapper");
        assert_eq!((item.version.as_deref(), item.source), (Some("8.10.2"), SourceClass::PrivateRegistry));
        assert!(item.source_host.is_none(), "an own distribution server stays unnamed");
        assert!(item.detail.ends_with("eigener Verteilungsort"));
        let debug = format!("{items:?}");
        assert!(!debug.contains("s3cret") && !debug.contains("ci-user") && !debug.contains("corp.example"));
        assert!(parse("gradle-wrapper.properties", "", gradle).is_empty(), "only gradle/wrapper is read by gradlew");

        let maven = "wrapperVersion=3.3.2\ndistributionType=only-script\ndistributionUrl=https://repo.maven.apache.org/maven2/org/apache/maven/apache-maven/3.9.9/apache-maven-3.9.9-bin.zip\n";
        let items = parse("maven-wrapper.properties", ".mvn/wrapper", maven);
        let item = pin(&items, "maven", "wrapper");
        assert_eq!((item.version.as_deref(), item.source), (Some("3.9.9"), SourceClass::PublicRegistry));
        assert!(item.detail.contains("Wrapper 3.3.2"));
        let legacy = "distributionUrl=https://repo.maven.apache.org/maven2/org/apache/maven/apache-maven/3.6.3/apache-maven-3.6.3-bin.zip\nwrapperUrl=https://repo.maven.apache.org/maven2/io/takari/maven-wrapper/0.5.6/maven-wrapper-0.5.6.jar\n";
        let items = parse("maven-wrapper.properties", ".mvn/wrapper", legacy);
        assert!(pin(&items, "maven", "wrapper").detail.contains("Wrapper 0.5.6"));
        let mvnd =
            "distributionUrl=https://archive.apache.org/dist/maven/mvnd/1.0.2/maven-mvnd-1.0.2-windows-amd64.zip\n";
        assert_eq!(version(&parse("maven-wrapper.properties", ".mvn/wrapper", mvnd), "mvnd", "wrapper"), Some("1.0.2"));
    }

    #[test]
    fn sbt_runtime_yarn_and_fvm_files() {
        let sbt = parse("build.properties", "project", "sbt.version=1.9.7\n");
        assert_eq!(version(&sbt, "sbt", "build.properties"), Some("1.9.7"));
        assert!(parse("build.properties", "", "sbt.version=1.9.7\n").is_empty(), "sbt reads project/ only");
        assert!(parse("build.properties", "project", "version.code=12\n").is_empty());
        assert_eq!(version(&parse("runtime.txt", "", "python-3.11.4\n"), "python", "runtime.txt"), Some("3.11.4"));
        assert!(parse("runtime.txt", "", "web: gunicorn app:app\n").is_empty());
        let yarnrc = "nodeLinker: node-modules\nyarnPath: .yarn/releases/yarn-4.1.0.cjs\nnpmRegistryServer: \"https://npm.corp.example\"\n";
        let items = parse(".yarnrc.yml", "", yarnrc);
        assert_eq!(version(&items, "yarn", "yarnPath"), Some("4.1.0"));
        let debug = format!("{items:?}");
        assert!(!debug.contains(".yarn/releases") && !debug.contains("corp.example"));
        assert!(parse(".yarnrc.yml", "", "nodeLinker: pnp\n").is_empty());
        let fvmrc = r#"{"flutter": "3.19.0", "flavors": {"production": "3.16.9"}, "updateVscodeSettings": true}"#;
        let items = parse(".fvmrc", "", fvmrc);
        assert_eq!(version(&items, "flutter", "fvm"), Some("3.19.0"));
        assert!(
            items.iter().any(|i| i.evidence.qualifier == "flavor:production" && i.version.as_deref() == Some("3.16.9"))
        );
        let legacy = parse("fvm_config.json", ".fvm", r#"{"flutterSdkVersion": "3.10.0"}"#);
        assert_eq!(version(&legacy, "flutter", "fvm"), Some("3.10.0"));
        assert!(Toolchain.parse_file(&file(".fvmrc", "{")).is_err());
    }

    #[test]
    fn package_json_engines_package_manager_volta_and_dev_engines() {
        let text = r#"{
  "name": "web",
  "engines": { "node": ">=18 <21", "npm": "~10.2.0", "vscode": "^1.80.0", "yarn": "*", "../evil": "1" },
  "packageManager": "pnpm@9.1.0+sha512.6f4b3c2a1d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b",
  "volta": { "node": "20.10.0", "yarn": "4.0.2", "extends": "../package.json" },
  "devEngines": {
    "runtime": { "name": "node", "version": ">=20", "onFail": "error" },
    "packageManager": [{ "name": "pnpm", "version": "9.x" }, { "name": "bun" }]
  },
  "dependencies": { "left-pad": "^1.3.0" }
}"#;
        let items = parse("package.json", "", text);
        assert_eq!(items[0].category, ItemCategory::Manifest);
        let node = pin(&items, "node", "engines");
        assert_eq!(node.version.as_deref(), Some(">=18 <21"));
        assert_eq!(node.detail, "Vorgabe laut package.json (engines) · Bereich");
        assert_eq!(version(&items, "vscode", "engines"), Some("^1.80.0"));
        assert!(pin(&items, "yarn", "engines").detail.ends_with("· Bereich"), "a wildcard is a range");
        assert!(items.iter().all(|i| !i.name.contains("evil")), "path-like names are no tools");
        let pnpm = pin(&items, "pnpm", "packageManager");
        assert_eq!(pnpm.version.as_deref(), Some("9.1.0"));
        assert!(pnpm.detail.ends_with("Prüfsumme festgelegt"));
        assert!(!format!("{items:?}").contains("sha512"));
        assert_eq!(version(&items, "node", "volta"), Some("20.10.0"));
        assert_eq!(version(&items, "yarn", "volta"), Some("4.0.2"));
        assert!(items.iter().all(|i| i.name != "extends" && i.name != "left-pad"));
        assert_eq!(version(&items, "node", "devEngines"), Some(">=20"));
        assert_eq!(version(&items, "pnpm", "devEngines"), Some("9.x"));
        let bun = pin(&items, "bun", "devEngines");
        assert!(bun.version.is_none() && bun.detail.contains("ohne Version"));

        let url = r#"{"packageManager": "yarn@https://registry.npmjs.org/@yarnpkg/cli-dist/-/cli-dist-3.2.3.tgz#sha224.16a0797d1710d1fb7ec40ab5c3801b68370a612a9b66ba117ad9924b"}"#;
        let items = parse("package.json", "", url);
        let yarn = pin(&items, "yarn", "packageManager");
        assert_eq!((yarn.version.as_deref(), yarn.source), (None, SourceClass::PublicRegistry));
        assert_eq!(yarn.source_host.as_deref(), Some("registry.npmjs.org"));
        assert!(!format!("{items:?}").contains("cli-dist"));
        let private = r#"{"packageManager": "pnpm@https://ci:t0ken@npm.corp.example/pnpm-9.1.0.tgz"}"#;
        let items = parse("package.json", "", private);
        let pnpm = pin(&items, "pnpm", "packageManager");
        assert_eq!((pnpm.source, pnpm.source_host.as_deref()), (SourceClass::PrivateRegistry, None));
        let debug = format!("{items:?}");
        assert!(!debug.contains("t0ken") && !debug.contains("corp.example"));
        assert!(parse("package.json", "", r#"{"name": "plain", "dependencies": {}}"#).is_empty());
        assert!(parse("package.json", "", "{not json").is_empty(), "npm reports broken manifests");
    }

    #[test]
    fn go_cargo_and_python_manifests() {
        let go_mod = "module example.com/app\n\ngo 1.22 // minimum\n\ntoolchain go1.22.4\n\nrequire (\n\tgolang.org/x/tools v0.20.0\n\tgo.uber.org/zap v1.27.0\n)\n";
        let items = parse("go.mod", "", go_mod);
        assert_eq!(items.len(), 3);
        assert_eq!(version(&items, "go", "go"), Some("1.22"));
        assert_eq!(version(&items, "go", "toolchain"), Some("1.22.4"));
        assert_eq!(version(&parse("go.work", "", "go 1.23.0\n\nuse (\n\t./api\n)\n"), "go", "go"), Some("1.23.0"));
        assert!(parse("go.mod", "", "module example.com/legacy\n").is_empty());

        let cargo =
            "[package]\nname = \"app\"\nrust-version = \"1.70\"\n\n[workspace.package]\nrust-version = \"1.65\"\n";
        let items = parse("Cargo.toml", "", cargo);
        assert_eq!(version(&items, "rust", "rust-version"), Some("1.70"));
        assert_eq!(version(&items, "rust", "workspace.rust-version"), Some("1.65"));
        assert!(parse("Cargo.toml", "crates/a", "[package]\nname = \"a\"\nrust-version.workspace = true\n").is_empty());

        let pyproject = "[project]\nname = \"demo\"\nrequires-python = \">=3.10\"\n\n[tool.poetry]\nrequires-poetry = \">=2.0\"\n\n[tool.poetry.dependencies]\npython = \"^3.10\"\nrequests = \"^2.31\"\n\n[tool.uv]\nrequired-version = \">=0.5.0\"\n";
        let items = parse("pyproject.toml", "", pyproject);
        assert_eq!(version(&items, "python", "requires-python"), Some(">=3.10"));
        assert_eq!(version(&items, "python", "poetry"), Some("^3.10"));
        assert_eq!(version(&items, "poetry", "requires-poetry"), Some(">=2.0"));
        assert_eq!(version(&items, "uv", "required-version"), Some(">=0.5.0"));
        assert!(items.iter().all(|i| i.name != "requests"));
        let uv = parse("uv.toml", "", "required-version = \"==0.5.0\"\n");
        let pinned = pin(&uv, "uv", "required-version");
        assert_eq!((pinned.version.as_deref(), pinned.detail.as_str()), (Some("==0.5.0"), "Vorgabe laut uv.toml"));
        let pipfile = "[[source]]\nurl = \"https://pypi.org/simple\"\n\n[packages]\nrequests = \"*\"\n\n[requires]\npython_version = \"3.11\"\npython_full_version = \"3.11.4\"\n";
        let items = parse("Pipfile", "", pipfile);
        assert_eq!(version(&items, "python", "python_version"), Some("3.11"));
        assert_eq!(version(&items, "python", "python_full_version"), Some("3.11.4"));
        assert!(parse("pyproject.toml", "", "[project\n").is_empty(), "pip reports broken manifests");
    }

    #[test]
    fn composer_and_pubspec_runtime_constraints() {
        let composer = r#"{"require": {"php": ">=8.1", "ext-mbstring": "*", "laravel/framework": "^11.0"}, "require-dev": {"phpunit/phpunit": "^10"}, "config": {"platform": {"php": "8.1.0"}}}"#;
        let items = parse("composer.json", "", composer);
        assert_eq!(version(&items, "php", "require"), Some(">=8.1"));
        assert_eq!(version(&items, "php", "config.platform"), Some("8.1.0"));
        assert!(items.iter().all(|i| !i.name.starts_with("ext-") && !i.name.contains('/')));
        let pubspec = "name: app\nenvironment:\n  sdk: \">=3.0.0 <4.0.0\"\n  flutter: '>=3.10.0' # minimum\ndependencies:\n  flutter:\n    sdk: flutter\nflutter:\n  uses-material-design: true\n";
        let items = parse("pubspec.yaml", "", pubspec);
        assert_eq!(items.len(), 3);
        assert_eq!(version(&items, "dart", "environment"), Some(">=3.0.0 <4.0.0"));
        assert_eq!(version(&items, "flutter", "environment"), Some(">=3.10.0"));
        assert!(parse("pubspec.yaml", "", "name: tool\ndependencies:\n  path: ^1.9.0\n").is_empty());
    }

    #[test]
    fn ruby_elixir_swift_zig_julia_and_r_pins() {
        let gemfile = "source \"https://rubygems.org\"\n# ruby \"2.7.0\"\nruby \"3.2.2\", engine: \"jruby\", engine_version: \"9.4.5.0\"\ngem \"rails\", \"~> 7.1\"\n";
        let items = parse("Gemfile", "", gemfile);
        assert_eq!(items.len(), 2);
        let ruby = pin(&items, "ruby", "ruby");
        assert_eq!(ruby.version.as_deref(), Some("3.2.2"));
        assert_eq!(ruby.detail, "Vorgabe laut Gemfile (ruby) · Engine jruby 9.4.5.0");
        assert_eq!(version(&parse("gems.rb", "", "ruby('>= 3.1', '< 3.4')\n"), "ruby", "ruby"), Some(">= 3.1, < 3.4"));
        let from_file = parse("Gemfile", "", "ruby file: \".ruby-version\"\n");
        assert!(pin(&from_file, "ruby", "ruby").version.is_none());
        assert!(parse("Gemfile", "", "ruby RUBY_VERSION\nruby_version = '3.0'\ngem 'ruby-progressbar'\n").is_empty());
        let lock = "GEM\n  remote: https://rubygems.org/\n  specs:\n    rake (13.1.0)\n\nPLATFORMS\n  x64-mingw-ucrt\n\nRUBY VERSION\n   ruby 3.1.4p0 (jruby 9.4.5.0)\n\nBUNDLED WITH\n   2.4.10\n";
        let items = parse("Gemfile.lock", "", lock);
        let ruby = pin(&items, "ruby", "RUBY VERSION");
        assert_eq!(ruby.version.as_deref(), Some("3.1.4p0"));
        assert!(ruby.detail.ends_with("Engine jruby 9.4.5.0"));
        assert_eq!(version(&items, "bundler", "BUNDLED WITH"), Some("2.4.10"));
        assert!(items.iter().all(|i| i.name != "rake"));

        let mix = "defmodule App.MixProject do\n  use Mix.Project\n  # elixir: \"~> 1.0\"\n  def project do\n    [app: :app, version: \"0.1.0\", elixir: \"~> 1.15\", deps: deps()]\n  end\nend\n";
        assert_eq!(version(&parse("mix.exs", "", mix), "elixir", "elixir"), Some("~> 1.15"));
        assert!(parse("mix.exs", "", "config :app, elixir: \"~> 1.15\"\n").is_empty());
        let swift = "// swift-tools-version:5.9\nimport PackageDescription\n";
        assert_eq!(version(&parse("Package.swift", "", swift), "swift", "swift-tools-version"), Some("5.9"));
        assert!(parse("Package.swift", "", "import PackageDescription\n// swift-tools-version:5.9\n").is_empty());
        let zon = ".{\n    .name = .app,\n    .version = \"0.1.0\",\n    // .minimum_zig_version = \"0.11.0\",\n    .minimum_zig_version = \"0.13.0\",\n}\n";
        assert_eq!(version(&parse("build.zig.zon", "", zon), "zig", "minimum_zig_version"), Some("0.13.0"));
        let julia = "name = \"App\"\n[deps]\nJSON = \"682c06a0-de6a-54ab-a142-c8b1cf79cde6\"\n[compat]\njulia = \"1.9\"\nJSON = \"0.21\"\n";
        let items = parse("Project.toml", "", julia);
        assert_eq!(items.len(), 2);
        assert_eq!(version(&items, "julia", "compat"), Some("1.9"));
        let renv = r#"{"R": {"Version": "4.3.1", "Repositories": [{"Name": "CRAN", "URL": "https://cloud.r-project.org"}]}, "Packages": {}}"#;
        assert_eq!(version(&parse("renv.lock", "", renv), "r", "R.Version"), Some("4.3.1"));
    }

    #[test]
    fn terraform_required_version_only_in_the_terraform_block() {
        let main = r#"terraform {
  required_version = ">= 1.5.0" # CLI
  required_providers {
    aws = { source = "hashicorp/aws", version = "~> 5.0" }
  }
  backend "s3" {
    bucket = "state-{env}"
  }
}

variable "doc" {
  default = <<EOT
terraform {
  required_version = "0.11.0"
}
EOT
}

/* terraform {
  required_version = "0.12.0"
} */

resource "null_resource" "x" {
  required_version = "9.9.9"
}
"#;
        let items = parse("main.tf", "", main);
        let terraform: Vec<&InventoryItem> = items.iter().filter(|i| i.name == "terraform").collect();
        assert_eq!(terraform.len(), 1, "{items:#?}");
        assert_eq!(terraform[0].version.as_deref(), Some(">= 1.5.0"));
        assert_eq!(terraform[0].detail, "Vorgabe laut main.tf (terraform) · Bereich");
        assert!(items.iter().all(|i| i.name != "aws"));
        let tofu = parse("versions.tofu", "", "terraform { required_version = \"~> 1.8\" }\n");
        assert_eq!(version(&tofu, "opentofu", "required_version"), Some("~> 1.8"));
        assert!(parse("outputs.tf", "", "output \"x\" { value = 1 }\n").is_empty());
    }

    #[test]
    fn shared_names_with_foreign_content_stay_empty() {
        for (name, rel_dir, content) in [
            ("package.json", "", r#"["not", "a", "manifest"]"#),
            ("composer.json", "", r#"{"require": {"monolog/monolog": "^3"}}"#),
            ("Gemfile", "", "source 'https://rubygems.org'\ngem 'rack'\n"),
            ("global.json", "", r#"{"sdk": "not an object"}"#),
            ("build.properties", "project", "sbt.version=\n"),
            ("Project.toml", "", "name = \"x\"\n[compat]\nJSON = \"0.21\"\n"),
            ("renv.lock", "", "{}"),
            ("gradle-wrapper.properties", "gradle/wrapper", "networkTimeout=10000\n"),
            (
                "maven-wrapper.properties",
                "tools",
                "distributionUrl=https://repo.maven.apache.org/maven2/org/apache/maven/apache-maven/3.9.9/apache-maven-3.9.9-bin.zip\n",
            ),
            ("fvm_config.json", "", r#"{"flutterSdkVersion": "3.10.0"}"#),
            ("readme.txt", "", "node 20\n"),
        ] {
            assert!(parse(name, rel_dir, content).is_empty(), "{name} in {rel_dir:?}");
        }
    }

    #[test]
    fn properties_unescape_and_continue_lines() {
        let pairs =
            properties("# comment\n! also\nkey = value\nurl=https\\://host/x\nlong = a\\\n    b\nspaced\\ key:v\n");
        let expected = [("key", "value"), ("url", "https://host/x"), ("long", "ab"), ("spaced key", "v")];
        assert_eq!(pairs, expected.map(|(k, v)| (k.to_string(), v.to_string())));
    }
}
