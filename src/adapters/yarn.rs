//! Yarn: yarn.lock in the classic v1 format and the Berry format (v2+),
//! read line by line (resolved), and node_modules (installed names).
//! `.yarnrc.yml` is not read: Berry names no registry, so its sources stay
//! unknown; a classic `resolved` URL is classified like npm's.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};

pub struct Yarn;

const PUBLIC: &[&str] = &["registry.yarnpkg.com", "registry.npmjs.org"];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "yarn",
    name: "Yarn",
    markers: &["yarn.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["yarn.lock v1", "yarn.lock Berry (v2+)", "node_modules (Ordnernamen)"],
    limits: ".yarnrc.yml wird nicht gelesen – Berry-Quellen bleiben unbekannt; Plug'n'Play-Cache (.yarn/cache) nicht inventarisiert.",
};

/// One lockfile entry while its indented properties are read.
#[derive(Default)]
struct Entry {
    key: String,
    version: Option<String>,
    resolved: Option<String>,
    dependencies: Vec<PackageDependency>,
}

impl Adapter for Yarn {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let berry = file.content.lines().any(|l| l.starts_with("__metadata:"));
        let mut items = vec![manifest("yarn", file, if berry { "Lockfile Berry" } else { "Lockfile v1" })];
        let mut entry: Option<Entry> = None;
        let mut section = "";
        for line in file.content.lines().take(2_000_000) {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            if indent == 0 {
                items.extend(entry.take().and_then(|e| package(e, file, berry)));
                entry = (!line.starts_with("__metadata"))
                    .then(|| line.strip_suffix(':'))
                    .flatten()
                    .map(|key| Entry { key: key.to_string(), ..Entry::default() });
                continue;
            }
            let Some(current) = entry.as_mut() else { continue };
            if indent == 2 {
                section = "";
                if let Some(value) = property(trimmed, "version") {
                    current.version = Some(value);
                } else if let Some(value) = property(trimmed, "resolved").or_else(|| property(trimmed, "resolution")) {
                    current.resolved = Some(value);
                } else if let Some(name) = trimmed.strip_suffix(':') {
                    section = match name {
                        "dependencies" | "optionalDependencies" | "peerDependencies" => name,
                        _ => "",
                    };
                }
            } else if indent == 4
                && !section.is_empty()
                && let Some((name, requirement)) = requirement(trimmed)
            {
                current.dependencies.push(PackageDependency { name, requirement, kind: section.into(), exact: false });
            }
        }
        items.extend(entry.and_then(|e| package(e, file, berry)));
        bounded(items)
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        (name == "node_modules" && super::npm::manager(siblings) == "yarn").then_some(DirClaim {
            adapter: "yarn",
            category: ItemCategory::Environment,
            label: "node_modules (Yarn)",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        super::npm::node_modules("yarn", rel_path, access)
    }
}

fn package(entry: Entry, file: &FoundFile<'_>, berry: bool) -> Option<InventoryItem> {
    // `"a@^1.0.0", "a@^1.2.0"` (v1) or `"a@npm:^1.0.0, a@npm:^1.2.0"` (Berry).
    let first = entry.key.split(',').next()?.trim().trim_matches('"');
    let (name, range) = super::bun::split_spec(first)?;
    let resolved = entry.resolved.as_deref().unwrap_or("");
    let (source, host) = match super::bun::protocol(range) {
        Some("workspace") => return None,
        Some("file" | "link" | "portal") => (SourceClass::Path, None),
        _ if resolved.contains("#commit=") || resolved.starts_with("git") => (SourceClass::Git, None),
        _ if berry => (SourceClass::Unknown, None),
        _ if resolved.is_empty() => (SourceClass::Unknown, None),
        _ => classify_url(resolved, PUBLIC),
    };
    let range = range.strip_prefix("npm:").unwrap_or(range);
    let mut item = InventoryItem::new(ItemCategory::Package, "yarn", name, file.rel_path())
        .version(entry.version.unwrap_or_default())
        .state(InstallState::Resolved)
        .source(source, host)
        .detail(format!("aufgelöst laut yarn.lock · Spezifikation {range}"));
    item.evidence.qualifier = first.to_string();
    item.evidence.dependencies = entry.dependencies;
    Some(item)
}

/// `version "1.2.3"` (v1) or `version: 1.2.3` (Berry).
fn property(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?;
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    rest.starts_with(' ').then(|| rest.trim().trim_matches('"').to_string())
}

/// `"@babel/highlight" "^7.12.13"` (v1) or `"@babel/highlight": ^7.12.13`.
fn requirement(line: &str) -> Option<(String, String)> {
    let (name, rest) = match line.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"')?,
        None => line.split_once([' ', ':'])?,
    };
    let requirement = rest.trim_start_matches(':').trim().trim_matches('"');
    (!name.is_empty()).then(|| (name.to_string(), requirement.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    #[test]
    fn classic_lockfile() {
        let lock = "# yarn lockfile v1\n\n\n\"@babel/code-frame@^7.0.0\", \"@babel/code-frame@^7.10.4\":\n  version \"7.12.13\"\n  resolved \"https://registry.yarnpkg.com/@babel/code-frame/-/code-frame-7.12.13.tgz#dcfc\"\n  integrity sha512-x\n  dependencies:\n    \"@babel/highlight\" \"^7.12.13\"\n\njs-tokens@^4.0.0:\n  version \"4.0.0\"\n  resolved \"https://npm.corp.example/js-tokens-4.0.0.tgz\"\n\nlocal@file:../local:\n  version \"1.0.0\"\n";
        let items = Yarn.parse_file(&file("yarn.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v1");
        let frame = items.iter().find(|i| i.name == "@babel/code-frame").unwrap();
        assert_eq!(frame.version.as_deref(), Some("7.12.13"));
        assert_eq!(frame.source, SourceClass::PublicRegistry);
        assert_eq!(frame.evidence.dependencies[0].name, "@babel/highlight");
        assert_eq!(frame.evidence.dependencies[0].requirement, "^7.12.13");
        assert_eq!(items.iter().find(|i| i.name == "js-tokens").unwrap().source, SourceClass::PrivateRegistry);
        assert_eq!(items.iter().find(|i| i.name == "local").unwrap().source, SourceClass::Path);
        assert_eq!(items.len(), 4);
    }

    #[test]
    fn berry_lockfile() {
        let lock = "__metadata:\n  version: 8\n  cacheKey: 10c0\n\n\"@babel/code-frame@npm:^7.0.0, @babel/code-frame@npm:^7.10.4\":\n  version: 7.12.13\n  resolution: \"@babel/code-frame@npm:7.12.13\"\n  dependencies:\n    \"@babel/highlight\": \"npm:^7.12.13\"\n  checksum: 10c0/abc\n  languageName: node\n  linkType: hard\n\n\"app@workspace:.\":\n  version: 0.0.0-use.local\n  resolution: \"app@workspace:.\"\n\n\"tool@https://github.com/user/tool.git#commit=0123abc\":\n  version: 1.0.0\n  resolution: \"tool@https://github.com/user/tool.git#commit=0123abc\"\n";
        let items = Yarn.parse_file(&file("yarn.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile Berry");
        let frame = items.iter().find(|i| i.name == "@babel/code-frame").unwrap();
        assert_eq!(frame.version.as_deref(), Some("7.12.13"));
        assert_eq!(frame.source, SourceClass::Unknown, ".yarnrc.yml is not read");
        assert!(frame.detail.ends_with("Spezifikation ^7.0.0"), "{}", frame.detail);
        assert_eq!(frame.evidence.dependencies[0].requirement, "npm:^7.12.13");
        assert!(!items.iter().any(|i| i.name == "app"), "workspaces are no dependencies");
        assert_eq!(items.iter().find(|i| i.name == "tool").unwrap().source, SourceClass::Git);
    }
}
