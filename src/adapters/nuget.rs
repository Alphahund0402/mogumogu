//! NuGet: SDK project files and Directory.Packages.props (declared, also
//! conditional and central), packages.config and packages.lock.json
//! (resolved), obj/project.assets.json (restored). No restore or build.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory};
use crate::limits::PACKAGE_FILE_MAX_BYTES;
use crate::parsers::{json, xml};

pub struct NuGet;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "nuget",
    name: "NuGet",
    markers: &["*.csproj", "*.fsproj", "*.vbproj", "Directory.Packages.props", "packages.config", "packages.lock.json"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    updates: Support::Unsupported,
    cleanup: Support::Unsupported,
    formats: &[
        "SDK-Projektdateien",
        "Directory.Packages.props",
        "packages.config",
        "packages.lock.json v1/v2",
        "obj/project.assets.json",
    ],
    limits: "Quellen aus NuGet.config werden nicht ausgewertet – Updatehinweise nicht unterstützt; globaler Paketordner wird nicht gelesen.",
};

const PROJECT_EXTENSIONS: &[&str] = &[".csproj", ".fsproj", ".vbproj"];

fn is_project_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PROJECT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

impl Adapter for NuGet {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name {
            "packages.lock.json" => bounded(lock_items(file)?),
            "packages.config" => bounded(packages_config(file)?),
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

fn project_items(file: &FoundFile<'_>) -> AdapterResult {
    let elements = xml::elements(file.content, 50_000, 32).map_err(|e| e.to_string())?;
    let frameworks: Vec<&str> = elements
        .iter()
        .filter(|e| e.name == "TargetFramework" || e.name == "TargetFrameworks")
        .map(|e| e.text.as_str())
        .collect();
    let central = file.name.eq_ignore_ascii_case("Directory.Packages.props");
    let mut detail = if central { "Zentrale Paketversionen".to_string() } else { "Projektdatei".to_string() };
    if !frameworks.is_empty() {
        detail.push_str(&format!(" · {}", frameworks.join(";")));
    }
    let mut items = vec![manifest("nuget", file, detail)];
    for (index, element) in elements.iter().enumerate() {
        if element.name != "PackageReference" && element.name != "PackageVersion" {
            continue;
        }
        let Some(name) = element.attr("Include").or_else(|| element.attr("Update")) else { continue };
        let version = element.attr("Version").map(str::to_string).or_else(|| {
            elements[index + 1..]
                .iter()
                .take_while(|e| e.depth > element.depth)
                .find(|e| e.name == "Version")
                .map(|e| e.text.clone())
        });
        let mut note = String::from("Spezifikation ");
        note.push_str(version.as_deref().unwrap_or("zentral/unbekannt"));
        if element.attr("Condition").is_some() {
            note.push_str(" · bedingt");
        }
        if central {
            note.push_str(" · zentral");
        }
        items.push(
            InventoryItem::new(ItemCategory::Package, "nuget", name, file.rel_path())
                .state(InstallState::Declared)
                .detail(note),
        );
    }
    Ok(items)
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
            items.push(
                InventoryItem::new(ItemCategory::Package, "nuget", name.as_str(), file.rel_path())
                    .version(entry["resolved"].as_str().unwrap_or_default())
                    .state(InstallState::Resolved)
                    .detail(format!("{} · {framework}", entry["type"].as_str().unwrap_or("?"))),
            );
        }
    }
    Ok(items)
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
}
