//! Go modules: go.mod (declared, direct and indirect), go.sum (versions
//! with a checksum of the module content) and vendor/modules.txt
//! (installed). Nothing is downloaded or built. GOPROXY, GOPRIVATE and
//! GOFLAGS are not read, so sources stay unknown except local `replace`
//! targets.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::limits::PACKAGE_FILE_MAX_BYTES;
use std::collections::BTreeSet;

pub struct Go;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "go",
    name: "Go-Module",
    markers: &["go.mod", "go.sum"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["go.mod (require, replace)", "go.sum", "vendor/modules.txt"],
    limits: "Kein go-Aufruf; Modulcache außerhalb des Projekts und go.work nicht gelesen; go.sum kann ältere, nicht mehr genutzte Versionen enthalten.",
};

impl Adapter for Go {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name {
            "go.mod" => bounded(go_mod(file)),
            _ => bounded(go_sum(file)),
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], children: &[String]) -> Option<DirClaim> {
        let vendored = siblings.iter().any(|s| s == "go.mod") && children.iter().any(|c| c == "modules.txt");
        (name == "vendor" && vendored).then_some(DirClaim {
            adapter: "go",
            category: ItemCategory::Environment,
            label: "vendor (Go)",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, "go", "vendor", rel_path)
                .detail("Mitgelieferte Module laut modules.txt; nicht übersetzt"),
        ];
        let text = access.read_text(&["modules.txt"], PACKAGE_FILE_MAX_BYTES).unwrap_or_default();
        for line in text.lines().take(200_000) {
            // `# path version` or `# path version => replacement`.
            let Some(rest) = line.strip_prefix("# ") else { continue };
            let mut parts = rest.split_whitespace();
            let (Some(path), Some(version)) = (parts.next(), parts.next()) else { continue };
            if version == "=>" {
                continue;
            }
            items.push(
                InventoryItem::new(ItemCategory::Package, "go", path, format!("{rel_path}/modules.txt"))
                    .version(version)
                    .state(InstallState::Installed)
                    .detail("im vendor-Ordner mitgeliefert"),
            );
        }
        bounded(items)
    }
}

fn go_mod(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let (mut module, mut go) = ("?".to_string(), "?".to_string());
    let mut requires: Vec<(String, String, bool)> = Vec::new();
    let mut replaced: Vec<(String, String)> = Vec::new();
    let mut block: Option<String> = None;
    for raw in file.content.lines().take(200_000) {
        let (code, comment) = raw.split_once("//").unwrap_or((raw, ""));
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        let (verb, rest) = match &block {
            Some(_) if code == ")" => {
                block = None;
                continue;
            }
            Some(verb) => (verb.clone(), code),
            None => {
                let (verb, rest) = code.split_once(char::is_whitespace).unwrap_or((code, ""));
                if rest.trim() == "(" {
                    block = Some(verb.to_string());
                    continue;
                }
                (verb.to_string(), rest.trim())
            }
        };
        let mut words = rest.split_whitespace().map(|w| w.trim_matches('"'));
        match verb.as_str() {
            "module" => module = words.next().unwrap_or("?").to_string(),
            "go" => go = words.next().unwrap_or("?").to_string(),
            "require" => {
                if let (Some(path), Some(version)) = (words.next(), words.next()) {
                    requires.push((path.to_string(), version.to_string(), comment.contains("indirect")));
                }
            }
            "replace" => {
                if let Some((old, new)) = rest.split_once("=>") {
                    let first = |side: &str| side.split_whitespace().next().unwrap_or("").trim_matches('"').to_string();
                    replaced.push((first(old), first(new)));
                }
            }
            _ => {}
        }
    }
    let mut items = vec![manifest("go", file, format!("Modul {module} · Go {go}"))];
    items[0].evidence.dependencies = requires
        .iter()
        .filter(|(.., indirect)| !indirect)
        .map(|(path, version, _)| PackageDependency {
            name: path.clone(),
            requirement: version.clone(),
            kind: "require".into(),
            exact: false,
        })
        .collect();
    for (path, version, indirect) in requires {
        let replacement = replaced.iter().find(|(old, _)| *old == path).map(|(_, new)| new.as_str());
        let local = replacement.is_some_and(local_path);
        let mut detail = format!("Mindestversion {version}");
        if indirect {
            detail.push_str(" · indirekt");
        }
        match replacement {
            Some(_) if local => detail.push_str(" · ersetzt durch lokalen Ordner"),
            Some(new) => detail.push_str(&format!(" · ersetzt durch {new}")),
            None => {}
        }
        let source = if local { SourceClass::Path } else { SourceClass::Unknown };
        let mut item = InventoryItem::new(ItemCategory::Package, "go", path, file.rel_path())
            .version(version)
            .state(InstallState::Declared)
            .source(source, None)
            .detail(detail);
        item.evidence.context = if indirect { "indirect" } else { "direct" }.into();
        items.push(item);
    }
    items
}

fn go_sum(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut items = vec![manifest("go", file, "Prüfsummen der Module")];
    let mut seen = BTreeSet::new();
    for line in file.content.lines().take(500_000) {
        let mut parts = line.split_whitespace();
        let (Some(path), Some(version)) = (parts.next(), parts.next()) else { continue };
        // `/go.mod` lines only cover the module's go.mod, not its code.
        if version.ends_with("/go.mod") || !seen.insert((path, version)) {
            continue;
        }
        items.push(
            InventoryItem::new(ItemCategory::Package, "go", path, file.rel_path())
                .version(version)
                .state(InstallState::Resolved)
                .detail("Prüfsumme des Modulinhalts in go.sum"),
        );
    }
    items
}

/// A `replace` target on disk: `./x`, `../x` or an absolute path.
fn local_path(target: &str) -> bool {
    let bytes = target.as_bytes();
    target.starts_with("./")
        || target.starts_with("../")
        || target.starts_with(".\\")
        || target.starts_with("..\\")
        || target.starts_with('/')
        || (bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const GO_MOD: &str = "module example.com/app\n\ngo 1.22\n\ntoolchain go1.22.4\n\nrequire (\n\tgithub.com/foo/bar v1.2.3\n\tgolang.org/x/sys v0.15.0 // indirect\n\tgithub.com/remyoudompheng/bigfft v0.0.0-20230129092748-24d4a6f8daec // indirect\n)\n\nrequire github.com/single/mod v0.1.0\n\nreplace github.com/foo/bar => ../bar\nreplace github.com/single/mod v0.1.0 => github.com/fork/mod v0.1.1\n";

    #[test]
    fn go_mod_requires_with_indirect_and_replacements() {
        let items = Go.parse_file(&file("go.mod", GO_MOD)).unwrap();
        assert_eq!(items[0].detail, "Modul example.com/app · Go 1.22");
        assert_eq!(items[0].evidence.dependencies.len(), 2, "only direct requirements are edges");
        let bar = items.iter().find(|i| i.name == "github.com/foo/bar").unwrap();
        assert_eq!((bar.version.as_deref(), bar.source), (Some("v1.2.3"), SourceClass::Path));
        assert_eq!(bar.evidence.context, "direct");
        let sys = items.iter().find(|i| i.name == "golang.org/x/sys").unwrap();
        assert!(sys.detail.contains("indirekt") && sys.source == SourceClass::Unknown);
        let pseudo = items.iter().find(|i| i.name == "github.com/remyoudompheng/bigfft").unwrap();
        assert_eq!(pseudo.version.as_deref(), Some("v0.0.0-20230129092748-24d4a6f8daec"));
        let single = items.iter().find(|i| i.name == "github.com/single/mod").unwrap();
        assert!(single.detail.ends_with("ersetzt durch github.com/fork/mod"), "{}", single.detail);
        assert!(items.iter().all(|i| i.install_state != Some(InstallState::Resolved)));
    }

    #[test]
    fn go_sum_keeps_module_content_once_and_vendor_is_installed() {
        let sum = "github.com/foo/bar v1.2.3 h1:abc=\ngithub.com/foo/bar v1.2.3/go.mod h1:def=\ngithub.com/old/dep v0.9.0/go.mod h1:ghi=\n";
        let items = Go.parse_file(&file("go.sum", sum)).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].install_state, Some(InstallState::Resolved));
        assert_eq!(items[1].version.as_deref(), Some("v1.2.3"));

        let mut dir = FakeDir::default();
        dir.files.insert(
            "modules.txt".into(),
            "# github.com/foo/bar v1.2.3\n## explicit; go 1.20\ngithub.com/foo/bar\n# example.com/local => ../local\n"
                .into(),
        );
        assert!(Go.claim_dir("vendor", &["main.go".into()], &["modules.txt".into()]).is_none());
        let claim = Go.claim_dir("vendor", &["go.mod".into()], &["modules.txt".into()]).unwrap();
        let items = Go.inspect_dir(claim, "vendor", &mut dir).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].install_state, Some(InstallState::Installed));
    }
}
