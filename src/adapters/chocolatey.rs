//! Chocolatey: static read of an approved Chocolatey root (`lib/<pkg>/
//! <pkg>.nuspec`, `lib-bad/`). No chocolatey PowerShell is ever started.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory};
use crate::parsers::xml;

pub struct Chocolatey;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "chocolatey",
    name: "Chocolatey",
    markers: &[],
    scope: AdapterScope::System,
    inventory: Support::Experimental,
    updates: Support::Unsupported,
    cleanup: Support::Unsupported,
    formats: &["lib/<paket>/<paket>.nuspec", "lib-bad/<paket>"],
    limits: "Nur freigegebene Chocolatey-Wurzel; Paketskripte werden nie ausgeführt.",
};

impl Adapter for Chocolatey {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn inventory_root(&self, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = Vec::new();
        for (package, is_dir) in access.list(&["lib"]) {
            if !is_dir {
                continue;
            }
            let nuspec = access
                .list(&["lib", &package])
                .into_iter()
                .find(|(name, dir)| !dir && name.to_ascii_lowercase().ends_with(".nuspec"))
                .and_then(|(name, _)| access.read_text(&["lib", &package, &name], 1 << 20));
            let (id, version) = nuspec
                .as_deref()
                .and_then(|text| xml::elements(text, 2_000, 16).ok())
                .map(|elements| {
                    let text = |n: &str| {
                        elements.iter().find(|e| e.name == n && e.parent == "metadata").map(|e| e.text.clone())
                    };
                    (text("id"), text("version"))
                })
                .unwrap_or((None, None));
            let item = InventoryItem::new(
                ItemCategory::Software,
                "chocolatey",
                id.unwrap_or(package.clone()),
                format!("lib/{package}"),
            )
            .state(InstallState::Installed);
            items.push(match version {
                Some(v) => item.version(v).detail("laut .nuspec"),
                None => item.detail("ohne lesbare .nuspec – Version unbekannt"),
            });
        }
        for (package, is_dir) in access.list(&["lib-bad"]) {
            if is_dir {
                items.push(
                    InventoryItem::new(
                        ItemCategory::Software,
                        "chocolatey",
                        package.as_str(),
                        format!("lib-bad/{package}"),
                    )
                    .detail("Defekter Installationsnachweis (lib-bad)"),
                );
            }
        }
        bounded(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::FakeDir;

    #[test]
    fn nuspec_and_bad_installs() {
        let mut dir = FakeDir::default();
        dir.dirs.insert("lib".into(), vec![("git".into(), true)]);
        dir.dirs.insert("lib/git".into(), vec![("git.nuspec".into(), false)]);
        dir.files.insert(
            "lib/git/git.nuspec".into(),
            r#"<?xml version="1.0"?><package><metadata><id>git</id><version>2.45.1</version></metadata></package>"#
                .into(),
        );
        dir.dirs.insert("lib-bad".into(), vec![("broken".into(), true)]);
        let items = Chocolatey.inventory_root(&mut dir).unwrap();
        assert_eq!(items[0].version.as_deref(), Some("2.45.1"));
        assert!(items[1].detail.contains("lib-bad"));
        assert_eq!(items[1].install_state, None);
    }
}
