//! Scoop: static read of an approved Scoop root (`apps/<app>/<version>`,
//! `install.json`, `manifest.json`, `persist/`, `cache/`). The `current`
//! junction is never followed; no Scoop or package script is started.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory};
use crate::parsers::json;

pub struct Scoop;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "scoop",
    name: "Scoop",
    markers: &[],
    scope: AdapterScope::System,
    inventory: Support::Experimental,
    updates: Support::Unsupported,
    cleanup: Support::Unsupported,
    formats: &["apps/<app>/<version>/install.json", "apps/<app>/<version>/manifest.json"],
    limits: "Nur freigegebene Scoop-Wurzel; Buckets werden nicht aktualisiert, Installationsskripte nicht ausgeführt.",
};

impl Adapter for Scoop {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn inventory_root(&self, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = Vec::new();
        for (app, is_dir) in access.list(&["apps"]) {
            if !is_dir {
                continue;
            }
            for (version, version_dir) in access.list(&["apps", &app]) {
                if !version_dir || version == "current" {
                    continue;
                }
                let install = access.read_text(&["apps", &app, &version, "install.json"], 1 << 20);
                let bucket = install
                    .as_deref()
                    .and_then(|t| json::parse(t, 16).ok())
                    .and_then(|v| v["bucket"].as_str().map(str::to_string))
                    .unwrap_or_else(|| "unbekannt".into());
                let declared = access
                    .read_text(&["apps", &app, &version, "manifest.json"], 1 << 20)
                    .and_then(|t| json::parse(&t, 16).ok())
                    .and_then(|v| v["version"].as_str().map(str::to_string));
                items.push(
                    InventoryItem::new(ItemCategory::Software, "scoop", app.as_str(), format!("apps/{app}/{version}"))
                        .version(declared.unwrap_or(version.clone()))
                        .state(InstallState::Installed)
                        .detail(format!("Bucket {bucket}")),
                );
            }
        }
        for (app, is_dir) in access.list(&["persist"]) {
            if is_dir {
                items.push(
                    InventoryItem::new(
                        ItemCategory::Output,
                        "scoop",
                        format!("{app} · persistente Daten"),
                        format!("persist/{app}"),
                    )
                    .detail("Benutzerdaten der App; bleiben geschützt"),
                );
            }
        }
        if !access.list(&["cache"]).is_empty() {
            items.push(
                InventoryItem::new(ItemCategory::Cache, "scoop", "Download-Cache", "cache")
                    .detail("Gemeinsamer Scoop-Cache"),
            );
        }
        bounded(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::FakeDir;

    #[test]
    fn versions_without_current_junction() {
        let mut dir = FakeDir::default();
        dir.dirs.insert("apps".into(), vec![("git".into(), true)]);
        dir.dirs.insert("apps/git".into(), vec![("2.45.0".into(), true), ("current".into(), true)]);
        dir.files.insert("apps/git/2.45.0/install.json".into(), r#"{"bucket":"main","architecture":"64bit"}"#.into());
        dir.dirs.insert("persist".into(), vec![("git".into(), true)]);
        let items = Scoop.inventory_root(&mut dir).unwrap();
        assert_eq!(items.iter().filter(|i| i.category == ItemCategory::Software).count(), 1);
        assert!(items[0].detail.contains("main"));
        assert!(items.iter().any(|i| i.category == ItemCategory::Output));
    }
}
