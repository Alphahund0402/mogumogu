//! AI access per approved folder, from the published AI artifacts and the
//! folder's known outputs. Pure calculation over loaded metadata.
use crate::domain::*;
use crate::profiles;
use crate::storage::PublishedItem;
use std::collections::BTreeMap;

/// Folders listed as candidates to keep from an agent.
const KEEP_OUT: usize = 8;

#[derive(Default)]
struct Gathered {
    tools: Vec<String>,
    access: AiAccess,
    ignore_files: Vec<String>,
}

pub(super) fn ai_access(
    items: &[PublishedItem],
    scopes: &[Scope],
    projects: &[Project],
    artifacts: &[ArtifactFinding],
) -> Vec<ProjectAccess> {
    let catalog = profiles::catalog();
    let mut by_scope: BTreeMap<i64, Gathered> = BTreeMap::new();
    for p in items.iter().filter(|p| p.item.category == ItemCategory::AiArtifact) {
        let entry = by_scope.entry(p.scope_id).or_default();
        let tool = catalog.profile(&p.item.ecosystem).map_or_else(|| p.item.ecosystem.clone(), |c| c.name.clone());
        if !entry.tools.contains(&tool) {
            entry.tools.push(tool);
        }
        let Some(found) = &p.item.evidence.ai_access else { continue };
        let total = &mut entry.access;
        for setting in &found.unrestricted {
            let named = format!("{}: {setting}", catalog.profile(&p.item.ecosystem).map_or("?", |c| c.name.as_str()));
            if !total.unrestricted.contains(&named) {
                total.unrestricted.push(named);
            }
        }
        total.extra_dirs += found.extra_dirs;
        total.extra_dirs_outside += found.extra_dirs_outside;
        total.read_denies += found.read_denies;
        total.hooks += found.hooks;
        total.servers_outside += found.servers_outside;
        total.sandbox = match (total.sandbox, found.sandbox) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), _) | (_, Some(false)) => Some(false),
            _ => None,
        };
        if found.ignore_file {
            total.ignore_file = true;
            let name = p.item.rel_path.rsplit('/').next().unwrap_or(&p.item.rel_path).to_string();
            if !entry.ignore_files.contains(&name) {
                entry.ignore_files.push(name);
            }
        }
    }
    let mut rows: Vec<ProjectAccess> = by_scope
        .into_iter()
        .filter_map(|(scope_id, gathered)| {
            let scope = scopes.iter().find(|s| s.id == scope_id && s.status == ScopeStatus::Approved)?;
            let name = scope
                .project_id
                .and_then(|id| projects.iter().find(|p| p.id == id))
                .map_or_else(|| scope.kind.label().to_string(), |p| p.name.clone());
            let a = &gathered.access;
            let level = if !a.unrestricted.is_empty() || a.extra_dirs_outside > 0 || a.servers_outside > 0 {
                AccessLevel::Wide
            } else if a.read_denies > 0 || a.ignore_file || a.sandbox == Some(true) {
                AccessLevel::Limited
            } else {
                AccessLevel::Open
            };
            let mut findings: Vec<String> =
                a.unrestricted.iter().map(|s| format!("{s} – Bestätigungen oder Sandbox aus")).collect();
            if a.extra_dirs_outside > 0 {
                findings.push(format!("{} zusätzliche Ordner außerhalb des Projekts freigegeben", a.extra_dirs_outside));
            }
            if a.servers_outside > 0 {
                findings.push(format!("{} MCP-Server mit einem Ordner außerhalb des Projekts", a.servers_outside));
            }
            if a.hooks > 0 {
                findings.push(format!("{} Hooks führen bei Ereignissen Befehle aus", a.hooks));
            }
            if a.read_denies > 0 {
                findings.push(format!("{} Lese- und Ausschlussregeln", a.read_denies));
            }
            if !gathered.ignore_files.is_empty() {
                findings.push(format!("Ignore-Dateien: {}", gathered.ignore_files.join(", ")));
            }
            match a.sandbox {
                Some(true) => findings.push("Sandbox laut Einstellung an".into()),
                Some(false) => findings.push("Sandbox laut Einstellung aus".into()),
                None => {}
            }
            if level == AccessLevel::Open {
                findings.push("Kein Kontextfilter: Der Agent kann jede Datei dieses Ordners lesen".into());
            }
            if gathered.tools.iter().any(|t| t == "Claude Code") && a.read_denies > 0 && a.sandbox != Some(true) {
                findings.push(
                    "Lese-Verbote von Claude Code binden nur seine Datei-Werkzeuge; Shell-Befehle laufen unter nativem Windows ohne Sandbox"
                        .into(),
                );
            }
            let mut keep_out: Vec<String> = artifacts
                .iter()
                .filter(|f| f.scope_id == scope_id)
                .map(|f| if f.rel_path.is_empty() { ".".to_string() } else { f.rel_path.clone() })
                .collect();
            keep_out.dedup();
            keep_out.truncate(KEEP_OUT);
            Some(ProjectAccess {
                scope_id,
                name,
                path: scope.path.clone(),
                audience: scope.kind.audience().into(),
                tools: gathered.tools,
                level,
                findings,
                keep_out,
            })
        })
        .collect();
    rows.sort_by_key(|r| (r.level != AccessLevel::Wide, r.level != AccessLevel::Open, r.name.clone()));
    rows
}
