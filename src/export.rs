//! Export path (PROJEKTPLAN §7.3): a further reduction on top of the local
//! inventory. By default absolute paths, project names and private package
//! names are replaced; the caller shows a preview before anything leaves.
use crate::domain::{Snapshot, SourceClass};
use crate::storage::PublishedItem;
use serde_json::{Value, json};

pub const EXPORT_VERSION: u32 = 1;

pub fn redacted(snapshot: &Snapshot, packages: &[PublishedItem], include_paths: bool) -> Value {
    let project_label = |index: usize, name: &str, path: &str| {
        if include_paths {
            json!({ "name": name, "path": path })
        } else {
            json!({ "name": format!("Projekt {}", index + 1) })
        }
    };
    let projects: Vec<Value> = snapshot
        .projects
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut value = project_label(i, &p.name, &p.path);
            value["ecosystem"] = json!(p.ecosystem);
            value["packages"] = json!(p.packages);
            value["protected"] = json!(p.protected);
            value
        })
        .collect();
    let packages: Vec<Value> = packages
        .iter()
        .map(|p| {
            let public = p.item.source == SourceClass::PublicRegistry;
            json!({
                "ecosystem": p.item.ecosystem,
                "name": if public || include_paths { p.item.name.clone() } else { "<privat oder unbekannt>".into() },
                "version": p.item.version,
                "state": p.item.install_state,
                "source": p.item.source,
            })
        })
        .collect();
    json!({
        "format": "mogumogu-export",
        "version": EXPORT_VERSION,
        "redacted": !include_paths,
        "note": if include_paths {
            "Enthält lokale Pfade und Namen – nur bewusst teilen."
        } else {
            "Pfade, Projektnamen und nicht öffentliche Paketnamen entfernt. Kein Anonymitätsversprechen."
        },
        "projects": projects,
        "packages": packages,
        "resources": snapshot.resources.len(),
        "ai_configurations": snapshot.ai.len(),
        "review_hints": snapshot.hints.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{InventoryItem, ItemCategory, Observation, Origin, Project};

    #[test]
    fn default_export_drops_paths_and_private_names() {
        let snapshot = Snapshot {
            projects: vec![Project {
                id: 1,
                name: "geheimprojekt".into(),
                path: r"C:\Users\alice\geheim".into(),
                ecosystem: "npm".into(),
                state: Observation::Unknown,
                evidence: String::new(),
                bytes: None,
                packages: None,
                protected: true,
                origin: Origin::Registered,
                read_approved: true,
                coverage: String::new(),
            }],
            ..Default::default()
        };
        let private = PublishedItem {
            scope_id: 1,
            project_id: Some(1),
            item: InventoryItem::new(ItemCategory::Package, "npm", "@corp/internal-billing", "x")
                .source(SourceClass::PrivateRegistry, Some("npm.corp".into())),
        };
        let text = redacted(&snapshot, &[private], false).to_string();
        for secret in ["geheimprojekt", "alice", "internal-billing", "npm.corp"] {
            assert!(!text.contains(secret), "{secret}");
        }
    }
}
