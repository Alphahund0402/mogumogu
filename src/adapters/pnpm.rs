//! pnpm: pnpm-lock.yaml (resolved), pnpm-workspace.yaml (package patterns,
//! catalogs as declared ranges, overrides as edges) and the virtual store
//! listing `node_modules/.pnpm` (installed). The lockfile does not name the
//! registry, so sources stay unknown and are never queried.
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
    cleanup: Support::Unsupported,
    formats: &[
        "pnpm-lock.yaml v5–v9 (Schlüssel)",
        "pnpm-workspace.yaml (packages, catalog, catalogs, overrides)",
        "node_modules/.pnpm",
    ],
    limits: "Registryquelle nicht im Lockfile – Updatehinweise deshalb nicht unterstützt; Store außerhalb des Projekts wird nicht gelesen.",
};

impl Adapter for Pnpm {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name == "pnpm-workspace.yaml" {
            return bounded(workspace(file));
        }
        let version = lines::yaml_keys(file.content)
            .find(|l| l.indent == 0 && l.key == "lockfileVersion")
            .map(|l| l.value.to_string())
            .unwrap_or_else(|| "?".into());
        let mut items = vec![manifest("pnpm", file, format!("Lockfile v{version}"))];
        for line in lines::section_keys(file.content, "packages") {
            if let Some((name, version)) = split_package_key(line.key) {
                let mut item = InventoryItem::new(ItemCategory::Package, "pnpm", name, file.rel_path())
                    .version(version)
                    .state(InstallState::Resolved)
                    .detail("aufgelöst laut pnpm-lock.yaml");
                item.evidence.qualifier = line.key.into();
                items.push(item);
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
                let mut item = InventoryItem::new(ItemCategory::Package, "pnpm", name, format!("{rel_path}/.pnpm"))
                    .version(version)
                    .state(InstallState::Installed)
                    .detail("im virtuellen Store vorhanden");
                item.evidence.qualifier = entry;
                items.push(item);
            }
        }
        bounded(items)
    }
}

/// pnpm-workspace.yaml: package patterns, the default `catalog` and named
/// `catalogs` (pnpm 9.5+), and the `overrides` pnpm 10 reads from here.
fn workspace(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut patterns = 0;
    let mut items = Vec::new();
    let mut overrides = Vec::new();
    let mut section = "";
    // Indent of the section's children, and the open named catalog with
    // the indent of its entries.
    let mut level = None;
    let mut catalog: Option<(&str, Option<usize>)> = None;
    for entry in lines::yaml_entries(file.content).take(200_000) {
        if entry.indent == 0 && !entry.item {
            section = entry.key.unwrap_or("");
            (level, catalog) = (None, None);
            continue;
        }
        let child = *level.get_or_insert(entry.indent);
        let Some(key) = entry.key else {
            patterns += usize::from(section == "packages" && entry.item && entry.indent == child);
            continue;
        };
        match section {
            "catalog" if entry.indent == child => items.extend(catalog_item(file, "default", key, entry.value)),
            "catalogs" if entry.indent == child => catalog = Some((key, None)),
            "catalogs" => {
                if let Some((name, entries)) = &mut catalog
                    && *entries.get_or_insert(entry.indent) == entry.indent
                {
                    items.extend(catalog_item(file, name, key, entry.value));
                }
            }
            "overrides" if entry.indent == child && !entry.value.is_empty() => {
                overrides.push(super::npm::override_edge(super::npm::pnpm_target(key), entry.value));
            }
            _ => {}
        }
    }
    let mut detail = format!("Workspace-Definition · {patterns} Einträge");
    if !items.is_empty() {
        detail.push_str(&format!(" · {} Katalogeinträge", items.len()));
    }
    if !overrides.is_empty() {
        detail.push_str(&format!(" · {} Overrides", overrides.len()));
    }
    let mut head = manifest("pnpm", file, detail);
    head.evidence.dependencies = overrides;
    items.insert(0, head);
    items
}

/// One catalog range as a declaration; workspace packages are none.
fn catalog_item(file: &FoundFile<'_>, catalog: &str, name: &str, spec: &str) -> Option<InventoryItem> {
    if spec.is_empty() || spec.starts_with("workspace:") || spec.starts_with(['{', '[']) {
        return None;
    }
    let (source, host) = super::npm::spec_source(spec);
    let mut item = InventoryItem::new(ItemCategory::Package, "pnpm", name, file.rel_path())
        .state(InstallState::Declared)
        .source(source, host)
        .detail(format!("Spezifikation {} · Katalog {catalog}", super::npm::spec_label(spec)));
    item.evidence.context = format!("catalog:{catalog}");
    Some(item)
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

    #[test]
    fn workspace_patterns_catalogs_and_overrides() {
        let yaml = "packages:\n  - 'apps/*'\n  - packages/*\n  - '!**/test/**'\n\n# Default catalog\ncatalog:\n  react: ^18.3.1\n  '@types/react': \"^18.3.0\" # pinned\n  my-lib: workspace:*\n\ncatalogs:\n  # referenced as catalog:react17\n  react17:\n    react: ^17.0.2\n    react-dom: ^17.0.2\n  legacy:\n    lodash: 4.17.21\n    local: link:../vendor/lodash\n\noverrides:\n  \"foo\": \"^1.0.0\"\n  \"qar@1>zoo\": \"2\"\n  \"foo@1.0.0>bar\": \"-\"\n  react: \"catalog:\"\n\nonlyBuiltDependencies:\n  - esbuild\n";
        let items = Pnpm.parse_file(&file("pnpm-workspace.yaml", yaml)).unwrap();
        assert_eq!(items[0].detail, "Workspace-Definition · 3 Einträge · 6 Katalogeinträge · 4 Overrides");
        let catalog: Vec<(&str, &str, &str)> =
            items.iter().skip(1).map(|i| (i.evidence.context.as_str(), i.name.as_str(), i.detail.as_str())).collect();
        assert_eq!(
            catalog,
            [
                ("catalog:default", "react", "Spezifikation ^18.3.1 · Katalog default"),
                ("catalog:default", "@types/react", "Spezifikation ^18.3.0 · Katalog default"),
                ("catalog:react17", "react", "Spezifikation ^17.0.2 · Katalog react17"),
                ("catalog:react17", "react-dom", "Spezifikation ^17.0.2 · Katalog react17"),
                ("catalog:legacy", "lodash", "Spezifikation 4.17.21 · Katalog legacy"),
                ("catalog:legacy", "local", "Spezifikation lokaler Pfad · Katalog legacy"),
            ]
        );
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        let edges: Vec<(&str, &str, &str)> = items[0]
            .evidence
            .dependencies
            .iter()
            .map(|d| (d.name.as_str(), d.requirement.as_str(), d.kind.as_str()))
            .collect();
        assert_eq!(
            edges,
            [
                ("foo", "^1.0.0", "override"),
                ("zoo", "2", "override"),
                ("bar", "entfernt", "override"),
                ("react", "catalog:", "override")
            ]
        );
        assert!(!format!("{items:?}").contains("../vendor"), "local paths are not stored");
    }
}
