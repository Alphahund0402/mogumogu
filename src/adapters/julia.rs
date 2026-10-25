//! Julia (Pkg): Project.toml / JuliaProject.toml (declared: deps, weak
//! deps, test extras, compat bounds and sources) and Manifest.toml /
//! JuliaManifest.toml / Manifest-v*.toml (resolved, formats 1.0 to 2.1).
//! No Julia process, no registry and no depot is touched; standard
//! libraries appear only as edges. Gleam's manifest.toml is left alone.
use super::nix::{git_host, short_ref};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::toml_table;

pub struct Julia;

/// UUID of the public General registry.
const GENERAL: &str = "23338594-aafe-5451-b93e-139f81909106";

static DESCRIPTOR: Descriptor = Descriptor {
    id: "julia",
    name: "Julia (Pkg)",
    markers: &[
        "Project.toml",
        "JuliaProject.toml",
        "Manifest.toml",
        "JuliaManifest.toml",
        "Manifest-v*.toml",
        "JuliaManifest-v*.toml",
    ],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "Project.toml ([deps], [weakdeps], [extras]/[targets], [compat], [sources])",
        "Manifest.toml 1.0, 2.0, 2.1",
    ],
    limits: "Kein julia-Aufruf; Depot und Registries werden nicht gelesen – vor Format 2.1 bleibt die Registry eines Pakets unbekannt; Standardbibliotheken nur als Abhängigkeitsverweis.",
};

impl Adapter for Julia {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let lower = file.name.to_ascii_lowercase();
        let julia_named = lower.starts_with("julia");
        if lower.ends_with("project.toml") {
            bounded(project(file, julia_named)?)
        } else {
            bounded(manifest_file(file, julia_named)?)
        }
    }
}

/// Parses the TOML; a broken file is an error only when it is clearly Julia's.
fn table(file: &FoundFile<'_>, julia: bool, markers: &[&str]) -> Result<Option<toml::Table>, String> {
    match toml_table(file.content) {
        Ok(table) => Ok(Some(table)),
        Err(e) if julia || markers.iter().any(|m| file.content.contains(m)) => Err(e.to_string()),
        Err(_) => Ok(None),
    }
}

fn project(file: &FoundFile<'_>, julia_named: bool) -> AdapterResult {
    let Some(table) = table(file, julia_named, &["[deps]", "\nuuid ="])? else { return Ok(Vec::new()) };
    let section = |key: &str| table.get(key).and_then(toml::Value::as_table);
    if section("deps").is_none() && !table.contains_key("uuid") && section("workspace").is_none() {
        return Ok(Vec::new());
    }
    let text = |key: &str| table.get(key).and_then(toml::Value::as_str);
    let compat = section("compat");
    let sources = section("sources");
    let tests: std::collections::HashSet<&str> = section("targets")
        .and_then(|t| t.get("test"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .take(5_000)
        .collect();
    let mut detail = text("name").map_or("Julia-Projekt".to_string(), |name| format!("Paket {name}"));
    if let Some(version) = text("version") {
        detail.push_str(&format!(" {version}"));
    }
    if let Some(julia) = compat.and_then(|c| c.get("julia")).and_then(toml::Value::as_str) {
        detail.push_str(&format!(" · Julia {julia}"));
    }
    if let Some(projects) = section("workspace").and_then(|w| w.get("projects")).and_then(toml::Value::as_array) {
        detail.push_str(&format!(" · Workspace mit {} Projekten", projects.len()));
    }
    let mut items = vec![manifest("julia", file, detail)];
    for key in ["deps", "weakdeps", "extras"] {
        for (name, _) in section(key).into_iter().flatten().take(5_000) {
            let context = match key {
                "weakdeps" => "weak",
                "extras" if tests.contains(&name.as_str()) => "test",
                "extras" => "extras",
                _ => "",
            };
            let spec = compat.and_then(|c| c.get(name)).and_then(toml::Value::as_str);
            let mut detail = spec.map_or("ohne compat-Eintrag".into(), |s| format!("Spezifikation {s}"));
            let origin = sources.and_then(|s| s.get(name)).and_then(toml::Value::as_table);
            let url = origin.and_then(|o| o.get("url")).and_then(toml::Value::as_str);
            let (source, host) = match (origin, url) {
                (Some(origin), _) if origin.contains_key("path") => {
                    detail.push_str(" · lokaler Pfad");
                    (SourceClass::Path, None)
                }
                (Some(origin), Some(url)) => {
                    detail.push_str(" · Git-Quelle");
                    if let Some(rev) = origin.get("rev").and_then(toml::Value::as_str).and_then(short_ref) {
                        detail.push_str(&format!(" · Rev {rev}"));
                    }
                    (SourceClass::Git, git_host(url))
                }
                _ => (SourceClass::Unknown, None),
            };
            if !context.is_empty() {
                detail.push_str(&format!(" · {context}"));
            }
            let mut item = InventoryItem::new(ItemCategory::Package, "julia", name.as_str(), file.rel_path())
                .state(InstallState::Declared)
                .source(source, host)
                .detail(detail);
            item.evidence.context = context.into();
            items.push(item);
        }
    }
    Ok(items)
}

/// `name → [{uuid, …}, …]` arrays of a manifest table.
fn entries(table: Option<&toml::Table>) -> Vec<(&String, &toml::Table)> {
    table
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| Some((name, value.as_array()?)))
        .flat_map(|(name, list)| {
            list.iter().filter_map(toml::Value::as_table).filter(|e| e.contains_key("uuid")).map(move |e| (name, e))
        })
        .take(20_000)
        .collect()
}

fn package_array(value: &toml::Value) -> bool {
    value.as_array().is_some_and(|list| {
        !list.is_empty() && list.iter().all(|e| e.as_table().is_some_and(|t| t.contains_key("uuid")))
    })
}

fn manifest_file(file: &FoundFile<'_>, julia_named: bool) -> AdapterResult {
    let markers = ["julia_version", "manifest_format", "[[deps.", "git-tree-sha1"];
    let Some(table) = table(file, julia_named, &markers)? else { return Ok(Vec::new()) };
    let format = table.get("manifest_format").and_then(toml::Value::as_str);
    // Format 1.0 keeps the packages at the top level, 2.x below `deps`.
    let packages = match format {
        Some(_) => entries(table.get("deps").and_then(toml::Value::as_table)),
        None if !table.is_empty() && table.values().all(package_array) => entries(Some(&table)),
        None => return Ok(Vec::new()),
    };
    let registries = table.get("registries").and_then(toml::Value::as_table).filter(|_| format.is_some());
    let (mut items, mut standard) = (Vec::new(), 0);
    for (name, entry) in packages {
        let text = |key: &str| entry.get(key).and_then(toml::Value::as_str);
        let (source, host, origin) = if entry.contains_key("path") {
            (SourceClass::Path, None, " · lokaler Pfad (entwickelt)".to_string())
        } else if let Some(url) = text("repo-url") {
            let origin = text("repo-rev").and_then(short_ref).map_or(" · Git".into(), |r| format!(" · Git-Ref {r}"));
            (SourceClass::Git, git_host(url), origin)
        } else if entry.contains_key("git-tree-sha1") {
            registry(entry, registries)
        } else {
            // A standard library ships with Julia and has no tree hash.
            standard += 1;
            continue;
        };
        let mut item = InventoryItem::new(ItemCategory::Package, "julia", name.as_str(), file.rel_path())
            .version(text("version").unwrap_or_default())
            .state(InstallState::Resolved)
            .source(source, host)
            .detail(format!("aufgelöst laut {}{origin}", file.name));
        item.evidence.qualifier = text("uuid").unwrap_or_default().to_string();
        let dependencies: Vec<String> = match entry.get("deps") {
            Some(toml::Value::Array(list)) => list.iter().filter_map(toml::Value::as_str).map(str::to_string).collect(),
            Some(toml::Value::Table(table)) => table.keys().cloned().collect(),
            _ => Vec::new(),
        };
        item.evidence.dependencies = dependencies
            .into_iter()
            .take(1_000)
            .map(|name| PackageDependency { name, requirement: String::new(), kind: "deps".into(), exact: false })
            .collect();
        items.push(item);
    }
    let mut detail = format!("Manifest-Format {}", format.unwrap_or("1.0"));
    if let Some(julia) = table.get("julia_version").and_then(toml::Value::as_str) {
        detail.push_str(&format!(" · Julia {julia}"));
    }
    detail.push_str(&format!(" · {} Pakete", items.len()));
    if standard > 0 {
        detail.push_str(&format!(" · {standard} Standardbibliotheken nicht einzeln aufgeführt"));
    }
    items.insert(0, manifest("julia", file, detail));
    Ok(items)
}

/// The registry of a registered package, recorded from format 2.1 on.
fn registry(entry: &toml::Table, registries: Option<&toml::Table>) -> (SourceClass, Option<String>, String) {
    let name = match entry.get("registries") {
        Some(toml::Value::String(name)) => Some(name.as_str()),
        Some(toml::Value::Array(list)) => list.first().and_then(toml::Value::as_str),
        _ => None,
    };
    let Some(name) = name else { return (SourceClass::Unknown, None, " · Registry nicht vermerkt".into()) };
    let registry = registries.and_then(|r| r.get(name)).and_then(toml::Value::as_table);
    let host = registry.and_then(|r| r.get("url")).and_then(toml::Value::as_str).and_then(git_host);
    let general = registry.and_then(|r| r.get("uuid")).and_then(toml::Value::as_str) == Some(GENERAL);
    let source = match (general, &host) {
        (true, _) => SourceClass::PublicRegistry,
        (false, Some(_)) => SourceClass::PrivateRegistry,
        (false, None) => SourceClass::Unknown,
    };
    (source, host, format!(" · Registry {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    const PROJECT: &str = r#"name = "Example"
uuid = "7876af07-990d-54b4-ab0e-23690620f79a"
authors = ["Jane Doe <jane@example.com>"]
version = "1.2.5"

[deps]
Dates = "ade2ca70-3891-5945-98fb-dc099432e06a"
JSON = "682c06a0-de6a-54ab-a142-c8b1cf79cde6"
Private = "d9f0e8c1-0000-4000-8000-000000000001"
Dev = "d9f0e8c1-0000-4000-8000-000000000002"

[weakdeps]
Contour = "d38c429a-6771-53c6-b99e-75d170b6e991"

[extensions]
ContourExt = "Contour"

[sources]
Private = {url = "https://ci:hunter2@git.corp.example/julia/Private.jl", rev = "main"}
Dev = {path = "dev/Dev"}

[compat]
JSON = "0.21"
Contour = "0.6.2"
julia = "1.10"

[extras]
Test = "8dfed614-e22c-5e08-85e1-65c5234f0b40"
Aqua = "4c88cf16-eb10-579e-8560-4a9242c79595"

[targets]
test = ["Test"]
"#;

    #[test]
    fn project_declares_deps_with_compat_and_sources() {
        let items = Julia.parse_file(&file("Project.toml", PROJECT)).unwrap();
        assert_eq!(items[0].detail, "Paket Example 1.2.5 · Julia 1.10");
        assert_eq!(items.len(), 8, "{items:#?}");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let json = find("JSON");
        assert_eq!((json.detail.as_str(), json.install_state), ("Spezifikation 0.21", Some(InstallState::Declared)));
        assert_eq!(find("Dates").detail, "ohne compat-Eintrag");
        let private = find("Private");
        assert_eq!((private.source, private.source_host.as_deref()), (SourceClass::Git, Some("git.corp.example")));
        assert_eq!(private.detail, "ohne compat-Eintrag · Git-Quelle · Rev main");
        assert_eq!(find("Dev").source, SourceClass::Path);
        let contour = find("Contour");
        assert_eq!(
            (contour.evidence.context.as_str(), contour.detail.as_str()),
            ("weak", "Spezifikation 0.6.2 · weak")
        );
        assert_eq!(find("Test").evidence.context, "test");
        assert_eq!(find("Aqua").evidence.context, "extras");
        assert!(!items.iter().any(|i| i.name == "Example" || i.name == "julia" || i.name == "ContourExt"));
        let text = format!("{items:?}");
        assert!(!text.contains("hunter2") && !text.contains("julia/Private") && !text.contains("jane@"));
    }

    #[test]
    fn foreign_and_broken_project_files() {
        assert!(Julia.parse_file(&file("Project.toml", "[project]\nname = \"x\"\n")).unwrap().is_empty());
        assert!(Julia.parse_file(&file("Project.toml", "[project\nname = \n")).unwrap().is_empty());
        assert!(Julia.parse_file(&file("Project.toml", "[deps]\nJSON = \n")).is_err());
        assert!(Julia.parse_file(&file("JuliaProject.toml", "name = ")).is_err());
        let workspace = "[workspace]\nprojects = [\"test\", \"docs\"]\n";
        let items = Julia.parse_file(&file("Project.toml", workspace)).unwrap();
        assert_eq!(items[0].detail, "Julia-Projekt · Workspace mit 2 Projekten");
    }

    const MANIFEST: &str = r#"# This file is machine-generated - editing it directly is not advised

julia_version = "1.10.4"
manifest_format = "2.0"
project_hash = "e4c7d6f9b2a1c3d5e7f9a1b3c5d7e9f1a3b5c7d9"

[[deps.Dates]]
deps = ["Printf"]
uuid = "ade2ca70-3891-5945-98fb-dc099432e06a"

[[deps.JSON]]
deps = ["Dates", "Mmap", "Parsers", "Unicode"]
git-tree-sha1 = "31e996f0a15c7b280ba9f76636b3ff9e2ae58c9a"
uuid = "682c06a0-de6a-54ab-a142-c8b1cf79cde6"
version = "0.21.4"

[[deps.Private]]
git-tree-sha1 = "54c7a512469a38312a058ec9f429e1db1f074474"
repo-rev = "main"
repo-url = "https://ci:hunter2@git.corp.example/julia/Private.jl"
uuid = "d9f0e8c1-0000-4000-8000-000000000001"
version = "0.3.0"

[[deps.Dev]]
path = "C:\\Users\\alice\\.julia\\dev\\Dev"
uuid = "d9f0e8c1-0000-4000-8000-000000000002"
version = "0.1.0"

[[deps.Twin]]
git-tree-sha1 = "46e44e869b4d90b96bd8ed1fdcf32244fddfb6cc"
uuid = "11111111-1111-4111-8111-111111111111"
version = "1.0.0"

    [deps.Twin.deps]
    Test = "8dfed614-e22c-5e08-85e1-65c5234f0b40"

[[deps.Twin]]
git-tree-sha1 = "8eb7b4d4ca487caade9ba3e85932e28ce6d6e1f8"
uuid = "22222222-2222-4222-8222-222222222222"
version = "2.0.0"
"#;

    #[test]
    fn manifest_v2_resolves_with_sources_and_skips_standard_libraries() {
        let items = Julia.parse_file(&file("Manifest.toml", MANIFEST)).unwrap();
        assert_eq!(
            items[0].detail,
            "Manifest-Format 2.0 · Julia 1.10.4 · 5 Pakete · 1 Standardbibliotheken nicht einzeln aufgeführt"
        );
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let json = find("JSON");
        assert_eq!((json.version.as_deref(), json.install_state), (Some("0.21.4"), Some(InstallState::Resolved)));
        assert_eq!(
            (json.source, json.detail.as_str()),
            (SourceClass::Unknown, "aufgelöst laut Manifest.toml · Registry nicht vermerkt")
        );
        assert_eq!(json.evidence.dependencies.len(), 4);
        let private = find("Private");
        assert_eq!((private.source, private.source_host.as_deref()), (SourceClass::Git, Some("git.corp.example")));
        assert!(private.detail.ends_with("Git-Ref main"));
        assert_eq!(find("Dev").source, SourceClass::Path);
        let twins: Vec<_> = items.iter().filter(|i| i.name == "Twin").collect();
        assert_eq!(twins.len(), 2);
        assert_ne!(twins[0].key(), twins[1].key(), "same name, different UUIDs");
        assert_eq!(twins[0].evidence.dependencies[0].name, "Test");
        assert!(!items.iter().any(|i| i.name == "Dates"));
        let text = format!("{items:?}");
        for secret in ["hunter2", "julia/Private", "alice", "31e996f0", "project_hash"] {
            assert!(!text.contains(secret), "{secret} must not be stored");
        }
    }

    #[test]
    fn manifest_v21_registries_and_v1_format() {
        let v21 = r#"julia_version = "1.13.0"
manifest_format = "2.1"

[registries.General]
uuid = "23338594-aafe-5451-b93e-139f81909106"
url = "https://github.com/JuliaRegistries/General.git"

[registries.CorpRegistry]
uuid = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
url = "https://token@git.corp.example/julia/Registry.git"

[[deps.Example]]
git-tree-sha1 = "8eb7b4d4ca487caade9ba3e85932e28ce6d6e1f8"
registries = "General"
uuid = "7876af07-990d-54b4-ab0e-23690620f79a"
version = "1.2.3"

[[deps.Internal]]
git-tree-sha1 = "54c7a512469a38312a058ec9f429e1db1f074474"
registries = ["CorpRegistry"]
uuid = "d9f0e8c1-0000-4000-8000-000000000003"
version = "0.9.0"
"#;
        let items = Julia.parse_file(&file("JuliaManifest-v1.13.toml", v21)).unwrap();
        let example = items.iter().find(|i| i.name == "Example").unwrap();
        assert_eq!((example.source, example.source_host.as_deref()), (SourceClass::PublicRegistry, Some("github.com")));
        let internal = items.iter().find(|i| i.name == "Internal").unwrap();
        assert_eq!(
            (internal.source, internal.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("git.corp.example"))
        );
        assert!(!format!("{items:?}").contains("token"));

        let v1 = "# This file is machine-generated - editing it directly is not advised\n\n[[Base64]]\nuuid = \"2a0f44e3-6c83-55bd-87e4-b1978d98bd5f\"\n\n[[Example]]\ndeps = [\"Test\"]\ngit-tree-sha1 = \"46e44e869b4d90b96bd8ed1fdcf32244fddfb6cc\"\nuuid = \"7876af07-990d-54b4-ab0e-23690620f79a\"\nversion = \"0.5.3\"\n\n[[Test]]\ndeps = [\"InteractiveUtils\", \"Logging\"]\nuuid = \"8dfed614-e22c-5e08-85e1-65c5234f0b40\"\n";
        let items = Julia.parse_file(&file("Manifest.toml", v1)).unwrap();
        assert_eq!(items[0].detail, "Manifest-Format 1.0 · 1 Pakete · 2 Standardbibliotheken nicht einzeln aufgeführt");
        assert_eq!(items[1].name, "Example");
        assert_eq!(items[1].version.as_deref(), Some("0.5.3"));
    }

    #[test]
    fn gleam_manifest_and_broken_julia_manifest() {
        let gleam = "# This file was generated by Gleam\n# You typically do not need to edit this file\n\npackages = [\n  { name = \"gleam_stdlib\", version = \"0.34.0\", build_tools = [\"gleam\"], requirements = [], otp_app = \"gleam_stdlib\", source = \"hex\", outer_checksum = \"1FB8454D21CB1E3F3D7B5BFEE0E2AE2A\" },\n]\n\n[requirements]\ngleam_stdlib = { version = \"~> 0.34\" }\n";
        assert!(Julia.parse_file(&file("manifest.toml", gleam)).unwrap().is_empty());
        assert!(Julia.parse_file(&file("manifest.toml", "packages = [\n")).unwrap().is_empty());
        assert!(Julia.parse_file(&file("Manifest.toml", "julia_version = \"1.10.4\"\n[[deps.JSON]\n")).is_err());
        assert!(Julia.parse_file(&file("JuliaManifest.toml", "[[x")).is_err());
    }
}
