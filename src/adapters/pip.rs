//! pip / Python: requirements files and pyproject.toml (declared) plus
//! virtual environments recognised by `pyvenv.cfg` (installed, from
//! `*.dist-info` folder names). No interpreter is ever started: Python
//! start-up would execute `.pth` lines (PROJEKTPLAN §7.2).
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::{lines, toml_table};

pub struct Pip;

pub(super) const PUBLIC: &[&str] = &["pypi.org", "files.pythonhosted.org"];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "pip",
    name: "pip",
    markers: &["requirements*.txt", "pyproject.toml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    updates: Support::Limited,
    cleanup: Support::Unsupported,
    formats: &[
        "requirements*.txt",
        "pyproject.toml (PEP 621, dependency-groups, Poetry)",
        "venv: pyvenv.cfg + *.dist-info",
    ],
    limits: "Keine Interpreterstarts; pip.conf außerhalb des Projekts wird nicht gelesen, Quellen bleiben ohne Indexangabe unbekannt.",
};

impl Adapter for Pip {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("pyproject.toml") {
            if file.siblings.iter().any(|s| s == "uv.lock") {
                return Ok(Vec::new()); // uv owns this project
            }
            return bounded(pyproject_items(file, "pip")?);
        }
        bounded(requirements(file))
    }

    fn claim_dir(&self, _name: &str, _siblings: &[String], children: &[String]) -> Option<DirClaim> {
        children.iter().any(|c| c.eq_ignore_ascii_case("pyvenv.cfg")).then_some(DirClaim {
            adapter: "pip",
            category: ItemCategory::Environment,
            label: "Python-Umgebung",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let config = access.read_text(&["pyvenv.cfg"], 64 * 1024).unwrap_or_default();
        let pairs = lines::key_values(&config);
        let value = |key: &str| pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str());
        let ecosystem = if value("uv").is_some() { "uv" } else { "pip" };
        let python = value("version").or_else(|| value("version_info")).unwrap_or("unbekannt");
        let mut packages = Vec::new();
        for site in site_packages(access) {
            let path: Vec<&str> = site.iter().map(String::as_str).collect();
            for (entry, is_dir) in access.list(&path) {
                let Some(stem) = entry.strip_suffix(".dist-info").filter(|_| is_dir) else { continue };
                let Some((name, version)) = stem.split_once('-') else { continue };
                packages.push(
                    InventoryItem::new(ItemCategory::Package, ecosystem, normalize(name), rel_path)
                        .version(version)
                        .state(InstallState::Installed)
                        .detail("installiert (dist-info); nicht importiert"),
                );
            }
        }
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, ecosystem, "Python-Umgebung", rel_path)
                .detail(format!("Python {python} · {} Pakete · kein Interpreterstart", packages.len())),
        ];
        items.extend(packages);
        bounded(items)
    }
}

/// Windows (`Lib/site-packages`) and POSIX (`lib/pythonX.Y/site-packages`).
fn site_packages(access: &mut dyn DirAccess) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for (entry, is_dir) in access.list(&[]) {
        if !is_dir || !entry.eq_ignore_ascii_case("lib") {
            continue;
        }
        for (child, child_dir) in access.list(&[&entry]) {
            if child_dir && child.eq_ignore_ascii_case("site-packages") {
                out.push(vec![entry.clone(), child]);
            } else if child_dir && child.starts_with("python") {
                out.push(vec![entry.clone(), child, "site-packages".into()]);
            }
        }
    }
    out
}

/// PEP 503 normalisation.
pub(super) fn normalize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut separator = false;
    for ch in name.chars() {
        if matches!(ch, '-' | '_' | '.') {
            separator = true;
            continue;
        }
        if separator && !out.is_empty() {
            out.push('-');
        }
        separator = false;
        out.push(ch.to_ascii_lowercase());
    }
    out
}

/// Splits a PEP 508 requirement into normalised name and specifier.
pub(super) fn requirement(text: &str) -> Option<(String, String)> {
    let text = text.split(';').next()?.trim();
    let end = text.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))).unwrap_or(text.len());
    let name = &text[..end];
    if name.is_empty() {
        return None;
    }
    let rest = text[end..].trim();
    let rest = rest.strip_prefix('[').and_then(|r| r.split_once(']')).map_or(rest, |(_, r)| r.trim());
    Some((normalize(name), rest.to_string()))
}

fn requirements(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let joined = file.content.replace("\\\r\n", " ").replace("\\\n", " ");
    let mut index: Option<(SourceClass, Option<String>)> = None;
    let mut entries = Vec::new();
    let mut references = 0;
    for line in joined.lines().take(20_000) {
        let line = line.split(" #").next().unwrap_or(line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or_default();
        match first {
            "-i" | "--index-url" | "--extra-index-url" => {
                if let Some(url) = words.next() {
                    let class = classify_url(url, PUBLIC);
                    if index.as_ref().is_none_or(|(c, _)| *c == SourceClass::PublicRegistry) {
                        index = Some(class);
                    }
                }
            }
            "-r" | "--requirement" | "-c" | "--constraint" => references += 1,
            "-e" | "--editable" => {
                let target = words.next().unwrap_or_default();
                let (source, _) = classify_url(target, PUBLIC);
                let source = if source == SourceClass::Git { SourceClass::Git } else { SourceClass::Path };
                entries.push(("editable".to_string(), String::new(), Some(source)));
            }
            _ if first.starts_with('-') => {}
            _ => {
                if let Some((name, spec)) = requirement(line) {
                    let direct = spec.strip_prefix('@').map(|url| classify_url(url.trim(), PUBLIC).0);
                    entries.push((name, spec, direct));
                }
            }
        }
    }
    let (index_class, index_host) = index.unwrap_or((SourceClass::Unknown, None));
    let mut items = vec![manifest(
        "pip",
        file,
        format!("Requirements · {} Einträge · {references} Verweise (nicht gefolgt)", entries.len()),
    )];
    for (name, spec, direct) in entries {
        let (source, host) = match direct {
            Some(class) => (class, None),
            None => (index_class, index_host.clone()),
        };
        let version = spec.strip_prefix("==").filter(|v| !v.contains(['*', ',']));
        let mut item = InventoryItem::new(ItemCategory::Package, "pip", name, file.rel_path())
            .state(InstallState::Declared)
            .source(source, host)
            .detail(if spec.is_empty() { "ohne Versionsangabe".into() } else { format!("Spezifikation {spec}") });
        if let Some(version) = version {
            item = item.version(version.trim());
        }
        items.push(item);
    }
    items
}

/// Declared dependencies of a pyproject.toml (shared with uv).
pub(super) fn pyproject_items(file: &FoundFile<'_>, ecosystem: &str) -> AdapterResult {
    let table = toml_table(file.content).map_err(|e| e.to_string())?;
    let project = table.get("project").and_then(|v| v.as_table());
    let tool = table.get("tool").and_then(|v| v.as_table());
    let private_index =
        tool.and_then(|t| t.get("uv")).and_then(|u| u.get("index")).and_then(|i| i.as_array()).is_some_and(|indexes| {
            indexes
                .iter()
                .filter_map(|i| i.get("url")?.as_str())
                .any(|url| classify_url(url, PUBLIC).0 != SourceClass::PublicRegistry)
        });
    let mut declared: Vec<(String, String, &str)> = Vec::new();
    let mut push_list = |values: Option<&toml::Value>, group: &'static str| {
        for value in values.and_then(|v| v.as_array()).into_iter().flatten() {
            if let Some((name, spec)) = value.as_str().and_then(requirement) {
                declared.push((name, spec, group));
            }
        }
    };
    push_list(project.and_then(|p| p.get("dependencies")), "");
    for groups in
        [project.and_then(|p| p.get("optional-dependencies")), table.get("dependency-groups")].into_iter().flatten()
    {
        for (_, list) in groups.as_table().into_iter().flatten() {
            push_list(Some(list), " · optional/Gruppe");
        }
    }
    push_list(table.get("build-system").and_then(|b| b.get("requires")), " · Build");
    if let Some(poetry) =
        tool.and_then(|t| t.get("poetry")).and_then(|p| p.get("dependencies")).and_then(|d| d.as_table())
    {
        for (name, spec) in poetry {
            if name != "python" {
                declared.push((normalize(name), spec.as_str().unwrap_or("Tabelle").to_string(), " · Poetry"));
            }
        }
    }
    let name = project.and_then(|p| p.get("name")).and_then(|n| n.as_str()).unwrap_or("ohne Namen");
    let mut detail = format!("Projekt {name} · {} Abhängigkeiten", declared.len());
    if private_index {
        detail.push_str(" · private Indexquelle konfiguriert");
    }
    let mut items = vec![manifest(ecosystem, file, detail)];
    let source = if private_index { SourceClass::PrivateRegistry } else { SourceClass::Unknown };
    for (name, spec, group) in declared {
        items.push(
            InventoryItem::new(ItemCategory::Package, ecosystem, name, file.rel_path())
                .state(InstallState::Declared)
                .source(source, None)
                .detail(format!("Spezifikation {}{group}", if spec.is_empty() { "–" } else { &spec })),
        );
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    #[test]
    fn requirements_keep_private_indexes_private() {
        let text = "--index-url https://user:token@pypi.corp.example/simple\nRequests[security]==2.31.0 ; python_version>'3'\n-r other.txt\nflask>=2\n";
        let items = Pip.parse_file(&file("requirements.txt", text)).unwrap();
        let requests = items.iter().find(|i| i.name == "requests").unwrap();
        assert_eq!(requests.version.as_deref(), Some("2.31.0"));
        assert_eq!(requests.source, SourceClass::PrivateRegistry);
        assert!(!format!("{items:?}").contains("token"));
    }

    #[test]
    fn pyproject_declares_without_resolving() {
        let text = "[project]\nname='demo'\ndependencies=['Django>=5', 'typing_extensions']\n[dependency-groups]\ndev=['pytest']\n";
        let items = Pip.parse_file(&file("pyproject.toml", text)).unwrap();
        assert!(items.iter().any(|i| i.name == "typing-extensions" && i.install_state == Some(InstallState::Declared)));
        assert!(items.iter().any(|i| i.name == "pytest"));
    }

    #[test]
    fn venv_is_recognised_by_marker_not_name() {
        assert!(Pip.claim_dir("env", &[], &["pyvenv.cfg".into()]).is_some());
        assert!(Pip.claim_dir(".venv", &[], &["notes.txt".into()]).is_none());
        let mut dir = FakeDir::default();
        dir.files.insert("pyvenv.cfg".into(), "home = C:\\Python312\nversion = 3.12.1\n".into());
        dir.dirs.insert(String::new(), vec![("Lib".into(), true)]);
        dir.dirs.insert("Lib".into(), vec![("site-packages".into(), true)]);
        dir.dirs.insert("Lib/site-packages".into(), vec![("requests-2.31.0.dist-info".into(), true)]);
        let claim = Pip.claim_dir(".venv", &[], &["pyvenv.cfg".into()]).unwrap();
        let items = Pip.inspect_dir(claim, ".venv", &mut dir).unwrap();
        assert!(items[0].detail.contains("3.12.1"));
        assert!(!items[0].detail.contains("Python312"), "home path must not be stored");
        assert!(items.iter().any(|i| i.name == "requests" && i.install_state == Some(InstallState::Installed)));
    }
}
