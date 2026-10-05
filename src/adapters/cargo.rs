//! Cargo: Cargo.toml (declared, workspace), Cargo.lock (resolved with
//! explicit sources) and `target/` as build output. Nothing is resolved,
//! built or fetched; `cargo metadata` would run build scripts of path deps.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::toml_table;

pub struct Cargo;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "cargo",
    name: "Cargo",
    markers: &["Cargo.toml", "Cargo.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    updates: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &["Cargo.toml (Abhängigkeitstabellen, Workspace)", "Cargo.lock v3/v4"],
    limits: "Features und Zielplattformen nur als Deklaration; ~/.cargo und Registry-Caches werden nicht gelesen.",
};

impl Adapter for Cargo {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let table = toml_table(file.content).map_err(|e| e.to_string())?;
        if file.name == "Cargo.lock" {
            return bounded(lock_items(file, &table));
        }
        bounded(manifest_items(file, &table))
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        (name == "target" && siblings.iter().any(|s| s == "Cargo.toml")).then_some(DirClaim {
            adapter: "cargo",
            category: ItemCategory::Output,
            label: "Cargo-Buildausgabe",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, _access: &mut dyn DirAccess) -> AdapterResult {
        Ok(vec![
            InventoryItem::new(ItemCategory::Output, "cargo", "target", rel_path)
                .detail("Generierte Buildausgabe; Größe erst nach Messung bekannt"),
        ])
    }
}

fn crates_io(source: &str) -> (SourceClass, Option<String>) {
    let lower = source.to_ascii_lowercase();
    if lower.starts_with("git+") {
        return (SourceClass::Git, None);
    }
    let url = lower.trim_start_matches("registry+").trim_start_matches("sparse+");
    match crate::privacy::url_host(url).as_deref() {
        Some("github.com") if url.contains("rust-lang/crates.io-index") => {
            (SourceClass::PublicRegistry, Some("crates.io".into()))
        }
        Some("index.crates.io") => (SourceClass::PublicRegistry, Some("crates.io".into())),
        Some(host) => (SourceClass::PrivateRegistry, Some(host.to_string())),
        None => (SourceClass::Unknown, None),
    }
}

fn lock_items(file: &FoundFile<'_>, table: &toml::Table) -> Vec<InventoryItem> {
    let version = table.get("version").and_then(|v| v.as_integer()).unwrap_or(1);
    let mut items = vec![manifest("cargo", file, format!("Lockfile v{version}"))];
    for package in table.get("package").and_then(|p| p.as_array()).into_iter().flatten() {
        let Some(name) = package.get("name").and_then(|n| n.as_str()) else { continue };
        let Some(source) = package.get("source").and_then(|s| s.as_str()) else { continue }; // workspace member
        let (class, host) = crates_io(source);
        items.push(
            InventoryItem::new(ItemCategory::Package, "cargo", name, file.rel_path())
                .version(package.get("version").and_then(|v| v.as_str()).unwrap_or_default())
                .state(InstallState::Resolved)
                .source(class, host)
                .detail("aufgelöst laut Cargo.lock"),
        );
    }
    items
}

fn manifest_items(file: &FoundFile<'_>, table: &toml::Table) -> Vec<InventoryItem> {
    let members = table.get("workspace").and_then(|w| w.get("members")).and_then(|m| m.as_array()).map_or(0, Vec::len);
    let name = table.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str());
    let detail = match (name, members) {
        (Some(name), 0) => format!("Crate {name}"),
        (Some(name), n) => format!("Crate {name} · Workspace mit {n} Mitgliedern"),
        (None, n) => format!("Virtueller Workspace mit {n} Mitgliedern"),
    };
    let mut items = vec![manifest("cargo", file, detail)];
    let mut sections: Vec<(&toml::Table, String)> = Vec::new();
    for (key, label) in [("dependencies", ""), ("dev-dependencies", " · dev"), ("build-dependencies", " · build")] {
        if let Some(deps) = table.get(key).and_then(|d| d.as_table()) {
            sections.push((deps, label.to_string()));
        }
    }
    for (target, spec) in table.get("target").and_then(|t| t.as_table()).into_iter().flatten() {
        if let Some(deps) = spec.get("dependencies").and_then(|d| d.as_table()) {
            sections.push((deps, format!(" · Ziel {target}")));
        }
    }
    if let Some(deps) = table.get("workspace").and_then(|w| w.get("dependencies")).and_then(|d| d.as_table()) {
        sections.push((deps, " · Workspace".into()));
    }
    for (deps, label) in sections {
        for (name, spec) in deps {
            let (source, requirement) = match spec {
                toml::Value::String(version) => (SourceClass::Unknown, version.clone()),
                toml::Value::Table(t) if t.contains_key("path") => (SourceClass::Path, "Pfad".into()),
                toml::Value::Table(t) if t.contains_key("git") => (SourceClass::Git, "Git".into()),
                toml::Value::Table(t) if t.contains_key("registry") => {
                    (SourceClass::PrivateRegistry, "eigene Registry".into())
                }
                toml::Value::Table(t) if t.get("workspace").and_then(|w| w.as_bool()) == Some(true) => {
                    (SourceClass::Unknown, "aus Workspace".into())
                }
                toml::Value::Table(t) => {
                    (SourceClass::Unknown, t.get("version").and_then(|v| v.as_str()).unwrap_or("?").to_string())
                }
                _ => (SourceClass::Unknown, "?".into()),
            };
            items.push(
                InventoryItem::new(ItemCategory::Package, "cargo", name.as_str(), file.rel_path())
                    .state(InstallState::Declared)
                    .source(source, None)
                    .detail(format!("Spezifikation {requirement}{label}")),
            );
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    #[test]
    fn lock_distinguishes_crates_io_git_and_members() {
        let lock = r#"version = 4
[[package]]
name = "mogumogu"
version = "0.1.0"
[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
[[package]]
name = "fork"
version = "0.1.0"
source = "git+https://github.com/x/fork?branch=main#abc"
"#;
        let items = Cargo.parse_file(&file("Cargo.lock", lock)).unwrap();
        assert_eq!(items.len(), 3);
        let serde = items.iter().find(|i| i.name == "serde").unwrap();
        assert_eq!(serde.source_host.as_deref(), Some("crates.io"));
        assert_eq!(items.iter().find(|i| i.name == "fork").unwrap().source, SourceClass::Git);
    }

    #[test]
    fn manifest_declares_and_target_is_output() {
        let text = "[package]\nname='x'\n[dependencies]\nserde={version='1',features=['derive']}\nlocal={path='../l'}\n[target.'cfg(windows)'.dependencies]\nwindows-sys='0.61'\n";
        let items = Cargo.parse_file(&file("Cargo.toml", text)).unwrap();
        assert_eq!(items.len(), 4);
        assert!(items.iter().any(|i| i.name == "windows-sys" && i.detail.contains("cfg(windows)")));
        assert!(Cargo.claim_dir("target", &["Cargo.toml".into()], &[]).is_some());
        assert!(Cargo.claim_dir("target", &[], &[]).is_none());
    }
}
