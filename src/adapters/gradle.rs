//! Gradle: version catalogs `libs.versions.toml` and string coordinates in
//! build scripts (declared), dependency lockfiles (resolved). Build scripts
//! are code: they are matched as text, never evaluated, so versions from
//! variables, BOMs or plugins stay unknown. The `build` folder beside a
//! build script is an output and is not walked.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory};
use crate::parsers;
use regex::Regex;
use std::sync::LazyLock;

pub struct Gradle;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "gradle",
    name: "Gradle",
    markers: &[
        "build.gradle",
        "build.gradle.kts",
        "libs.versions.toml",
        "gradle.lockfile",
        "buildscript-gradle.lockfile",
    ],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["libs.versions.toml", "build.gradle(.kts): Koordinaten als Text", "gradle.lockfile"],
    limits: "Kein Gradle-Aufruf; Buildskripte werden nicht ausgewertet – Versionen aus Variablen, BOMs oder Plugins bleiben unbekannt; Repositories nicht gelesen.",
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

impl Adapter for Gradle {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name {
            "libs.versions.toml" => bounded(catalog(file)?),
            "gradle.lockfile" | "buildscript-gradle.lockfile" => bounded(lockfile(file)),
            _ => bounded(script(file)),
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let script = siblings.iter().any(|s| s == "build.gradle" || s == "build.gradle.kts");
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

fn script(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    let mut catalog_refs = 0;
    for line in file.content.lines().take(200_000) {
        let code = line.trim_start();
        if code.starts_with("//") || code.starts_with('*') || code.starts_with("/*") {
            continue;
        }
        catalog_refs += line.matches("libs.").count();
        if let Some(plugin) = PLUGIN.captures(line) {
            let detail = format!("Gradle-Plugin · Spezifikation {}", &plugin[2]);
            items.push(declared(file, &plugin[1], Some(&plugin[2]), "plugin", detail));
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
    items.insert(0, manifest("gradle", file, detail));
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
}
