//! Bun: bun.lock (resolved; the text lockfile since Bun 1.2) and
//! node_modules (installed names). The binary bun.lockb is not read. An
//! empty registry field means the default registry, which bunfig.toml may
//! change, so such sources stay unknown and are never queried.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::json;

pub struct Bun;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "bun",
    name: "Bun",
    markers: &["bun.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["bun.lock v1 (Text, JSONC)", "node_modules (Ordnernamen)"],
    limits: "bun.lockb (binär) und bunfig.toml werden nicht gelesen – ohne Registry-Angabe bleibt die Quelle unbekannt.",
};

impl Adapter for Bun {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let value = json::parse_jsonc(file.content, 64).map_err(|e| e.to_string())?;
        let workspaces = value["workspaces"].as_object().map_or(0, |w| w.len());
        let version = value["lockfileVersion"].as_i64().unwrap_or(0);
        let mut items = vec![manifest("bun", file, format!("Lockfile v{version} · {workspaces} Workspaces"))];
        for (key, entry) in value["packages"].as_object().into_iter().flatten() {
            let Some((name, spec)) = entry[0].as_str().and_then(split_spec) else { continue };
            let (source, host) = match protocol(spec) {
                Some("workspace") => continue,
                Some("github" | "git" | "git+https" | "git+ssh") => (SourceClass::Git, None),
                Some("file" | "link") => (SourceClass::Path, None),
                _ => match entry[1].as_str().filter(|url| !url.is_empty()) {
                    Some(url) => classify_url(url, super::npm::PUBLIC),
                    None => (SourceClass::Unknown, None),
                },
            };
            let mut item = InventoryItem::new(ItemCategory::Package, "bun", name, file.rel_path())
                .version(spec)
                .state(InstallState::Resolved)
                .source(source, host)
                .detail("aufgelöst laut bun.lock");
            item.evidence.qualifier = key.clone();
            item.evidence.dependencies = super::npm::dependencies(&entry[2]);
            items.push(item);
        }
        bounded(items)
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        (name == "node_modules" && super::npm::manager(siblings) == "bun").then_some(DirClaim {
            adapter: "bun",
            category: ItemCategory::Environment,
            label: "node_modules (Bun)",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        super::npm::node_modules("bun", rel_path, access)
    }
}

/// `name@1.2.3`, `@scope/name@1.2.3`, `name@github:user/repo#ref`.
pub(super) fn split_spec(spec: &str) -> Option<(&str, &str)> {
    let start = usize::from(spec.starts_with('@'));
    let index = spec[start..].find('@')? + start;
    let (name, version) = (&spec[..index], &spec[index + 1..]);
    (!name.is_empty() && !version.is_empty()).then_some((name, version))
}

/// The protocol of a version spec (`workspace:`, `github:`, `file:` …).
pub(super) fn protocol(spec: &str) -> Option<&str> {
    let (prefix, _) = spec.split_once(':')?;
    prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '+').then_some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    #[test]
    fn text_lockfile_with_trailing_commas_and_protocols() {
        let lock = r#"{
          "lockfileVersion": 1,
          "workspaces": {
            "": { "name": "app", "dependencies": { "react": "^18.2.0", }, },
            "packages/ui": { "name": "@app/ui", },
          },
          "packages": {
            "react": ["react@18.2.0", "", { "dependencies": { "loose-envify": "^1.1.0" } }, "sha512-x"],
            "@types/node": ["@types/node@20.1.0", "", {}, "sha512-y"],
            "@corp/x": ["@corp/x@2.0.0", "https://npm.corp.example/", {}, "sha512-z"],
            "@app/ui": ["@app/ui@workspace:packages/ui"],
            "tool": ["tool@github:user/tool#0123abc", {}, "user-tool-0123abc"],
          },
        }"#;
        let items = Bun.parse_file(&file("bun.lock", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v1 · 2 Workspaces");
        let react = items.iter().find(|i| i.name == "react").unwrap();
        assert_eq!((react.version.as_deref(), react.install_state), (Some("18.2.0"), Some(InstallState::Resolved)));
        assert_eq!(react.source, SourceClass::Unknown, "an empty registry is the configurable default");
        assert_eq!(react.evidence.dependencies[0].name, "loose-envify");
        assert_eq!(items.iter().find(|i| i.name == "@types/node").unwrap().version.as_deref(), Some("20.1.0"));
        assert_eq!(items.iter().find(|i| i.name == "@corp/x").unwrap().source, SourceClass::PrivateRegistry);
        assert_eq!(items.iter().find(|i| i.name == "tool").unwrap().source, SourceClass::Git);
        assert!(!items.iter().any(|i| i.name == "@app/ui"), "own workspace packages are no dependencies");
        assert!(Bun.parse_file(&file("bun.lock", "{\"packages\": [")).is_err());
    }

    #[test]
    fn node_modules_beside_a_bun_lockfile() {
        assert!(Bun.claim_dir("node_modules", &["bun.lockb".into()], &[]).is_some());
        assert!(Bun.claim_dir("node_modules", &["package-lock.json".into()], &[]).is_none());
        let mut dir = FakeDir::default();
        dir.dirs.insert(String::new(), vec![("react".into(), true), (".bin".into(), true)]);
        let claim = Bun.claim_dir("node_modules", &["bun.lock".into()], &[]).unwrap();
        let items = Bun.inspect_dir(claim, "node_modules", &mut dir).unwrap();
        assert!(items.iter().any(|i| i.name == "react" && i.ecosystem == "bun"));
        assert_eq!(items.len(), 2);
    }
}
