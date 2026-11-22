//! Static reference index. Every match remains evidence, never proof of non-use.
use crate::domain::*;
use crate::limits::PACKAGE_EVIDENCE_ROWS as EVIDENCE_ROWS;
use crate::storage::PublishedItem;
use std::collections::{BTreeSet, HashMap, HashSet};

type NameKey = (String, String);
type VersionKey = (String, String, String);
type DependencyRef<'a> = (&'a PublishedItem, &'a PackageDependency);

#[derive(Default)]
pub(super) struct ReferenceIndex<'a> {
    declarations: HashMap<NameKey, Vec<&'a PublishedItem>>,
    commands: HashMap<&'a str, Vec<&'a PublishedItem>>,
    executables: HashMap<PackageId, BTreeSet<&'a str>>,
    ai_packages: HashMap<NameKey, Vec<&'a PublishedItem>>,
    locked: HashMap<VersionKey, Vec<&'a PublishedItem>>,
    dependencies: HashMap<VersionKey, Vec<DependencyRef<'a>>>,
}

impl<'a> ReferenceIndex<'a> {
    /// Indexes only references that can match one of the `targets` shown on the
    /// current page, avoiding allocations for the rest of the inventory.
    pub fn build_for(items: &'a [PublishedItem], targets: &[PackageFinding]) -> Self {
        let mut index = Self::default();
        if targets.is_empty() {
            return index;
        }
        let wanted_ids: HashSet<&PackageId> = targets.iter().map(|f| &f.id).collect();
        let wanted_names: HashSet<(&str, &str)> =
            targets.iter().map(|f| (f.id.ecosystem.as_str(), f.id.name.as_str())).collect();
        let wanted_versions: HashSet<(&str, &str, &str)> =
            targets.iter().map(|f| (f.id.ecosystem.as_str(), f.id.name.as_str(), f.id.version.as_str())).collect();
        let mut wanted_commands: HashSet<&'a str> = HashSet::new();
        for p in items {
            let item = &p.item;
            let eco = package_ecosystem(&item.ecosystem);
            if item.category == ItemCategory::AiReference
                && item.detail == "Referenz, kein Installations- oder Nutzungsnachweis"
            {
                let mut parts = item.name.split_whitespace();
                if let (Some(runner), Some(target), None) = (parts.next(), parts.next(), parts.next()) {
                    let eco = match runner {
                        "npx" | "pnpx" | "bunx" => Some("npm"),
                        "uvx" => Some("python"),
                        _ => None,
                    };
                    if let Some(eco) = eco {
                        let name = target.rfind('@').filter(|index| *index > 0).map_or(target, |i| &target[..i]);
                        let norm = package_name(eco, name);
                        if wanted_names.contains(&(eco, norm.as_str())) {
                            index.ai_packages.entry((eco.into(), norm)).or_default().push(p);
                        }
                    }
                }
            }
            for dep in &item.evidence.dependencies {
                let dep_name = package_name(eco, &dep.name);
                if wanted_names.contains(&(eco, dep_name.as_str())) {
                    let req = if dep.exact { dep.requirement.clone() } else { String::new() };
                    index.dependencies.entry((eco.into(), dep_name, req)).or_default().push((p, dep));
                }
            }
            if item.category != ItemCategory::Package
                && !(item.category == ItemCategory::Cache && item.ecosystem == "nuget")
            {
                continue;
            }
            if item.install_state == Some(InstallState::Declared) {
                let norm = package_name(&item.ecosystem, &item.name);
                if wanted_names.contains(&(eco, norm.as_str())) {
                    index.declarations.entry((eco.into(), norm)).or_default().push(p);
                }
                continue;
            }
            let has_exec = !item.evidence.executables.is_empty();
            let is_resolved = item.install_state == Some(InstallState::Resolved);
            if !has_exec && !is_resolved {
                continue;
            }
            let norm = package_name(&item.ecosystem, &item.name);
            if !wanted_names.contains(&(eco, norm.as_str())) {
                continue;
            }
            let version = item.version.as_deref().unwrap_or("");
            if has_exec {
                let id = item.package_id();
                if wanted_ids.contains(&id) {
                    let entry = index.executables.entry(id).or_default();
                    for cmd in &item.evidence.executables {
                        entry.insert(cmd.as_str());
                        wanted_commands.insert(cmd.as_str());
                    }
                }
            }
            if is_resolved && wanted_versions.contains(&(eco, norm.as_str(), version)) {
                index.locked.entry((eco.into(), norm, version.into())).or_default().push(p);
            }
        }
        if !wanted_commands.is_empty() {
            for p in items {
                for command in &p.item.evidence.commands {
                    if wanted_commands.contains(command.as_str()) {
                        index.commands.entry(command.as_str()).or_default().push(p);
                    }
                }
            }
        }
        index
    }

    pub fn possible(&self, id: &PackageId, project_by_id: &HashMap<i64, &str>) -> Vec<String> {
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
