//! Static reference index. Every match remains evidence, never proof of non-use.
use crate::domain::*;
use crate::limits::PACKAGE_EVIDENCE_ROWS as EVIDENCE_ROWS;
use crate::storage::PublishedItem;
use std::collections::{BTreeMap, BTreeSet};

type NameKey = (String, String);
type VersionKey = (String, String, String);
type DependencyRef<'a> = (&'a PublishedItem, &'a PackageDependency);

#[derive(Default)]
pub(super) struct ReferenceIndex<'a> {
    declarations: BTreeMap<NameKey, Vec<&'a PublishedItem>>,
    commands: BTreeMap<String, Vec<&'a PublishedItem>>,
    executables: BTreeMap<PackageId, BTreeSet<String>>,
    ai_packages: BTreeMap<NameKey, Vec<&'a PublishedItem>>,
    locked: BTreeMap<VersionKey, Vec<&'a PublishedItem>>,
    dependencies: BTreeMap<VersionKey, Vec<DependencyRef<'a>>>,
}

impl<'a> ReferenceIndex<'a> {
    pub fn build(items: &'a [PublishedItem]) -> Self {
        let mut index = Self::default();
        for p in items {
            let item = &p.item;
            let eco = package_ecosystem(&item.ecosystem).to_string();
            for command in &item.evidence.commands {
                index.commands.entry(command.clone()).or_default().push(p);
            }
            if item.category == ItemCategory::AiReference
                && item.detail == "Referenz, kein Installations- oder Nutzungsnachweis"
            {
                let parts = item.name.split_whitespace().collect::<Vec<_>>();
                if let [runner, target] = parts.as_slice() {
                    let eco = match *runner {
                        "npx" | "pnpx" | "bunx" => Some("npm"),
                        "uvx" => Some("python"),
                        _ => None,
                    };
                    if let Some(eco) = eco {
                        let name = target.rfind('@').filter(|index| *index > 0).map_or(*target, |i| &target[..i]);
                        index.ai_packages.entry((eco.into(), package_name(eco, name))).or_default().push(p);
                    }
                }
            }
            for dep in &item.evidence.dependencies {
                index
                    .dependencies
                    .entry((
                        eco.clone(),
                        package_name(&eco, &dep.name),
                        if dep.exact { dep.requirement.clone() } else { String::new() },
                    ))
                    .or_default()
                    .push((p, dep));
            }
            if item.category != ItemCategory::Package
                && !(item.category == ItemCategory::Cache && item.ecosystem == "nuget")
            {
                continue;
            }
            if item.install_state == Some(InstallState::Declared) {
                index.declarations.entry((eco, package_name(&item.ecosystem, &item.name))).or_default().push(p);
                continue;
            }
            let id = item.package_id();
            index.executables.entry(id.clone()).or_default().extend(item.evidence.executables.iter().cloned());
            if item.install_state == Some(InstallState::Resolved) {
                index.locked.entry((id.ecosystem.clone(), id.name.clone(), id.version.clone())).or_default().push(p);
            }
        }
        index
    }

    pub fn possible(&self, id: &PackageId, project_by_id: &BTreeMap<i64, &str>) -> Vec<String> {
        let Self { declarations, commands, executables, ai_packages, locked, dependencies } = self;
        let mut values = Vec::new();
        let name_key = (id.ecosystem.clone(), id.name.clone());
        for command in executables.get(id).into_iter().flatten() {
            for p in commands.get(command).into_iter().flatten().take(EVIDENCE_ROWS) {
                let project = p.project_id.and_then(|id| project_by_id.get(&id)).copied().unwrap_or("Projekt");
                push(
                    &mut values,
                    format!("{project} · {} · Befehlsname {command} (Auflösung/Ausführung ungeprüft)", p.item.rel_path),
                );
            }
        }
        for p in ai_packages.get(&name_key).into_iter().flatten().take(EVIDENCE_ROWS) {
            let project = p.project_id.and_then(|id| project_by_id.get(&id)).copied().unwrap_or("Projekt");
            push(
                &mut values,
                format!("{project} · {} · MCP-Startreferenz {} (kein Nutzungsnachweis)", p.item.rel_path, p.item.name),
            );
        }
        for p in locked
            .get(&(id.ecosystem.clone(), id.name.clone(), id.version.clone()))
            .into_iter()
            .flatten()
            .take(EVIDENCE_ROWS)
        {
            if p.item.version.as_deref() == Some(&id.version) && p.item.package_id() != *id {
                let project = p.project_id.and_then(|id| project_by_id.get(&id)).copied().unwrap_or("Projekt");
                push(
                    &mut values,
                    format!("{project} · {} · gleiche Version, Quelle nicht abgeglichen", p.item.rel_path),
                );
            }
        }
        for p in declarations.get(&name_key).into_iter().flatten().take(EVIDENCE_ROWS) {
            let project = p.project_id.and_then(|id| project_by_id.get(&id)).copied().unwrap_or("Projekt");
            push(
                &mut values,
                format!("{project} · {} · {} (Version/Quelle nicht aufgelöst)", p.item.rel_path, p.item.detail),
            );
        }
        for (p, dep) in dependencies
            .get(&(id.ecosystem.clone(), id.name.clone(), id.version.clone()))
            .into_iter()
            .flatten()
            .chain(dependencies.get(&(id.ecosystem.clone(), id.name.clone(), String::new())).into_iter().flatten())
            .take(EVIDENCE_ROWS)
        {
            let project = p.project_id.and_then(|id| project_by_id.get(&id)).copied().unwrap_or("Globales Tool");
            let parent = if p.item.category == ItemCategory::Manifest {
                p.item.rel_path.clone()
            } else {
                format!("{}@{}", p.item.name, p.item.version.as_deref().unwrap_or("?"))
            };
            push(
                &mut values,
                format!(
                    "{project} · {parent} → {} ({}, {})",
                    id.name,
                    dep.kind,
                    if dep.requirement.is_empty() { "Version offen" } else { &dep.requirement }
                ),
            );
        }
        values
    }
}

pub(super) fn push(values: &mut Vec<String>, value: String) {
    if values.len() >= EVIDENCE_ROWS {
        return;
    }
    let value = crate::privacy::label(&value);
    if values.len() < EVIDENCE_ROWS && !values.contains(&value) {
        values.push(value);
    }
}
