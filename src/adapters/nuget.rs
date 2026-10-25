//! NuGet: SDK project files, Directory.Build.props/.targets and
//! Directory.Packages.props (declared: also conditional, central and
//! global; project references as edges), packages.config and
//! packages.lock.json (resolved), obj/project.assets.json (restored), the
//! local tool manifest `.config/dotnet-tools.json`, solutions (project
//! counts) and nuget.config (source hosts only). MSBuild is not evaluated:
//! imports, targets and conditions stay unknown and `$(…)` versions resolve
//! only from the same file. Credentials, API keys and proxy settings in
//! nuget.config are never read. No restore or build.
use super::maven::Hosts;
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency};
use crate::limits::PACKAGE_FILE_MAX_BYTES;
use crate::parsers::{json, xml};
use std::collections::{BTreeMap, BTreeSet};

pub struct NuGet;

pub(super) const PUBLIC: &[&str] = &["api.nuget.org", "www.nuget.org", "nuget.org"];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "nuget",
    name: "NuGet",
    markers: &[
        "*.csproj",
        "*.fsproj",
        "*.vbproj",
        "Directory.Packages.props",
        "Directory.Build.props",
        "Directory.Build.targets",
        "packages.config",
        "packages.lock.json",
        "dotnet-tools.json",
        "*.sln",
        "*.slnx",
        "nuget.config",
    ],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "SDK-Projektdateien (PackageReference, ProjectReference, FrameworkReference, MSBuild-SDKs)",
        "Directory.Build.props/.targets",
        "Directory.Packages.props (zentral, global)",
        "packages.config",
        "packages.lock.json v1/v2",
        "obj/project.assets.json",
        ".config/dotnet-tools.json",
        "*.sln, *.slnx (Projektanzahl)",
        "nuget.config (Quellen-Hosts)",
    ],
    limits: "Kein MSBuild-/dotnet-Aufruf; Importe, Bedingungen und Ziele werden nicht ausgewertet – $(…)-Versionen nur aus derselben Datei; nuget.config nur Quellen-Hosts, Zugangsdaten und API-Schlüssel werden nie gelesen; welche Quelle ein Paket liefert, bleibt unbekannt.",
};

const PROJECT_EXTENSIONS: &[&str] = &[".csproj", ".fsproj", ".vbproj"];
/// Project type GUID of a solution folder in a `.sln`.
const SOLUTION_FOLDER: &str = "{2150E333-8FDC-42A3-9474-1A3956D46DE8}";

fn is_project_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PROJECT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

impl Adapter for NuGet {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let lower = file.name.to_ascii_lowercase();
        match lower.as_str() {
            "packages.lock.json" => bounded(lock_items(file)?),
            "packages.config" => bounded(packages_config(file)?),
            "dotnet-tools.json" => bounded(tool_manifest(file)?),
            "nuget.config" => bounded(nuget_config(file)?),
            _ if lower.ends_with(".sln") => bounded(solution(file)),
            _ if lower.ends_with(".slnx") => bounded(solution_xml(file)?),
            _ => bounded(project_items(file)?),
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let project = siblings.iter().any(|s| is_project_file(s));
        match name {
            "bin" if project => {
                Some(DirClaim { adapter: "nuget", category: ItemCategory::Output, label: ".NET-Buildausgabe" })
            }
            "obj" if project => {
                Some(DirClaim { adapter: "nuget", category: ItemCategory::Output, label: ".NET-Zwischenausgabe" })
            }
            _ => None,
        }
    }

    fn inspect_dir(&self, claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = vec![
            InventoryItem::new(ItemCategory::Output, "nuget", claim.label, rel_path)
                .detail("Generierte Ausgabe; Größe erst nach Messung bekannt"),
        ];
        if claim.label == ".NET-Zwischenausgabe"
            && let Some(text) = access.read_text(&["project.assets.json"], PACKAGE_FILE_MAX_BYTES)
        {
            let value = json::parse(&text, 64).map_err(|e| e.to_string())?;
            for (key, library) in value["libraries"].as_object().into_iter().flatten() {
                if library["type"].as_str() != Some("package") {
                    continue;
                }
                let Some((name, version)) = key.split_once('/') else { continue };
                items.push(
                    InventoryItem::new(ItemCategory::Package, "nuget", name, format!("{rel_path}/project.assets.json"))
                        .version(version)
                        .state(InstallState::Installed)
                        .detail("wiederhergestellt laut project.assets.json"),
                );
            }
        }
        bounded(items)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MsBuild {
    Project,
    Central,
    Build,
}

fn project_items(file: &FoundFile<'_>) -> AdapterResult {
    let elements = xml::elements(file.content, 50_000, 32).map_err(|e| e.to_string())?;
    let parents = super::maven::parents(&elements);
    let kind = match file.name.to_ascii_lowercase().as_str() {
        "directory.packages.props" => MsBuild::Central,
        "directory.build.props" | "directory.build.targets" => MsBuild::Build,
        _ => MsBuild::Project,
    };
    // `$(Name)` from this file only; MSBuild property names ignore case.
    let mut properties: BTreeMap<String, &str> = BTreeMap::new();
    for element in elements.iter().filter(|e| e.parent == "PropertyGroup" && !e.text.is_empty()) {
        properties.entry(element.name.to_ascii_lowercase()).or_insert(element.text.as_str());
    }
    let resolve = |raw: &str| -> Option<String> {
        if !raw.contains("$(") {
            return Some(raw.to_string());
        }
        let name = raw.strip_prefix("$(").and_then(|v| v.strip_suffix(')'))?;
        let value = properties.get(&name.to_ascii_lowercase())?;
        (!value.contains("$(")).then(|| value.to_string())
    };
    // Names for the manifest detail, a few of each.
    let (mut frameworks, mut sdks, mut framework_refs): (Vec<&str>, Vec<&str>, Vec<&str>) = Default::default();
    let mut references: Vec<PackageDependency> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let name = element.name.as_str();
        match name {
            "TargetFramework" | "TargetFrameworks" if !element.text.is_empty() => {
                remember(&mut frameworks, &element.text)
            }
            "Project" if element.depth == 0 => {
                for spec in element.attr("Sdk").unwrap_or_default().split(';').map(str::trim).take(8) {
                    let (sdk, version) = spec.split_once('/').map_or((spec, None), |(n, v)| (n, Some(v)));
                    if sdk.is_empty() {
                        continue;
                    }
                    remember(&mut sdks, sdk);
                    items.extend(version.map(|version| msbuild_sdk(file, sdk, version)));
                }
            }
            "Sdk" => {
                if let Some(sdk) = element.attr("Name") {
                    remember(&mut sdks, sdk);
                    items.extend(element.attr("Version").map(|version| msbuild_sdk(file, sdk, version)));
                }
            }
            "FrameworkReference" => {
                if let Some(framework) = element.attr("Include") {
                    remember(&mut framework_refs, framework);
                }
            }
            "ProjectReference" => {
                let path = element.attr("Include").unwrap_or_default();
                let target = path.rsplit(['/', '\\']).next().unwrap_or_default().trim();
                if !target.is_empty() && !target.contains(['*', '$']) && seen.insert(target) {
                    references.push(PackageDependency {
                        name: target.to_string(),
                        kind: "ProjectReference".into(),
                        ..PackageDependency::default()
                    });
                }
            }
            "PackageReference"
            | "PackageVersion"
            | "GlobalPackageReference"
            | "PackageDownload"
            | "DotNetCliToolReference" => {
                let update = element.attr("Include").is_none();
                let Some(package) = element.attr("Include").or_else(|| element.attr("Update")) else { continue };
                let raw = element.attr("VersionOverride").or_else(|| element.attr("Version")).or_else(|| {
                    elements[index + 1..]
                        .iter()
                        .take_while(|e| e.depth > element.depth)
                        .find(|e| e.name == "Version")
                        .map(|e| e.text.as_str())
                });
                let mut note = match raw.map(|raw| (raw, resolve(raw))) {
                    Some((_, Some(version))) => format!("Spezifikation {version}"),
                    Some((raw, None)) => format!("Version aus Variable {raw}"),
                    None => "Spezifikation zentral/unbekannt".into(),
                };
                let conditional = element.attr("Condition").is_some()
                    || parents[index].is_some_and(|parent| elements[parent].attr("Condition").is_some());
                let (context, label) = match (name, kind) {
                    ("PackageVersion", _) => ("", " · zentral"),
                    ("GlobalPackageReference", MsBuild::Build) => ("Directory.Build", " · global für alle Projekte"),
                    ("GlobalPackageReference", _) => ("global", " · global für alle Projekte"),
                    ("PackageDownload", _) => ("download", " · nur heruntergeladen"),
                    ("DotNetCliToolReference", _) => ("dotnet-cli-tool", " · CLI-Tool (veraltet)"),
                    (_, MsBuild::Build) => ("Directory.Build", ""),
                    _ => ("", ""),
                };
                if element.attr("VersionOverride").is_some() {
                    note.push_str(" · VersionOverride");
                }
                if conditional {
                    note.push_str(" · bedingt");
                }
                note.push_str(label);
                if update {
                    note.push_str(" · Update");
                }
                let mut qualifier = Vec::new();
                if update {
                    qualifier.push("update".to_string());
                }
                if name == "GlobalPackageReference" && kind == MsBuild::Build {
                    qualifier.push("global".into());
                }
                if conditional {
                    qualifier.push(raw.unwrap_or("bedingt").to_string());
                }
                let mut item = InventoryItem::new(ItemCategory::Package, "nuget", package, file.rel_path())
                    .state(InstallState::Declared)
                    .detail(note);
                item.evidence.context = context.into();
                item.evidence.qualifier = qualifier.join("|");
                items.push(item);
            }
            _ => {}
        }
    }
    let mut detail = match kind {
        MsBuild::Central => "Zentrale Paketversionen".to_string(),
        MsBuild::Build => format!("Gemeinsame Build-Einstellungen ({})", file.name),
        MsBuild::Project => "Projektdatei".to_string(),
    };
    if !frameworks.is_empty() {
        detail.push_str(&format!(" · {}", frameworks.join(";")));
    }
    if !sdks.is_empty() {
        detail.push_str(&format!(" · SDK {}", sdks.join(", ")));
    }
    if !framework_refs.is_empty() {
        detail.push_str(&format!(" · Framework {}", framework_refs.join(", ")));
    }
    if !references.is_empty() {
        detail.push_str(&format!(" · {} Projektverweise", references.len()));
    }
    let mut manifest = manifest("nuget", file, detail);
    manifest.evidence.dependencies = references;
    items.insert(0, manifest);
    Ok(items)
}

/// Keeps a few distinct names for a detail.
fn remember<'a>(list: &mut Vec<&'a str>, value: &'a str) {
    if list.len() < 8 && !list.contains(&value) {
        list.push(value);
    }
}

/// An MSBuild project SDK with a version comes from a NuGet feed.
fn msbuild_sdk(file: &FoundFile<'_>, name: &str, version: &str) -> InventoryItem {
    let mut item = InventoryItem::new(ItemCategory::Package, "nuget", name, file.rel_path())
        .state(InstallState::Declared)
        .detail(format!("Spezifikation {version} · MSBuild-SDK"));
    item.evidence.context = "msbuild-sdk".into();
    item
}

fn packages_config(file: &FoundFile<'_>) -> AdapterResult {
    let elements = xml::elements(file.content, 50_000, 8).map_err(|e| e.to_string())?;
    let mut items = vec![manifest("nuget", file, "packages.config")];
    for element in elements.iter().filter(|e| e.name == "package") {
        let Some(id) = element.attr("id") else { continue };
        let framework = element.attr("targetFramework").unwrap_or("–");
        items.push(
            InventoryItem::new(ItemCategory::Package, "nuget", id, file.rel_path())
                .version(element.attr("version").unwrap_or_default())
                .state(InstallState::Resolved)
                .detail(format!("festgelegt · {framework}")),
        );
    }
    Ok(items)
}

fn lock_items(file: &FoundFile<'_>) -> AdapterResult {
    let value = json::parse(file.content, 32).map_err(|e| e.to_string())?;
    let mut items = vec![manifest("nuget", file, format!("Lockfile v{}", value["version"].as_i64().unwrap_or(0)))];
    for (framework, packages) in value["dependencies"].as_object().into_iter().flatten() {
        for (name, entry) in packages.as_object().into_iter().flatten() {
            if entry["type"].as_str() == Some("Project") {
                continue;
            }
            let mut item = InventoryItem::new(ItemCategory::Package, "nuget", name.as_str(), file.rel_path())
                .version(entry["resolved"].as_str().unwrap_or_default())
                .state(InstallState::Resolved)
                .detail(format!("{} · {framework}", entry["type"].as_str().unwrap_or("?")));
            item.evidence.context = framework.into();
            item.evidence.dependencies = entry["dependencies"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, spec)| PackageDependency {
                    name: name.clone(),
                    requirement: spec.as_str().unwrap_or("").into(),
                    kind: "lock".into(),
                    exact: false,
                })
                .collect();
            items.push(item);
        }
    }
    Ok(items)
}

/// `.config/dotnet-tools.json`: local .NET tools pinned to exact versions.
fn tool_manifest(file: &FoundFile<'_>) -> AdapterResult {
    if file.rel_dir != ".config" && !file.rel_dir.ends_with("/.config") {
        return Ok(Vec::new());
    }
    let value = json::parse_jsonc(file.content, 16).map_err(|e| e.to_string())?;
    let Some(tools) = value["tools"].as_object() else { return Ok(Vec::new()) };
    let mut detail =
        format!("Lokales Tool-Manifest v{} · {} Tools", value["version"].as_i64().unwrap_or(0), tools.len());
    if value["isRoot"].as_bool() == Some(true) {
        detail.push_str(" · Suchwurzel");
    }
    let mut items = vec![manifest("nuget", file, detail)];
    for (name, entry) in tools {
        let version = entry["version"].as_str().unwrap_or_default();
        let commands: Vec<String> = entry["commands"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str())
            .take(16)
            .map(str::to_string)
            .collect();
        let spec =
            if version.is_empty() { "Version unbekannt".to_string() } else { format!("Spezifikation {version}") };
        let mut item = InventoryItem::new(ItemCategory::Package, "nuget", name.as_str(), file.rel_path())
            .version(version)
            .state(InstallState::Declared)
            .detail(format!("{spec} · dotnet-Tool · Befehle {}", commands.join(", ")));
        item.evidence.context = "dotnet-tool".into();
        item.evidence.executables = commands;
        items.push(item);
    }
    Ok(items)
}

/// A classic solution: `Project("{type}") = "Name", "path", "{id}"` lines.
fn solution(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    if !file.content.lines().take(5).any(|l| l.contains("Microsoft Visual Studio Solution File")) {
        return Vec::new();
    }
    let (mut projects, mut folders) = (0, 0);
    for line in file.content.lines().take(200_000) {
        let Some(rest) = line.trim_start().strip_prefix("Project(\"") else { continue };
        if rest.split('"').next().is_some_and(|kind| kind.eq_ignore_ascii_case(SOLUTION_FOLDER)) {
            folders += 1;
        } else {
            projects += 1;
        }
    }
    let mut detail = format!("Projektmappe · {projects} Projekte");
    if folders > 0 {
        detail.push_str(&format!(" · {folders} Projektmappenordner"));
    }
    vec![manifest("nuget", file, detail)]
}

/// The XML solution format: `<Solution>` with `<Project Path>` entries.
fn solution_xml(file: &FoundFile<'_>) -> AdapterResult {
    let elements = xml::elements(file.content, 50_000, 32).map_err(|e| e.to_string())?;
    if !elements.first().is_some_and(|e| e.depth == 0 && e.name == "Solution") {
        return Ok(Vec::new());
    }
    let projects = elements.iter().filter(|e| e.name == "Project" && e.attr("Path").is_some()).count();
    let folders = elements.iter().filter(|e| e.name == "Folder").count();
    let mut detail = format!("Projektmappe (XML) · {projects} Projekte");
    if folders > 0 {
        detail.push_str(&format!(" · {folders} Projektmappenordner"));
    }
    Ok(vec![manifest("nuget", file, detail)])
}

/// Source hosts of a nuget.config. Only direct children of the source
/// sections are looked at: `<packageSourceCredentials>`, `<apikeys>` and
/// `<config>` (proxy credentials) are skipped entirely.
fn nuget_config(file: &FoundFile<'_>) -> AdapterResult {
    let elements = xml::elements(file.content, 50_000, 32).map_err(|e| e.to_string())?;
    if !elements.first().is_some_and(|e| e.depth == 0 && e.name.eq_ignore_ascii_case("configuration")) {
        return Ok(Vec::new());
    }
    let mut hosts = Hosts::default();
    let (mut sources, mut disabled, mut cleared, mut mapping) = (0, 0, false, false);
    for element in elements.iter().filter(|e| e.depth == 2) {
        let section = element.parent.to_ascii_lowercase();
        match (section.as_str(), element.name.as_str()) {
            ("packagesources", "add") => {
                if let Some(value) = element.attr("value") {
                    sources += 1;
                    hosts.add(value, PUBLIC);
                }
            }
            ("packagesources", "clear") => cleared = true,
            ("disabledpackagesources", "add") => {
                disabled += usize::from(element.attr("value").is_some_and(|v| v.eq_ignore_ascii_case("true")));
            }
            ("packagesourcemapping", "packageSource") => mapping = true,
            _ => {}
        }
    }
    let mut detail = format!("NuGet-Konfiguration · {sources} Paketquellen");
    if let Some(label) = hosts.label() {
        detail.push_str(&format!(" ({label})"));
    }
    if hosts.has_private() {
        detail.push_str(" · private Paketquelle konfiguriert");
    }
    if cleared {
        detail.push_str(" · geerbte Quellen entfernt");
    }
    if disabled > 0 {
        detail.push_str(&format!(" · {disabled} deaktiviert"));
    }
    if mapping {
        detail.push_str(" · Quellenzuordnung");
    }
    Ok(vec![manifest("nuget", file, detail)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    #[test]
    fn sdk_project_with_conditional_and_child_version() {
        let text = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup>
            <ItemGroup><PackageReference Include="Serilog" Version="3.1.1" />
            <PackageReference Include="Moq" Condition="'$(Test)'=='1'"><Version>4.20.0</Version></PackageReference></ItemGroup></Project>"#;
        let items = NuGet.parse_file(&file("App.csproj", text)).unwrap();
        assert!(items[0].detail.contains("net8.0"));
        let moq = items.iter().find(|i| i.name == "Moq").unwrap();
        assert!(moq.detail.contains("4.20.0") && moq.detail.contains("bedingt"));
    }

    #[test]
    fn lock_and_assets() {
        let lock = r#"{"version":1,"dependencies":{"net8.0":{"Serilog":{"type":"Direct","requested":"[3.1.1, )","resolved":"3.1.1"},"Lib":{"type":"Project"}}}}"#;
        let items = NuGet.parse_file(&file("packages.lock.json", lock)).unwrap();
        assert_eq!(items.len(), 2);
        let mut dir = FakeDir::default();
        dir.files.insert(
            "project.assets.json".into(),
            r#"{"libraries":{"Serilog/3.1.1":{"type":"package"},"Lib/1.0.0":{"type":"project"}}}"#.into(),
        );
        let claim = NuGet.claim_dir("obj", &["App.csproj".into()], &[]).unwrap();
        let items = NuGet.inspect_dir(claim, "obj", &mut dir).unwrap();
        assert_eq!(items.iter().filter(|i| i.install_state == Some(InstallState::Installed)).count(), 1);
    }

    #[test]
    fn project_references_frameworks_sdks_and_variables() {
        let text = r#"<Project Sdk="Microsoft.NET.Sdk.Web">
  <Sdk Name="Microsoft.Build.CentralPackageVersions" Version="2.1.3" />
  <PropertyGroup><TargetFrameworks>net8.0;net9.0</TargetFrameworks><SerilogVersion>4.1.0</SerilogVersion></PropertyGroup>
  <ItemGroup>
    <FrameworkReference Include="Microsoft.AspNetCore.App" />
    <ProjectReference Include="..\Core\Core.csproj" />
    <ProjectReference Include="../Shared/Shared.fsproj" />
    <ProjectReference Include="..\Core\Core.csproj" />
    <ProjectReference Include="..\**\*.csproj" />
  </ItemGroup>
  <ItemGroup Condition="'$(TargetFramework)' == 'net9.0'">
    <PackageReference Include="Serilog" Version="$(SerilogVersion)" />
    <PackageReference Include="Polly" Version="$(PollyVersion)" />
  </ItemGroup>
  <ItemGroup>
    <PackageReference Include="Serilog" Version="3.1.1" />
    <PackageReference Include="Humanizer" VersionOverride="2.14.1" />
    <PackageDownload Include="Microsoft.NETCore.App.Ref" Version="[6.0.0]" />
  </ItemGroup>
</Project>"#;
        let items = NuGet.parse_file(&file("Web.csproj", text)).unwrap();
        let manifest = &items[0];
        assert_eq!(
            manifest.detail,
            "Projektdatei · net8.0;net9.0 · SDK Microsoft.NET.Sdk.Web, Microsoft.Build.CentralPackageVersions · Framework Microsoft.AspNetCore.App · 2 Projektverweise"
        );
        let edges: Vec<(&str, &str)> =
            manifest.evidence.dependencies.iter().map(|d| (d.name.as_str(), d.kind.as_str())).collect();
        assert_eq!(edges, [("Core.csproj", "ProjectReference"), ("Shared.fsproj", "ProjectReference")]);

        let serilog: Vec<&InventoryItem> = items.iter().filter(|i| i.name == "Serilog").collect();
        assert_eq!(serilog.len(), 2, "conditional and plain references both stay");
        assert!(serilog.iter().any(|i| i.detail == "Spezifikation 4.1.0 · bedingt"));
        assert!(serilog.iter().any(|i| i.detail == "Spezifikation 3.1.1" && i.evidence.qualifier.is_empty()));
        let polly = items.iter().find(|i| i.name == "Polly").unwrap();
        assert!(polly.detail.starts_with("Version aus Variable $(PollyVersion)"), "{}", polly.detail);
        let humanizer = items.iter().find(|i| i.name == "Humanizer").unwrap();
        assert_eq!(humanizer.detail, "Spezifikation 2.14.1 · VersionOverride");
        let download = items.iter().find(|i| i.name == "Microsoft.NETCore.App.Ref").unwrap();
        assert_eq!(download.evidence.context, "download");
        let sdk = items.iter().find(|i| i.name == "Microsoft.Build.CentralPackageVersions").unwrap();
        assert_eq!((sdk.evidence.context.as_str(), sdk.install_state), ("msbuild-sdk", Some(InstallState::Declared)));
        assert!(!items.iter().any(|i| i.name == "Microsoft.NET.Sdk.Web"), "SDKs without version ship with .NET");
    }

    #[test]
    fn directory_build_and_global_package_references() {
        let props = r#"<Project>
  <PropertyGroup><AnalyzersVersion>9.0.0</AnalyzersVersion></PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Microsoft.CodeAnalysis.NetAnalyzers" Version="$(AnalyzersVersion)" PrivateAssets="all" />
    <GlobalPackageReference Include="Nerdbank.GitVersioning" Version="3.6.146" />
    <PackageReference Include="Nerdbank.GitVersioning" Version="3.6.146" />
    <ProjectReference Include="$(RepoRoot)src\Analyzers\Analyzers.csproj" />
  </ItemGroup>
</Project>"#;
        let items = NuGet.parse_file(&file("Directory.Build.props", props)).unwrap();
        assert_eq!(items[0].detail, "Gemeinsame Build-Einstellungen (Directory.Build.props) · 1 Projektverweise");
        assert_eq!(items[0].evidence.dependencies[0].name, "Analyzers.csproj");
        assert!(items.iter().skip(1).all(|i| i.evidence.context == "Directory.Build"));
        let analyzers = items.iter().find(|i| i.name == "Microsoft.CodeAnalysis.NetAnalyzers").unwrap();
        assert_eq!(analyzers.detail, "Spezifikation 9.0.0");
        let versioning: Vec<&str> = items
            .iter()
            .filter(|i| i.name == "Nerdbank.GitVersioning")
            .map(|i| i.evidence.qualifier.as_str())
            .collect();
        assert_eq!(versioning, ["global", ""]);

        let targets = r#"<Project><ItemGroup><PackageReference Update="Newtonsoft.Json" Version="13.0.3" /></ItemGroup></Project>"#;
        let items = NuGet.parse_file(&file("Directory.Build.targets", targets)).unwrap();
        let update = &items[1];
        assert_eq!(
            (update.evidence.context.as_str(), update.evidence.qualifier.as_str()),
            ("Directory.Build", "update")
        );
        assert!(update.detail.ends_with("· Update"), "{}", update.detail);

        let central = r#"<Project>
  <PropertyGroup><ManagePackageVersionsCentrally>true</ManagePackageVersionsCentrally></PropertyGroup>
  <ItemGroup>
    <PackageVersion Include="PackageA" Version="1.0.0" Condition="'$(TargetFramework)' == 'netstandard2.0'" />
    <PackageVersion Include="PackageA" Version="2.0.0" Condition="'$(TargetFramework)' == 'net8.0'" />
    <GlobalPackageReference Include="Nerdbank.GitVersioning" Version="3.5.109" />
  </ItemGroup>
</Project>"#;
        let items = NuGet.parse_file(&file("Directory.Packages.props", central)).unwrap();
        assert_eq!(items[0].detail, "Zentrale Paketversionen");
        let keys: std::collections::BTreeSet<String> = items.iter().map(InventoryItem::key).collect();
        assert_eq!(keys.len(), 4, "conditional central versions are kept apart");
        let global = items.iter().find(|i| i.name == "Nerdbank.GitVersioning").unwrap();
        assert_eq!(global.evidence.context, "global");
        assert!(global.detail.contains("3.5.109") && global.detail.contains("global"));
    }

    #[test]
    fn local_tool_manifest_only_below_dot_config() {
        let text = r#"{
  "version": 1,
  "isRoot": true,
  "tools": {
    "dotnet-ef": { "version": "8.0.11", "commands": ["dotnet-ef"], "rollForward": false },
    "csharpier": { "version": "0.30.6", "commands": ["dotnet-csharpier"] },
  }
}"#;
        let mut found = file("dotnet-tools.json", text);
        assert!(NuGet.parse_file(&found).unwrap().is_empty());
        found.rel_dir = ".config";
        let items = NuGet.parse_file(&found).unwrap();
        assert_eq!(items[0].detail, "Lokales Tool-Manifest v1 · 2 Tools · Suchwurzel");
        let ef = items.iter().find(|i| i.name == "dotnet-ef").unwrap();
        assert_eq!((ef.version.as_deref(), ef.evidence.context.as_str()), (Some("8.0.11"), "dotnet-tool"));
        assert_eq!(
            (ef.install_state, ef.evidence.executables.clone()),
            (Some(InstallState::Declared), vec!["dotnet-ef".into()])
        );
        found.rel_dir = "tools/.config";
        assert!(NuGet.parse_file(&FoundFile { content: r#"{"profiles":{}}"#, ..found }).unwrap().is_empty());
        assert!(NuGet.parse_file(&FoundFile { content: "{\"tools\": {", ..found }).is_err());
    }

    #[test]
    fn solutions_count_projects_without_folders() {
        let sln = "\u{feff}\r\nMicrosoft Visual Studio Solution File, Format Version 12.00\r\n# Visual Studio Version 17\r\nProject(\"{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}\") = \"App\", \"src\\App\\App.csproj\", \"{11111111-1111-1111-1111-111111111111}\"\r\nEndProject\r\nProject(\"{2150E333-8FDC-42A3-9474-1A3956D46DE8}\") = \"Solution Items\", \"Solution Items\", \"{22222222-2222-2222-2222-222222222222}\"\r\nEndProject\r\nProject(\"{9A19103F-16F7-4668-BE54-9A1E7A4F7556}\") = \"Tests\", \"tests\\Tests\\Tests.csproj\", \"{33333333-3333-3333-3333-333333333333}\"\r\nEndProject\r\nGlobal\r\nEndGlobal\r\n";
        let items = NuGet.parse_file(&file("App.sln", sln)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].detail, "Projektmappe · 2 Projekte · 1 Projektmappenordner");
        assert!(NuGet.parse_file(&file("notes.sln", "just text")).unwrap().is_empty());

        let slnx = r#"<Solution>
  <Folder Name="/src/"><Project Path="src/App/App.csproj" /><Project Path="src/Lib/Lib.fsproj" Type="Classic F#" /></Folder>
  <Folder Name="/Solution Items/"><File Path="README.md" /></Folder>
  <Project Path="tests/Tests/Tests.csproj" />
</Solution>"#;
        let items = NuGet.parse_file(&file("App.slnx", slnx)).unwrap();
        assert_eq!(items[0].detail, "Projektmappe (XML) · 3 Projekte · 2 Projektmappenordner");
        assert!(NuGet.parse_file(&file("App.slnx", "<Solution><Project")).is_err());
        assert!(NuGet.parse_file(&file("App.slnx", "<Other/>")).unwrap().is_empty());
    }

    #[test]
    fn nuget_config_keeps_hosts_but_never_credentials() {
        let text = r#"<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <config>
    <add key="http_proxy" value="http://proxyuser:proxy-pass@proxy.corp.example:3128" />
    <add key="defaultPushSource" value="https://push.corp.example/api/v2/package" />
  </config>
  <packageSources>
    <clear />
    <add key="nuget.org" value="https://api.nuget.org/v3/index.json" protocolVersion="3" />
    <add key="Contoso" value="https://feeduser:feed-pass@pkgs.contoso.example/nuget/v3/index.json" />
    <add key="Local" value="..\packages" />
  </packageSources>
  <packageSourceCredentials>
    <Contoso>
      <add key="Username" value="user@contoso.example" />
      <add key="ClearTextPassword" value="hunter2-cleartext" />
    </Contoso>
    <packageSources><add key="Password" value="nested-secret" /></packageSources>
  </packageSourceCredentials>
  <apikeys><add key="https://push.corp.example/api/v2/package" value="encrypted-api-key" /></apikeys>
  <disabledPackageSources><add key="Local" value="true" /></disabledPackageSources>
  <packageSourceMapping>
    <packageSource key="Contoso"><package pattern="Contoso.*" /></packageSource>
  </packageSourceMapping>
</configuration>"#;
        let items = NuGet.parse_file(&file("NuGet.Config", text)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].detail,
            "NuGet-Konfiguration · 3 Paketquellen (1 öffentlich, 1 lokal, privat: pkgs.contoso.example) · private Paketquelle konfiguriert · geerbte Quellen entfernt · 1 deaktiviert · Quellenzuordnung"
        );
        let debug = format!("{items:?}");
        for secret in [
            "hunter2",
            "nested-secret",
            "encrypted-api-key",
            "feed-pass",
            "feeduser",
            "proxy",
            "user@contoso",
            "push.corp",
        ] {
            assert!(!debug.contains(secret), "{secret} leaked");
        }
        assert!(!items[0].detail.contains(".."), "local source paths are not kept");
        assert!(NuGet.parse_file(&file("nuget.config", "<configuration><packageSources></configuration>")).is_err());
        assert!(NuGet.parse_file(&file("nuget.config", "<settings/>")).unwrap().is_empty());
    }
}
