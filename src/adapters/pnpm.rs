//! pnpm: pnpm-lock.yaml (resolved), pnpm-workspace.yaml and the virtual
//! store listing `node_modules/.pnpm` (installed). The lockfile does not
//! name the registry, so sources stay unknown and are never queried.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory};
use crate::parsers::lines;

pub struct Pnpm;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "pnpm",
    name: "pnpm",
    markers: &["pnpm-lock.yaml", "pnpm-workspace.yaml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    updates: Support::Unsupported,
    cleanup: Support::Unsupported,
    formats: &["pnpm-lock.yaml v5–v9 (Schlüssel)", "pnpm-workspace.yaml", "node_modules/.pnpm"],
    limits: "Registryquelle nicht im Lockfile – Updatehinweise deshalb nicht unterstützt; Store außerhalb des Projekts wird nicht gelesen.",
};

impl Adapter for Pnpm {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name == "pnpm-workspace.yaml" {
            let patterns = lines::section_keys(file.content, "packages").len();
            return Ok(vec![manifest("pnpm", file, format!("Workspace-Definition · {patterns} Einträge"))]);
        }
        let version = lines::yaml_keys(file.content)
            .find(|l| l.indent == 0 && l.key == "lockfileVersion")
            .map(|l| l.value.to_string())
            .unwrap_or_else(|| "?".into());
        let mut items = vec![manifest("pnpm", file, format!("Lockfile v{version}"))];
        for line in lines::section_keys(file.content, "packages") {
            if let Some((name, version)) = split_package_key(line.key) {
                items.push(
                    InventoryItem::new(ItemCategory::Package, "pnpm", name, file.rel_path())
                        .version(version)
                        .state(InstallState::Resolved)
                        .detail("aufgelöst laut pnpm-lock.yaml"),
                );
            }
        }
        bounded(items)
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let pnpm = siblings.iter().any(|s| s == "pnpm-lock.yaml");
        (name == "node_modules" && pnpm).then_some(DirClaim {
            adapter: "pnpm",
            category: ItemCategory::Environment,
            label: "node_modules (pnpm)",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, "pnpm", "node_modules", rel_path)
                .detail("Verknüpfte Abhängigkeiten; Store-Inhalt außerhalb nicht gelesen"),
        ];
        for (entry, is_dir) in access.list(&[".pnpm"]) {
            if !is_dir || entry == "node_modules" || entry.starts_with('.') {
                continue;
            }
            // Store folder names encode "@scope+name@version(peer…)".
            let decoded = entry.replacen('+', "/", usize::from(entry.starts_with('@')));
            if let Some((name, version)) = split_package_key(&decoded) {
                items.push(
                    InventoryItem::new(ItemCategory::Package, "pnpm", name, format!("{rel_path}/.pnpm"))
                        .version(version)
                        .state(InstallState::Installed)
                        .detail("im virtuellen Store vorhanden"),
                );
            }
        }
        bounded(items)
    }
}

/// `/left-pad@1.3.0`, `'@scope/x@2.0.0(react@18)'`, `/left-pad/1.3.0` (v5).
pub(super) fn split_package_key(key: &str) -> Option<(String, String)> {
    let key = key.trim_start_matches('/');
    let key = key.split('(').next().unwrap_or(key);
    // v5/v6 path style "name/version_peers": the last segment is a version.
    let base = key.split('_').next().unwrap_or(key);
    let segments: Vec<&str> = base.split('/').collect();
    if segments.len() >= 2 && segments.last().is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit())) {
        let (version, name) = segments.split_last()?;
        return Some((name.join("/"), (*version).to_string()));
    }
    let index = key.rfind('@').filter(|&i| i > 0)?;
    let (name, version) = (&key[..index], &key[index + 1..]);
    (!name.is_empty() && !version.is_empty()).then(|| (name.to_string(), version.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    #[test]
    fn lock_keys_of_all_versions() {
        assert_eq!(split_package_key("/left-pad@1.3.0"), Some(("left-pad".into(), "1.3.0".into())));
        assert_eq!(split_package_key("@scope/x@2.0.0(react@18.2.0)"), Some(("@scope/x".into(), "2.0.0".into())));
        assert_eq!(split_package_key("/left-pad/1.3.0"), Some(("left-pad".into(), "1.3.0".into())));
        assert_eq!(split_package_key("/foo/1.0.0_react@17.0.0"), Some(("foo".into(), "1.0.0".into())));
        assert_eq!(split_package_key("is_js@1.0.0"), Some(("is_js".into(), "1.0.0".into())));
        assert_eq!(split_package_key("garbage"), None);
    }

    #[test]
    fn lockfile_and_store() {
        let lock = "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      left-pad:\n        specifier: ^1.3.0\n        version: 1.3.0\npackages:\n  left-pad@1.3.0:\n    resolution: {integrity: sha512-x}\n";
        let items = Pnpm.parse_file(&file("pnpm-lock.yaml", lock)).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].detail, "Lockfile v9.0");
        let mut dir = FakeDir::default();
        dir.dirs.insert(".pnpm".into(), vec![("@scope+x@2.0.0".into(), true), ("left-pad@1.3.0".into(), true)]);
        let claim = Pnpm.claim_dir("node_modules", &["pnpm-lock.yaml".into()], &[]).unwrap();
        let items = Pnpm.inspect_dir(claim, "node_modules", &mut dir).unwrap();
        assert!(items.iter().any(|i| i.name == "@scope/x" && i.version.as_deref() == Some("2.0.0")));
    }
}
