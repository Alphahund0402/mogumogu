//! Conda: environment.yml/.yaml (declared; the nested `pip:` list as PyPI
//! requirements), conda-lock.yml (resolved, unified lockfile v1) and
//! environments inside a project, recognised by their `conda-meta` folder
//! (installed, from the record file names; the JSON records are not
//! opened). No conda, mamba or Python runs and `.condarc` is not read, so a
//! channel that is neither a known public name nor a URL stays unknown.
//! `prefix` and `variables` of an environment file are never kept.
use super::pip::{self, Declared, Source};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::lines;

pub struct Conda;

const PUBLIC: &[&str] = &["conda.anaconda.org", "repo.anaconda.com", "anaconda.org"];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "conda",
    name: "Conda",
    markers: &["environment.yml", "environment.yaml", "*conda-lock.yml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "environment.yml/.yaml (channels, dependencies, pip)",
        "conda-lock.yml (Lockfile v1)",
        "Umgebung: conda-meta/*.json (Dateinamen)",
    ],
    limits: "Kein conda-/mamba-Aufruf; .condarc wird nicht gelesen – Kanäle ohne bekannten Namen oder URL bleiben unbekannt; conda-meta-Datensätze werden nicht geöffnet, explizite Lockfiles (@EXPLICIT) nicht gelesen.",
};

impl Adapter for Conda {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let lower = file.name.to_ascii_lowercase();
        if lower == "conda-lock.yml" || lower.ends_with(".conda-lock.yml") {
            return bounded(lock_items(file)?);
        }
        if lower == "environment.yml" || lower == "environment.yaml" {
            return bounded(environment(file));
        }
        Ok(Vec::new())
    }

    fn claim_dir(&self, _name: &str, _siblings: &[String], children: &[String]) -> Option<DirClaim> {
        children.iter().any(|c| c.eq_ignore_ascii_case("conda-meta")).then_some(DirClaim {
            adapter: "conda",
            category: ItemCategory::Environment,
            label: "Conda-Umgebung",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let records = format!("{rel_path}/conda-meta");
        let mut python = None;
        let mut items = Vec::new();
        for (entry, is_dir) in access.list(&["conda-meta"]) {
            let stem = entry.strip_suffix(".json").filter(|_| !is_dir);
            let Some((name, version)) = stem.and_then(record_name) else { continue };
            if name.eq_ignore_ascii_case("python") {
                python = Some(version.to_string());
            }
            items.push(
                InventoryItem::new(ItemCategory::Package, "conda", name.to_ascii_lowercase(), records.as_str())
                    .version(version)
                    .state(InstallState::Installed)
                    .detail("Installationsdatensatz in conda-meta (nur Dateiname gelesen)"),
            );
        }
        let conda = items.len();
        // pip and uv leave an INSTALLER file; conda's own Python packages name conda.
        for site in pip::site_packages(access) {
            let path: Vec<&str> = site.iter().map(String::as_str).collect();
            for (entry, is_dir) in access.list(&path) {
                let stem = entry.strip_suffix(".dist-info").filter(|_| is_dir);
                let Some((name, version)) = stem.and_then(|s| s.split_once('-')) else { continue };
                let mut installer_path = path.clone();
                installer_path.extend([entry.as_str(), "INSTALLER"]);
                let installer = access.read_text(&installer_path, 256).unwrap_or_default();
                let installer = installer.trim();
                if installer.is_empty() || installer.eq_ignore_ascii_case("conda") {
                    continue;
                }
                let tool = if installer.len() <= 32
                    && installer.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                {
                    installer
                } else {
                    "unbekanntem Werkzeug"
                };
                let mut item = InventoryItem::new(
                    ItemCategory::Package,
                    "pip",
                    pip::normalize(name),
                    format!("{rel_path}/{}", site.join("/")),
                )
                .version(version)
                .state(InstallState::Installed)
                .detail(format!("mit {tool} in die Conda-Umgebung installiert (dist-info); nicht importiert"));
                item.evidence.context = "conda-pip".into();
                items.push(item);
            }
        }
        let detail = format!(
            "Python {} · {conda} Conda-Pakete · {} pip-Pakete · kein Interpreterstart",
            python.as_deref().unwrap_or("unbekannt"),
            items.len() - conda
        );
        items.insert(
            0,
            InventoryItem::new(ItemCategory::Environment, "conda", "Conda-Umgebung", rel_path).detail(detail),
        );
        bounded(items)
    }
}

/// `numpy-1.26.4-py311h64a7726_0`: conda versions and builds contain no `-`,
/// names may.
fn record_name(stem: &str) -> Option<(&str, &str)> {
    let (rest, build) = stem.rsplit_once('-')?;
    let (name, version) = rest.rsplit_once('-')?;
    (!name.is_empty() && !version.is_empty() && !build.is_empty()).then_some((name, version))
}

/// A channel name or URL. Tokens sit in channel URL paths (`/t/<token>/`):
/// only the host is kept, and a channel with a token counts as private.
fn channel(text: &str) -> Source {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let drive = text.as_bytes().get(1) == Some(&b':') && text.starts_with(|c: char| c.is_ascii_alphabetic());
    if lower.starts_with("file:") || text.starts_with(['.', '/', '\\', '~']) || drive {
        return (SourceClass::Path, None);
    }
    if let Some((_, rest)) = text.split_once("://") {
        let (class, host) = classify_url(text, PUBLIC);
        let token = rest.split('/').skip(1).any(|segment| segment == "t");
        return match class {
            SourceClass::PublicRegistry if token => (SourceClass::PrivateRegistry, host),
            _ => (class, host),
        };
    }
    match lower.split('/').next().unwrap_or_default() {
        "conda-forge" | "bioconda" => (SourceClass::PublicRegistry, Some("conda.anaconda.org".into())),
        "defaults" | "main" | "r" | "msys2" | "pkgs" => (SourceClass::PublicRegistry, Some("repo.anaconda.com".into())),
        _ => pip::UNKNOWN,
    }
}

/// A channel or environment name, not a URL or path.
fn plain_name(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with(['/', '.'])
        && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// `[channel::]name[constraint]` with constraints such as `=1.2`, `==1.2`,
/// `>=1.2,<2`, `=1.2=build`, ` 1.2 build` or `[version='>=1.2']`.
fn match_spec(text: &str) -> Option<(Option<&str>, String, String)> {
    let (channel, rest) = match text.split_once("::") {
        Some((channel, rest)) => (Some(channel.trim()), rest.trim()),
        None => (None, text.trim()),
    };
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))).unwrap_or(rest.len());
    let name = &rest[..end];
    (!name.is_empty()).then(|| (channel, name.to_ascii_lowercase(), rest[end..].trim().to_string()))
}

/// `==1.2`, `=1.2=build` and `1.2 build` pin one version; `=1.2` means 1.2.*.
fn exact(constraint: &str) -> Option<String> {
    if let Some(version) = pip::pinned(constraint) {
        return Some(version.to_string());
    }
    let (version, build) = match constraint.strip_prefix('=') {
        Some(rest) => rest.split_once('=')?,
        None => constraint.split_once(' ')?,
    };
    let plain = |s: &str| !s.is_empty() && !s.contains(['*', '<', '>', '!', '|', ',', '=', ' ', '[']);
    let (version, build) = (version.trim(), build.trim());
    (plain(version) && plain(build) && version.starts_with(|c: char| c.is_ascii_digit())).then(|| version.to_string())
}

/// An environment file. Without a `dependencies` list it belongs to some
/// other tool and yields nothing.
fn environment(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let (mut name, mut listed) = ("", false);
    let (mut channels, mut specs, mut pip_lines) = (Vec::new(), Vec::new(), Vec::new());
    let (mut section, mut item_indent, mut in_pip) = ("", None, false);
    for entry in lines::yaml_entries(file.content).take(200_000) {
        if entry.indent == 0 && !entry.item {
            section = entry.key.unwrap_or("");
            (item_indent, in_pip) = (None, false);
            let flow = lines::flow_list(entry.value).unwrap_or_default();
            match section {
                "name" => name = entry.value,
                "channels" => channels.extend(flow),
                "dependencies" => {
                    listed = true;
                    specs.extend(flow);
                }
                _ => {}
            }
            continue;
        }
        if !entry.item {
            continue;
        }
        let indent = *item_indent.get_or_insert(entry.indent);
        match section {
            "channels" if entry.indent == indent && entry.key.is_none() => channels.push(entry.value),
            "dependencies" if entry.indent == indent => {
                in_pip = entry.key == Some("pip");
                match entry.key {
                    None => specs.push(entry.value),
                    Some("pip") => pip_lines.extend(lines::flow_list(entry.value).unwrap_or_default()),
                    Some(_) => {}
                }
            }
            "dependencies" if in_pip && entry.indent > indent && entry.key.is_none() => pip_lines.push(entry.value),
            _ => {}
        }
    }
    if !listed {
        return Vec::new();
    }
    let sources: Vec<Source> =
        channels.iter().filter(|c| !c.eq_ignore_ascii_case("nodefaults")).take(100).map(|c| channel(c)).collect();
    let default = pip::combine(&sources);
    let mut python = None;
    let mut declared = Vec::new();
    for spec in specs.into_iter().take(20_000) {
        let Some((pinned_channel, name, constraint)) = match_spec(spec) else { continue };
        if name == "python" {
            python = Some(constraint); // a runtime pin, not a package of this view
            continue;
        }
        let mut entry = Declared::new(name, "");
        entry.version = exact(&constraint);
        entry.spec = constraint;
        if let Some(pinned) = pinned_channel {
            entry.source = Some(channel(pinned));
            if plain_name(pinned) {
                entry.note = format!(" · Kanal {pinned}");
            }
        }
        declared.push(entry);
    }
    let pip = pip::requirement_lines(pip_lines.into_iter(), "conda-pip");
    let pip_default = pip.index.clone().unwrap_or(pip::UNKNOWN);
    let mut detail = format!(
        "Conda-Umgebung {} · {} Conda-Pakete · {} pip-Pakete · {} Kanäle",
        if plain_name(name) { name } else { "ohne Namen" },
        declared.len(),
        pip.declared.len(),
        sources.len()
    );
    if let Some(python) = python {
        detail.push_str(&format!(" · Python {}", python.trim_start_matches('=')));
    }
    if sources.iter().any(|(class, _)| *class == SourceClass::PrivateRegistry) {
        detail.push_str(" · privater Kanal");
    }
    let mut items = vec![manifest("conda", file, detail)];
    items.extend(declared.into_iter().map(|d| d.item(file, "conda", &default)));
    items.extend(pip.declared.into_iter().map(|d| d.item(file, "pip", &pip_default)));
    pip::keep_repeated(&mut items);
    items
}

/// One package of a conda-lock file while its fields are read.
#[derive(Default)]
struct Locked<'a> {
    name: &'a str,
    version: &'a str,
    manager: &'a str,
    platform: &'a str,
    url: &'a str,
    category: &'a str,
    optional: bool,
    dependencies: Vec<(&'a str, &'a str)>,
}

/// conda-lock.yml: one entry per package, platform and category; packages
/// solved by pip keep their PyPI identity.
fn lock_items(file: &FoundFile<'_>) -> AdapterResult {
    let (mut version, mut seen_packages) = (None, false);
    let mut platforms = Vec::new();
    let mut packages: Vec<Locked<'_>> = Vec::new();
    let (mut section, mut level, mut nested) = ("", None, "");
    for entry in lines::yaml_entries(file.content) {
        if entry.indent == 0 && !entry.item {
            section = entry.key.unwrap_or("");
            (level, nested) = (None, "");
            match section {
                "version" => version = Some(entry.value),
                "package" => seen_packages = true,
                _ => {}
            }
            continue;
        }
        if section == "metadata" {
            match entry.key {
                Some(key) if !entry.item => {
                    nested = key;
                    if key == "platforms" {
                        platforms.extend(lines::flow_list(entry.value).unwrap_or_default());
                    }
                }
                None if entry.item && nested == "platforms" && platforms.len() < 100 => platforms.push(entry.value),
                _ => {}
            }
            continue;
        }
        if section != "package" {
            continue;
        }
        let level = *level.get_or_insert(entry.indent);
        if entry.item && entry.indent == level {
            if packages.len() > crate::limits::ITEMS_PER_FILE {
                break; // `bounded` reports the overflow
            }
            packages.push(Locked::default());
            nested = "";
        }
        let (Some(package), Some(key)) = (packages.last_mut(), entry.key) else { continue };
        if entry.indent == level {
            nested = if entry.value.is_empty() { key } else { "" };
            match key {
                "name" => package.name = entry.value,
                "version" => package.version = entry.value,
                "manager" => package.manager = entry.value,
                "platform" => package.platform = entry.value,
                "url" => package.url = entry.value,
                "category" => package.category = entry.value,
                "optional" => package.optional = entry.value == "true",
                _ => {}
            }
        } else if nested == "dependencies" && entry.indent > level && package.dependencies.len() < 10_000 {
            package.dependencies.push((key, entry.value));
        }
    }
    if version.is_none() && !seen_packages {
        return Err("conda-lock-Datei ohne version und package".into());
    }
    let mut detail = format!("Lockfile v{}", version.unwrap_or("?"));
    if !platforms.is_empty() {
        detail.push_str(&format!(" · Plattformen {}", platforms.join(", ")));
    }
    let mut items = vec![manifest("conda", file, detail)];
    for package in packages.into_iter().filter(|p| !p.name.is_empty()) {
        let pip = package.manager == "pip";
        let identity = |name: &str| if pip { pip::normalize(name) } else { name.to_ascii_lowercase() };
        let (class, host) = match (pip, package.url.is_empty()) {
            (_, true) => pip::UNKNOWN,
            (true, false) => classify_url(package.url, pip::PUBLIC),
            (false, false) => channel(package.url),
        };
        let mut detail = format!("aufgelöst laut {} · {}", file.name, package.platform);
        if package.optional {
            detail.push_str(" · optional");
        }
        let mut item = InventoryItem::new(
            ItemCategory::Package,
            if pip { "pip" } else { "conda" },
            identity(package.name),
            file.rel_path(),
        )
        .version(package.version)
        .state(InstallState::Resolved)
        .source(class, host)
        .detail(detail);
        item.evidence.context = package.category.to_string();
        item.evidence.qualifier = package.platform.to_string();
        item.evidence.dependencies = package
            .dependencies
            .iter()
            .map(|(name, requirement)| PackageDependency {
                name: identity(name),
                requirement: pip::kept_spec(requirement.to_string()),
                kind: "lock".into(),
                exact: false,
            })
            .collect();
        items.push(item);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const ENVIRONMENT: &str = "\
name: analysis
channels:
  - conda-forge
  - https://conda.anaconda.org/t/tk-0123456789abcdef/corp-channel
  - nodefaults
dependencies:
  - python=3.11
  - numpy>=1.26
  - pandas=2.2.2=py311h8f0b8f4_0
  - conda-forge::scipy==1.13.1
  - r-base 4.3.3 hd8ed1ab_0
  - pip
  - pip:
    - requests==2.32.3
    - --extra-index-url https://user:hunter2@pkgs.corp.example/simple
    - corp-tool>=1.0
    - -e .
variables:
  API_TOKEN: do-not-store
prefix: C:\\Users\\someone\\miniconda3\\envs\\analysis
";

    #[test]
    fn environment_file_with_channels_and_pip_section() {
        let items = Conda.parse_file(&file("environment.yml", ENVIRONMENT)).unwrap();
        assert_eq!(
            items[0].detail,
            "Conda-Umgebung analysis · 5 Conda-Pakete · 2 pip-Pakete · 2 Kanäle · Python 3.11 · privater Kanal"
        );
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let numpy = find("numpy");
        assert_eq!((numpy.ecosystem.as_str(), numpy.install_state), ("conda", Some(InstallState::Declared)));
        assert_eq!(numpy.source, SourceClass::PrivateRegistry, "a token channel may serve any package");
        assert_eq!(numpy.detail, "Spezifikation >=1.26");
        assert_eq!(find("pandas").version.as_deref(), Some("2.2.2"));
        assert_eq!(find("r-base").version.as_deref(), Some("4.3.3"));
        let scipy = find("scipy");
        assert_eq!(
            (scipy.source, scipy.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("conda.anaconda.org"))
        );
        assert_eq!(scipy.version.as_deref(), Some("1.13.1"));
        assert!(find("pip").ecosystem == "conda");
        let requests = items.iter().find(|i| i.name == "requests").unwrap();
        assert_eq!((requests.ecosystem.as_str(), requests.evidence.context.as_str()), ("pip", "conda-pip"));
        assert_eq!(requests.source, SourceClass::PrivateRegistry);
        assert!(items.iter().all(|i| i.name != "python"), "the Python pin is a runtime pin");
        let text = format!("{items:?}");
        for secret in ["tk-0123456789abcdef", "hunter2", "do-not-store", "someone", "corp-channel"] {
            assert!(!text.contains(secret), "{secret} leaked");
        }
    }

    #[test]
    fn foreign_and_flow_environment_files() {
        let compose = "services:\n  web:\n    image: nginx\nenvironment:\n  - FOO=bar\n";
        assert!(Conda.parse_file(&file("environment.yml", compose)).unwrap().is_empty());
        let flow = "name: tiny\nchannels: [defaults]\ndependencies: [numpy, 'scipy>=1.11']\n";
        let items = Conda.parse_file(&file("environment.yaml", flow)).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].source_host.as_deref(), Some("repo.anaconda.com"));
        let unnamed = "dependencies:\n- numpy\n- mycorp::internal-lib\n";
        let items = Conda.parse_file(&file("environment.yml", unnamed)).unwrap();
        assert_eq!(items[1].source, SourceClass::Unknown, "without channels the .condarc decides");
        let internal = items.iter().find(|i| i.name == "internal-lib").unwrap();
        assert_eq!(internal.source, SourceClass::Unknown, "unknown named channel");
        assert!(internal.detail.ends_with("Kanal mycorp"));
        let local = "name: C:\\Users\\someone\\envs\\x\ndependencies:\n  - C:\\Users\\someone\\channel::tool\n";
        let items = Conda.parse_file(&file("environment.yml", local)).unwrap();
        assert!(items[0].detail.starts_with("Conda-Umgebung ohne Namen"));
        assert_eq!(items[1].source, SourceClass::Path);
        assert!(!format!("{items:?}").contains("someone"), "no local paths are kept");
    }

    const LOCK: &str = "\
version: 1
metadata:
  content_hash:
    linux-64: 4c0bc5a8a2c1d6e9
    win-64: 8d2a6d0f1b3c4e5a
  channels:
  - url: conda-forge
    used_env_vars: []
  platforms:
  - linux-64
  - win-64
  sources:
  - environment.yml
package:
- name: python
  version: 3.11.9
  manager: conda
  platform: linux-64
  dependencies:
    bzip2: '>=1.0.8,<2.0a0'
    libffi: '>=3.4,<4.0a0'
  url: https://conda.anaconda.org/conda-forge/linux-64/python-3.11.9-hb806964_0_cpython.conda
  hash:
    md5: ac68acfa8b558ed406c75e98d3428d7b
    sha256: 177f33a1fb8d3476b38f73c37b42f01c0b014fa0e039a701fd9f83d83aefb4e0
  category: main
  optional: false
- name: python
  version: 3.11.9
  manager: conda
  platform: win-64
  dependencies: {}
  url: https://conda.anaconda.org/t/tk-secret-123/conda-forge/win-64/python-3.11.9-h631f459_0_cpython.conda
  hash:
    md5: 23b2ec0a2ac0f81f4b73cc1d2c0ee8f8
  category: main
  optional: false
- name: Requests
  version: 2.32.3
  manager: pip
  platform: linux-64
  dependencies:
    certifi: '>=2017.4.17'
  url: https://files.pythonhosted.org/packages/f9/9b/requests-2.32.3-py3-none-any.whl
  hash:
    sha256: 70761cfe03c773ceb22aa2f671b4757976145175cdfca038c02654d061d6dcc6
  category: dev
  optional: true
";

    #[test]
    fn unified_lockfile_per_platform_and_manager() {
        let items = Conda.parse_file(&file("conda-lock.yml", LOCK)).unwrap();
        assert_eq!(items[0].detail, "Lockfile v1 · Plattformen linux-64, win-64");
        assert_eq!(items.len(), 4);
        let linux = &items[1];
        assert_eq!((linux.ecosystem.as_str(), linux.evidence.qualifier.as_str()), ("conda", "linux-64"));
        assert_eq!(
            (linux.source, linux.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("conda.anaconda.org"))
        );
        assert_eq!(linux.evidence.dependencies.len(), 2);
        assert_eq!(linux.evidence.context, "main");
        assert_eq!(items[2].source, SourceClass::PrivateRegistry, "a token channel is private");
        assert_ne!(items[1].key(), items[2].key());
        let requests = &items[3];
        assert_eq!((requests.ecosystem.as_str(), requests.name.as_str()), ("pip", "requests"));
        assert_eq!((requests.source, requests.evidence.context.as_str()), (SourceClass::PublicRegistry, "dev"));
        assert!(requests.detail.ends_with("optional"));
        assert!(!format!("{items:?}").contains("tk-secret-123"));
        assert!(Conda.parse_file(&file("conda-lock.yml", "just: text\n")).is_err());
        assert_eq!(Conda.parse_file(&file("prod.conda-lock.yml", LOCK)).unwrap().len(), 4);
    }

    #[test]
    fn environment_folder_is_claimed_by_conda_meta_and_read_by_names() {
        assert!(Conda.claim_dir("env", &[], &["conda-meta".into(), "Lib".into()]).is_some());
        assert!(Conda.claim_dir("env", &["environment.yml".into()], &["Lib".into()]).is_none());
        let mut dir = FakeDir::default();
        dir.dirs.insert(
            "conda-meta".into(),
            vec![
                ("python-3.11.9-h631f459_0_cpython.json".into(), false),
                ("python-dateutil-2.9.0.post0-pyhd8ed1ab_0.json".into(), false),
                ("_libgcc_mutex-0.1-conda_forge.json".into(), false),
                ("history".into(), false),
                ("broken.json".into(), false),
            ],
        );
        dir.dirs.insert(String::new(), vec![("Lib".into(), true), ("conda-meta".into(), true)]);
        dir.dirs.insert("Lib".into(), vec![("site-packages".into(), true)]);
        dir.dirs.insert(
            "Lib/site-packages".into(),
            vec![("six-1.16.0.dist-info".into(), true), ("corp_tool-2.0.0.dist-info".into(), true)],
        );
        dir.files.insert("Lib/site-packages/six-1.16.0.dist-info/INSTALLER".into(), "conda\n".into());
        dir.files.insert("Lib/site-packages/corp_tool-2.0.0.dist-info/INSTALLER".into(), "pip\n".into());
        let claim = Conda.claim_dir("env", &[], &["conda-meta".into()]).unwrap();
        let items = Conda.inspect_dir(claim, "env", &mut dir).unwrap();
        assert_eq!(items[0].detail, "Python 3.11.9 · 3 Conda-Pakete · 1 pip-Pakete · kein Interpreterstart");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let dateutil = find("python-dateutil");
        assert_eq!(
            (dateutil.version.as_deref(), dateutil.install_state),
            (Some("2.9.0.post0"), Some(InstallState::Installed))
        );
        assert_eq!(find("_libgcc_mutex").version.as_deref(), Some("0.1"));
        assert_eq!(find("python").rel_path, "env/conda-meta");
        let tool = find("corp-tool");
        assert_eq!((tool.ecosystem.as_str(), tool.evidence.context.as_str()), ("pip", "conda-pip"));
        assert!(items.iter().all(|i| i.name != "six"), "conda's own Python packages are not counted twice");
    }
}
