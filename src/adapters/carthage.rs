//! Carthage: Cartfile and Cartfile.private (declared; the private file is
//! for development only) and Cartfile.resolved (resolved), read line by
//! line, plus `Carthage/` beside them (installed: `Build/.Name.version`
//! files and checkout folder names). Nothing is fetched or built; the JSON
//! specifications behind binary framework URLs are not read.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::json;
use regex::Regex;
use std::sync::LazyLock;

pub struct Carthage;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "carthage",
    name: "Carthage",
    markers: &["Cartfile", "Cartfile.private", "Cartfile.resolved"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["Cartfile, Cartfile.private", "Cartfile.resolved", "Carthage/Build/.*.version, Carthage/Checkouts"],
    limits: "Kein carthage-Aufruf; Binär-Spezifikationen hinter URLs werden nicht abgerufen; ohne .version-Datei bleibt die installierte Version unbekannt.",
};

/// `github "owner/repo" ~> 1.0`, `git "url" "branch"`, `binary "url" == 1.0`.
static LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r##"^(github|git|binary)\s+"([^"]+)"(?:\s+(?:(>=|~>|==)\s*([^\s"#]+)|"([^"]*)"))?\s*(?:#.*)?$"##)
        .expect("valid regex")
});

impl Adapter for Carthage {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let resolved = file.name.eq_ignore_ascii_case("Cartfile.resolved");
        let private = file.name.eq_ignore_ascii_case("Cartfile.private");
        let mut items = Vec::new();
        for (number, line) in file.content.lines().enumerate().take(100_000) {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some(found) = LINE.captures(line) else {
                if resolved {
                    return Err(format!("Cartfile.resolved: Zeile {} nicht lesbar", number + 1));
                }
                continue;
            };
            let (origin, id) = (&found[1], &found[2]);
            let name = super::swiftpm::repo_name(id).map(|n| if origin == "binary" { strip_json(n) } else { n });
            let Some(name) = name.filter(|n| !n.is_empty()) else { continue };
            let (source, host) = match origin {
                "github" => (SourceClass::Git, None),
                "git" if super::swiftpm::is_local(id) => (SourceClass::Path, None),
                "git" => (SourceClass::Git, None),
                _ if id.contains("://") && !super::swiftpm::is_local(id) => classify_url(id, &[]),
                _ => (SourceClass::Path, None),
            };
            let quoted = found.get(5).map(|m| m.as_str());
            let (version, mut detail) = if resolved {
                let value = quoted.unwrap_or_default();
                match super::swiftpm::commit(value) {
                    Some(commit) => (commit, format!("aufgelöst laut Cartfile.resolved · Commit {commit}")),
                    None => (value, "aufgelöst laut Cartfile.resolved".to_string()),
                }
            } else {
                match (found.get(3).map(|m| m.as_str()), found.get(4).map(|m| m.as_str()), quoted) {
                    (Some("=="), Some(version), _) => (version, format!("Spezifikation == {version}")),
                    (Some(operator), Some(version), _) => ("", format!("Spezifikation {operator} {version}")),
                    (_, _, Some(reference)) => match super::swiftpm::commit(reference) {
                        Some(commit) => ("", format!("Commit {commit}")),
                        None => ("", format!("Git-Referenz {reference}")),
                    },
                    _ => ("", "ohne Versionsangabe".to_string()),
                }
            };
            if origin == "binary" {
                detail.push_str(" · Binärframework");
            }
            if private {
                detail.push_str(" · nur Entwicklung");
            }
            let state = if resolved { InstallState::Resolved } else { InstallState::Declared };
            let mut item = InventoryItem::new(ItemCategory::Package, "carthage", name, file.rel_path())
                .version(version)
                .state(state)
                .source(source, host)
                .detail(detail);
            if private {
                item.evidence.context = "dev".into();
            }
            items.push(item);
        }
        let kind = if resolved { "festgelegt" } else { "deklariert" };
        items.insert(0, manifest("carthage", file, format!("{} · {} Abhängigkeiten {kind}", file.name, items.len())));
        bounded(items)
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let cartfile = siblings.iter().any(|s| s == "Cartfile" || s == "Cartfile.private" || s == "Cartfile.resolved");
        (name == "Carthage" && cartfile).then_some(DirClaim {
            adapter: "carthage",
            category: ItemCategory::Environment,
            label: "Carthage",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        // `Build/.Name.version` (JSON) names the built commitish.
        let mut built: Vec<(String, Option<String>)> = Vec::new();
        for (entry, is_dir) in access.list(&["Build"]).into_iter().take(5_000) {
            let Some(name) = entry.strip_prefix('.').and_then(|e| e.strip_suffix(".version")) else { continue };
            if is_dir || name.is_empty() {
                continue;
            }
            let version = access.read_text(&["Build", &entry], 64 * 1024).and_then(|text| commitish(&text));
            built.push((name.to_string(), version));
        }
        let checkouts: Vec<String> = access
            .list(&["Checkouts"])
            .into_iter()
            .filter(|(name, is_dir)| *is_dir && !name.starts_with('.'))
            .map(|(name, _)| name)
            .collect();
        let mut items = vec![InventoryItem::new(ItemCategory::Environment, "carthage", "Carthage", rel_path).detail(
            format!("{} gebaute Abhängigkeiten, {} Checkouts; nicht ausgeführt", built.len(), checkouts.len()),
        )];
        for (name, version) in &built {
            let detail =
                if version.is_some() { "gebaut laut Build/.version-Datei" } else { "gebaut; Version unbekannt" };
            items.push(
                InventoryItem::new(ItemCategory::Package, "carthage", name, format!("{rel_path}/Build"))
                    .version(version.clone().unwrap_or_default())
                    .state(InstallState::Installed)
                    .detail(detail),
            );
        }
        for name in checkouts {
            if built.iter().any(|(built, _)| *built == name) {
                continue;
            }
            items.push(
                InventoryItem::new(ItemCategory::Package, "carthage", name, format!("{rel_path}/Checkouts"))
                    .state(InstallState::Installed)
                    .detail("Checkout vorhanden; nicht gebaut, Version unbekannt"),
            );
        }
        bounded(items)
    }
}

/// A binary framework is named after its specification file.
fn strip_json(name: &str) -> &str {
    name.strip_suffix(".json").unwrap_or(name)
}

/// The tag or commit a `Build/.Name.version` file was built from.
fn commitish(text: &str) -> Option<String> {
    let value = json::parse(text, 8).ok()?;
    let commitish = value["commitish"].as_str()?;
    Some(super::swiftpm::commit(commitish).unwrap_or(commitish).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const CARTFILE: &str = r#"# Require version 2.3.1 or later
github "ReactiveCocoa/ReactiveCocoa" >= 2.3.1

# Require version 1.x
github "Mantle/Mantle" ~> 1.0    # (1.0 or later, but less than 2.0)

# Require exactly version 0.4.1
github "jspahrsummers/libextobjc" == 0.4.1

# Use the latest version
github "jspahrsummers/xcconfigs"

# Use a project from GitHub Enterprise
github "https://enterprise.local/ghe/desktop/git-error-translations"

# Use a project from any arbitrary server, on the "development" branch
git "https://deploy:hunter2@enterprise.local/desktop/git-error-translations2.git" "development"

# Use a local project
git "file:///directory/to/project" "branch"

# A binary only framework
binary "https://my.domain.com/release/MyFramework.json" ~> 2.3

# Binary only framework via local file path
binary "file:///some/local/path/MyLocalFramework.json" ~> 2.3
"#;

    #[test]
    fn cartfile_origins_and_requirements() {
        let items = Carthage.parse_file(&file("Cartfile", CARTFILE)).unwrap();
        assert_eq!(items[0].detail, "Cartfile · 9 Abhängigkeiten deklariert");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let reactive = find("ReactiveCocoa");
        assert_eq!((reactive.source, reactive.detail.as_str()), (SourceClass::Git, "Spezifikation >= 2.3.1"));
        assert_eq!(find("Mantle").detail, "Spezifikation ~> 1.0");
        assert_eq!(find("libextobjc").version.as_deref(), Some("0.4.1"));
        assert_eq!(find("xcconfigs").detail, "ohne Versionsangabe");
        assert_eq!(find("git-error-translations").source, SourceClass::Git);
        let branch = find("git-error-translations2");
        assert_eq!((branch.source, branch.detail.as_str()), (SourceClass::Git, "Git-Referenz development"));
        assert_eq!(find("project").source, SourceClass::Path);
        let binary = find("MyFramework");
        assert_eq!(
            (binary.source, binary.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("my.domain.com"))
        );
        assert_eq!(binary.detail, "Spezifikation ~> 2.3 · Binärframework");
        assert_eq!(find("MyLocalFramework").source, SourceClass::Path);
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        assert!(!format!("{items:?}").contains("hunter2"));
    }

    #[test]
    fn private_cartfile_is_development_context() {
        let items = Carthage.parse_file(&file("Cartfile.private", "github \"Quick/Nimble\" ~> 13.0\n")).unwrap();
        assert_eq!(items[1].evidence.context, "dev");
        assert!(items[1].detail.ends_with("nur Entwicklung"));
    }

    #[test]
    fn resolved_versions_commits_and_broken_lines() {
        let resolved = "binary \"https://my.domain.com/release/MyFramework.json\" \"2.3.1\"\ngithub \"Mantle/Mantle\" \"1.5.4\"\ngithub \"ReactiveCocoa/ReactiveCocoa\" \"v2.3.1\"\ngit \"https://user:t0ken@enterprise.local/desktop/translations.git\" \"8ff4393ede2ca86d5a78edaf62b3a14d90bffab9\"\n";
        let items = Carthage.parse_file(&file("Cartfile.resolved", resolved)).unwrap();
        assert_eq!(items[0].detail, "Cartfile.resolved · 4 Abhängigkeiten festgelegt");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Resolved)));
        let version = |name: &str| items.iter().find(|i| i.name == name).unwrap().version.clone();
        assert_eq!(version("MyFramework").as_deref(), Some("2.3.1"));
        assert_eq!(version("ReactiveCocoa").as_deref(), Some("v2.3.1"));
        let translations = items.iter().find(|i| i.name == "translations").unwrap();
        assert_eq!(translations.version.as_deref(), Some("8ff4393ede2c"));
        assert!(translations.detail.ends_with("Commit 8ff4393ede2c"));
        assert!(!format!("{items:?}").contains("t0ken"));
        assert!(Carthage.parse_file(&file("Cartfile.resolved", "github \"a/b\" \"1.0\"\n<<<<<<< HEAD\n")).is_err());
        assert!(Carthage.parse_file(&file("Cartfile", "<<<<<<< HEAD\ngithub \"a/b\"\n")).unwrap().len() == 2);
    }

    #[test]
    fn carthage_folder_beside_a_cartfile() {
        assert!(Carthage.claim_dir("Carthage", &["Podfile".into()], &[]).is_none());
        let claim = Carthage.claim_dir("Carthage", &["Cartfile.resolved".into()], &[]).unwrap();
        let mut dir = FakeDir::default();
        dir.dirs.insert(
            "Build".into(),
            vec![
                (".Mantle.version".into(), false),
                (".MyFramework.version".into(), false),
                ("Mantle.xcframework".into(), true),
            ],
        );
        dir.dirs.insert("Checkouts".into(), vec![("Mantle".into(), true), ("Nimble".into(), true)]);
        dir.files.insert(
            "Build/.Mantle.version".into(),
            r#"{"commitish":"1.5.4","XCFrameworks":[{"name":"Mantle","hash":"abc","linking":"dynamic"}]}"#.into(),
        );
        dir.files.insert(
            "Build/.MyFramework.version".into(),
            r#"{"commitish":"8ff4393ede2ca86d5a78edaf62b3a14d90bffab9","iOS":[]}"#.into(),
        );
        let items = Carthage.inspect_dir(claim, "Carthage", &mut dir).unwrap();
        assert_eq!(items.len(), 4);
        let version = |name: &str| items.iter().find(|i| i.name == name).unwrap().version.clone();
        assert_eq!(version("Mantle").as_deref(), Some("1.5.4"));
        assert_eq!(version("MyFramework").as_deref(), Some("8ff4393ede2c"));
        assert_eq!(version("Nimble"), None);
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Installed)));
    }
}
