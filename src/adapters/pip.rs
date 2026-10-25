//! pip / Python: requirements and constraints files (pip-tools inputs and
//! compiled output included), setup.cfg and pyproject.toml (declared) plus
//! virtual environments recognised by `pyvenv.cfg` (installed, from
//! `*.dist-info` folder names). No interpreter is ever started: Python
//! start-up would execute `.pth` lines (PROJEKTPLAN §7.2), and setup.py is
//! code, so it is not read. The other Python adapters share the helpers
//! here; [`python_manager`] decides which of them reports a pyproject.toml.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::{lines, toml_table};
use std::collections::HashMap;

pub struct Pip;

pub(super) const PUBLIC: &[&str] = &["pypi.org", "files.pythonhosted.org"];

/// A source class and the registry host, when one is known.
pub(super) type Source = (SourceClass, Option<String>);

pub(super) const UNKNOWN: Source = (SourceClass::Unknown, None);

static DESCRIPTOR: Descriptor = Descriptor {
    id: "pip",
    name: "pip",
    markers: &["requirements*.txt", "requirements*.in", "constraints*.txt", "setup.cfg", "pyproject.toml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "requirements*.txt, requirements*.in, constraints*.txt (Ausgabe von pip-compile/uv als aufgelöst)",
        "setup.cfg ([options], [options.extras_require])",
        "pyproject.toml ohne Lockfile eines Managers (PEP 621, dependency-groups, Hatch-Umgebungen)",
        "venv: pyvenv.cfg + *.dist-info",
    ],
    limits: "Keine Interpreterstarts; setup.py wird nicht ausgewertet, pip.conf außerhalb des Projekts nicht gelesen – Quellen bleiben ohne Indexangabe unbekannt; Verweise (-r, -c, file:) werden nicht verfolgt, Marker nicht ausgewertet.",
};

impl Adapter for Pip {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("pyproject.toml") {
            return match owned_pyproject(file, "pip")? {
                Some(table) => bounded(pyproject_items(file, &table)),
                None => Ok(Vec::new()),
            };
        }
        if file.name.eq_ignore_ascii_case("setup.cfg") {
            return bounded(setup_cfg(file));
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
            packages.extend(installed_packages(access, &path, rel_path, ecosystem));
        }
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, ecosystem, "Python-Umgebung", rel_path)
                .detail(format!("Python {python} · {} Pakete · kein Interpreterstart", packages.len())),
        ];
        items.extend(packages);
        bounded(items)
    }
}

/// Selected installed-distribution metadata only. RECORD contents are counted,
/// never followed: they can name paths outside the approved environment.
pub(super) fn installed_packages(
    access: &mut dyn DirAccess,
    site: &[&str],
    rel: &str,
    ecosystem: &str,
) -> Vec<InventoryItem> {
    let mut packages = Vec::new();
    for (entry, is_dir) in access.list(site) {
        let Some(stem) = entry.strip_suffix(".dist-info").filter(|_| is_dir) else { continue };
        let Some((name, version)) = stem.split_once('-') else { continue };
        let mut item = InventoryItem::new(ItemCategory::Package, ecosystem, normalize(name), rel)
            .version(version)
            .state(InstallState::Installed)
            .detail("Installationsmetadaten (dist-info); nicht importiert");
        let mut path = site.to_vec();
        path.push(&entry);
        path.push("METADATA");
        if let Some(text) = access.read_text(&path, 1 << 20) {
            // Metadata headers end at the first blank line. Description text
            // and executable .pth content are never interpreted.
            for line in text.lines().take_while(|l| !l.is_empty()).take(20_000) {
                if let Some(raw) = line.strip_prefix("Requires-Dist:")
                    && let Some((name, spec)) = requirement(raw.trim())
                {
                    item.evidence.dependencies.push(crate::domain::PackageDependency {
                        name,
                        requirement: kept_spec(spec),
                        kind: "Requires-Dist (Bedingungen ungeprüft)".into(),
                        exact: false,
                    });
                }
            }
        }
        *path.last_mut().expect("RECORD filename") = "RECORD";
        if let Some(text) = access.read_text(&path, 1 << 20) {
            item.evidence.owned_files = Some(text.lines().filter(|l| !l.trim().is_empty()).count() as u64);
        }
        item.evidence.qualifier = site.iter().copied().chain([entry.as_str()]).collect::<Vec<_>>().join("/");
        packages.push(item);
    }
    packages
}

/// Windows (`Lib/site-packages`) and POSIX (`lib/pythonX.Y/site-packages`).
pub(super) fn site_packages(access: &mut dyn DirAccess) -> Vec<Vec<String>> {
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
    if !name.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    let rest = text[end..].trim();
    let rest = rest.strip_prefix('[').and_then(|r| r.split_once(']')).map_or(rest, |(_, r)| r.trim());
    // `name (>=1.0)`: the older parenthesised form, still written by Poetry.
    let rest = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')).map_or(rest, str::trim);
    // A specifier starts with an operator or `@`: `token: abc` in a foreign
    // text file is no requirement and must not become name plus value.
    if !rest.is_empty() && !rest.starts_with(['<', '>', '=', '!', '~', '@']) {
        return None;
    }
    Some((normalize(name), rest.to_string()))
}

/// A specifier fit to keep in an edge: a direct reference keeps no URL.
pub(super) fn kept_spec(spec: String) -> String {
    if spec.starts_with('@') { "Direktverweis".into() } else { spec }
}

/// The version of an exact pin such as `==1.2.3`.
pub(super) fn pinned(spec: &str) -> Option<&str> {
    let version = spec.strip_prefix("===").or_else(|| spec.strip_prefix("=="))?.trim();
    (!version.is_empty() && !version.contains(['*', ',', ' ', ';', '|'])).then_some(version)
}

/// Source of a direct reference (`name @ url`, `-e path`, a URL line). Only
/// the class and a registry host are kept, never the URL or path.
pub(super) fn direct_source(target: &str) -> Source {
    let target = target.trim();
    let lower = target.to_ascii_lowercase();
    if ["git+", "hg+", "svn+", "bzr+", "git@", "git://"].iter().any(|prefix| lower.starts_with(prefix)) {
        (SourceClass::Git, None)
    } else if lower.contains("://") && !lower.starts_with("file:") {
        classify_url(target, PUBLIC)
    } else {
        (SourceClass::Path, None)
    }
}

/// An index or find-links URL.
pub(super) fn index_source(url: &str) -> Source {
    classify_url(url.trim(), PUBLIC)
}

/// The source of requirements without a pin, from the indexes that serve
/// them: one private index makes them private, several public hosts keep
/// the class without a host, and no index at all stays unknown.
pub(super) fn combine(candidates: &[Source]) -> Source {
    if let Some(private) = candidates.iter().find(|(class, _)| *class == SourceClass::PrivateRegistry) {
        return private.clone();
    }
    let Some((class, host)) = candidates.first() else { return UNKNOWN };
    if candidates.iter().any(|(other, _)| other != class) {
        return UNKNOWN;
    }
    let host = host.clone().filter(|h| candidates.iter().all(|(_, other)| other.as_deref() == Some(h.as_str())));
    (*class, host)
}

/// Package indexes a project file configures, classified; URLs are dropped.
#[derive(Debug, Default)]
pub(super) struct Indexes {
    /// Indexes by name, for requirements pinned to one.
    pub named: Vec<(String, Source)>,
    /// Indexes that serve requirements without a pin.
    pub unpinned: Vec<Source>,
}

impl Indexes {
    pub fn named(&self, name: &str) -> Source {
        self.named.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map_or(UNKNOWN, |(_, source)| source.clone())
    }

    pub fn default_source(&self) -> Source {
        combine(&self.unpinned)
    }
}

/// One declared requirement before it becomes an inventory item.
#[derive(Clone, Debug)]
pub(super) struct Declared {
    pub name: String,
    /// Version constraint, or the kind of a direct source (`Git`, `Pfad`, `URL`).
    pub spec: String,
    pub context: String,
    /// A direct reference or pinned index; `None` takes the file's default.
    pub source: Option<Source>,
    pub version: Option<String>,
    /// Detail suffix such as ` · Quelle corp`.
    pub note: String,
}

impl Declared {
    pub fn new(name: String, context: &str) -> Self {
        Self { name, spec: String::new(), context: context.into(), source: None, version: None, note: String::new() }
    }

    pub fn with_source(mut self, source: Source) -> Self {
        self.spec = match source.0 {
            SourceClass::Git => "Git",
            SourceClass::Path => "Pfad",
            _ => "URL",
        }
        .into();
        self.source = Some(source);
        self
    }

    pub fn item(self, file: &FoundFile<'_>, ecosystem: &str, default: &Source) -> InventoryItem {
        let (source, host) = self.source.unwrap_or_else(|| default.clone());
        let mut detail = if self.spec.is_empty() {
            "ohne Versionsangabe".to_string()
        } else {
            format!("Spezifikation {}", self.spec)
        };
        if !self.context.is_empty() {
            detail.push_str(" · ");
            detail.push_str(&context_label(&self.context));
        }
        detail.push_str(&self.note);
        let mut item = InventoryItem::new(ItemCategory::Package, ecosystem, self.name, file.rel_path())
            .version(self.version.unwrap_or_default())
            .state(InstallState::Declared)
            .source(source, host)
            .detail(detail);
        item.evidence.context = self.context;
        item
    }
}

fn context_label(context: &str) -> String {
    if let Some(extra) = context.strip_prefix("extra:") {
        format!("Extra {extra}")
    } else if let Some(env) = context.strip_prefix("hatch:") {
        format!("Hatch-Umgebung {env}")
    } else {
        match context {
            "build" => "Build".into(),
            "constraint" => "nur Einschränkung (constraint)".into(),
            "conda-pip" => "pip in Conda-Umgebung".into(),
            other => other.into(),
        }
    }
}

/// A PEP 508 requirement, or a PDM-style `-e` line, as a declaration.
pub(super) fn pep508(text: &str, context: &str) -> Option<Declared> {
    let text = text.trim();
    if let Some(target) = text.strip_prefix("-e ").or_else(|| text.strip_prefix("--editable ")) {
        return editable(target.trim(), context);
    }
    let (name, spec) = requirement(text)?;
    let mut declared = Declared::new(name, context);
    match spec.strip_prefix('@') {
        Some(target) => declared = declared.with_source(direct_source(target)),
        None => {
            declared.version = pinned(&spec).map(str::to_string);
            declared.spec = spec;
        }
    }
    Some(declared)
}

/// An editable requirement; only an `#egg=` fragment names it.
fn editable(target: &str, context: &str) -> Option<Declared> {
    let mut declared = Declared::new(egg_name(target)?, context).with_source(direct_source(target));
    declared.note.push_str(" · editierbar");
    Some(declared)
}

fn egg_name(target: &str) -> Option<String> {
    let (_, fragment) = target.split_once('#')?;
    let value = fragment.split('&').find_map(|part| part.strip_prefix("egg="))?;
    requirement(value).map(|(name, _)| name)
}

/// Name and version of a wheel or sdist file named at the end of a URL or path.
fn archive_name(target: &str) -> Option<(String, String)> {
    let file = target.split(['?', '#']).next()?.rsplit(['/', '\\']).next()?;
    let lower = file.to_ascii_lowercase();
    let (name, version) = if lower.ends_with(".whl") {
        let mut parts = file.split('-');
        (parts.next()?, parts.next()?)
    } else {
        let stem = [".tar.gz", ".tar.bz2", ".tgz", ".zip"]
            .iter()
            .find_map(|ext| lower.ends_with(ext).then(|| &file[..file.len() - ext.len()]))?;
        stem.rsplit_once('-')?
    };
    let valid = requirement(name).is_some() && version.starts_with(|c: char| c.is_ascii_digit());
    valid.then(|| (normalize(name), version.to_string()))
}

/// The project itself (`-e .`, `.[dev]`).
fn is_self(target: &str) -> bool {
    target.trim_end_matches(['/', '\\']) == "." || target.starts_with(".[")
}

/// A URL or path instead of a named requirement.
fn is_target(word: &str) -> bool {
    let scheme = word
        .split_once("://")
        .is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+'));
    scheme
        || word.starts_with(['.', '/', '\\', '~'])
        || word.to_ascii_lowercase().starts_with("file:")
        || (word.as_bytes().get(1) == Some(&b':') && word.starts_with(|c: char| c.is_ascii_alphabetic()))
}

/// A line up to its comment: `#` at the start or after whitespace.
fn strip_comment(line: &str) -> &str {
    let mut previous = ' ';
    for (index, ch) in line.char_indices() {
        if ch == '#' && previous.is_whitespace() {
            return &line[..index];
        }
        previous = ch;
    }
    line
}

/// A requirement without its trailing options such as `--hash=…`.
fn before_options(line: &str) -> &str {
    let mut previous = 'x';
    for (index, ch) in line.char_indices() {
        if previous.is_whitespace() && line[index..].starts_with("--") {
            return line[..index].trim_end();
        }
        previous = ch;
    }
    line
}

/// Requirement lines of a requirements file or a conda `pip:` list.
#[derive(Debug, Default)]
pub(super) struct RequirementLines {
    pub declared: Vec<Declared>,
    /// Index of requirements without a direct reference, when one is named.
    pub index: Option<Source>,
    pub references: usize,
    pub unnamed: usize,
}

pub(super) fn requirement_lines<'a>(lines: impl Iterator<Item = &'a str>, context: &str) -> RequirementLines {
    let mut out = RequirementLines::default();
    for raw in lines.take(20_000) {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let line = before_options(line);
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or_default();
        let (option, inline) = match first.split_once('=') {
            Some((option, value)) if option.starts_with("--") => (option, Some(value)),
            _ => (first, None),
        };
        let value = inline.or_else(|| words.next());
        match option {
            "-i" | "--index-url" | "--extra-index-url" | "-f" | "--find-links" => {
                if let Some(url) = value {
                    let source = index_source(url);
                    if out.index.as_ref().is_none_or(|(class, _)| *class == SourceClass::PublicRegistry) {
                        out.index = Some(source);
                    }
                }
            }
            "-r" | "--requirement" | "-c" | "--constraint" => out.references += 1,
            "-e" | "--editable" => match value {
                Some(target) if is_self(target) => {}
                Some(target) => match editable(target, context) {
                    Some(declared) => out.declared.push(declared),
                    None => out.unnamed += 1,
                },
                None => {}
            },
            _ if option.starts_with('-') => {}
            _ if is_target(first) => {
                let named = egg_name(first).map(|name| (name, None));
                match named.or_else(|| archive_name(first).map(|(name, version)| (name, Some(version)))) {
                    Some((name, version)) => {
                        let mut declared = Declared::new(name, context).with_source(direct_source(first));
                        declared.version = version;
                        out.declared.push(declared);
                    }
                    None if is_self(first) => {}
                    None => out.unnamed += 1,
                }
            }
            _ => out.declared.extend(pep508(line, context)),
        }
    }
    out
}

fn requirements(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let joined = file.content.replace("\\\r\n", " ").replace("\\\n", " ");
    let lower = file.name.to_ascii_lowercase();
    let constraint = lower.starts_with("constraints");
    // pip-compile and uv (pip compile, export) mark their pinned output.
    let compiled = joined.lines().take(12).any(|line| {
        line.starts_with('#') && (line.contains("autogenerated by pip-compile") || line.contains("autogenerated by uv"))
    });
    let parsed = requirement_lines(joined.lines(), if constraint { "constraint" } else { "" });
    let kind = if compiled {
        "Festgelegte Requirements (pip-compile/uv)"
    } else if constraint {
        "Constraints"
    } else if lower.ends_with(".in") {
        "pip-tools-Eingabe"
    } else {
        "Requirements"
    };
    let mut detail =
        format!("{kind} · {} Einträge · {} Verweise (nicht gefolgt)", parsed.declared.len(), parsed.references);
    if parsed.unnamed > 0 {
        detail.push_str(&format!(" · {} Direktverweise ohne Paketnamen", parsed.unnamed));
    }
    let default = parsed.index.unwrap_or(UNKNOWN);
    let mut items = vec![manifest("pip", file, detail)];
    for declared in parsed.declared {
        let mut item = declared.item(file, "pip", &default);
        if compiled {
            item = item.state(InstallState::Resolved).detail(format!("aufgelöst laut {} (pip-compile/uv)", file.name));
        }
        items.push(item);
    }
    keep_repeated(&mut items);
    items
}

/// Marker variables: a single-line `list-semi` value splits a marker off.
const MARKER_NAMES: &[&str] = &[
    "python-version",
    "python-full-version",
    "sys-platform",
    "platform-system",
    "platform-machine",
    "platform-release",
    "platform-version",
    "platform-python-implementation",
    "implementation-name",
    "implementation-version",
    "os-name",
    "extra",
];

/// setup.cfg in INI style. One that only configures other tools (flake8,
/// pytest …) declares nothing and is no manifest.
fn setup_cfg(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut section = String::new();
    let (mut name, mut python) = (None, None);
    let mut values: Vec<(String, Vec<&str>)> = Vec::new();
    let mut open = false;
    for line in file.content.lines().take(20_000) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, raw)) = values.last_mut().filter(|_| open) {
                raw.push(trimmed);
            }
            continue;
        }
        open = false;
        if let Some(header) = trimmed.strip_prefix('[') {
            section = header.trim_end_matches(']').trim().to_ascii_lowercase();
            continue;
        }
        let Some(split) = trimmed.find(['=', ':']) else { continue };
        let (key, value) = (trimmed[..split].trim(), trimmed[split + 1..].trim());
        let context = match (section.as_str(), key.to_ascii_lowercase().replace('-', "_").as_str()) {
            ("metadata", "name") => {
                name = Some(value);
                continue;
            }
            ("options", "python_requires") => {
                python = Some(value);
                continue;
            }
            ("options", "install_requires") => String::new(),
            ("options", "setup_requires") => "build".into(),
            ("options", "tests_require") => "test".into(),
            ("options.extras_require", _) => format!("extra:{key}"),
            _ => continue,
        };
        values.push((context, vec![value]));
        open = true;
    }
    if values.is_empty() {
        return Vec::new();
    }
    let mut declared = Vec::new();
    let mut references = 0;
    for (context, raw) in values {
        // `list-semi`: one requirement per line, or `;`-separated on one line.
        let entries: Vec<&str> = if raw.len() > 1 { raw } else { raw[0].split(';').collect() };
        for entry in entries.into_iter().map(str::trim).filter(|e| !e.is_empty()) {
            if entry.starts_with("file:") {
                references += 1;
            } else {
                declared.extend(pep508(entry, &context).filter(|r| !MARKER_NAMES.contains(&r.name.as_str())));
            }
        }
    }
    let mut detail = format!(
        "setup.cfg · Projekt {} · {} Abhängigkeiten · {references} Verweise (nicht gefolgt)",
        name.unwrap_or("ohne Namen"),
        declared.len()
    );
    if let Some(python) = python {
        detail.push_str(&format!(" · Python {python}"));
    }
    let mut items = vec![manifest("pip", file, detail)];
    items.extend(declared.into_iter().map(|d| d.item(file, "pip", &UNKNOWN)));
    keep_repeated(&mut items);
    items
}

/// The one Python adapter that reports a pyproject.toml: the manager whose
/// lockfile lies beside it, Poetry for its own dependency tables, else pip.
pub(super) fn python_manager(siblings: &[String], table: Option<&toml::Table>) -> &'static str {
    let has = |name: &str| siblings.iter().any(|s| s == name);
    if has("uv.lock") {
        "uv"
    } else if has("poetry.lock") {
        "poetry"
    } else if has("pdm.lock") {
        "pdm"
    } else if table.is_some_and(super::poetry::declares_dependencies) {
        "poetry"
    } else {
        "pip"
    }
}

/// The parsed pyproject.toml when `manager` owns it. Only the owner reports
/// a syntax error, so a broken file counts once.
pub(super) fn owned_pyproject(file: &FoundFile<'_>, manager: &str) -> Result<Option<toml::Table>, String> {
    let table = toml_table(file.content);
    if python_manager(file.siblings, table.as_ref().ok()) != manager {
        return Ok(None);
    }
    table.map(Some).map_err(|e| e.to_string())
}

/// `[tool.<name>]` of a pyproject.toml.
pub(super) fn tool<'a>(table: &'a toml::Table, name: &str) -> Option<&'a toml::Table> {
    table.get("tool")?.get(name)?.as_table()
}

/// `[project] name`, or Poetry's own `[tool.poetry] name`.
pub(super) fn project_name(table: &toml::Table) -> Option<&str> {
    table.get("project").and_then(|p| p.get("name")).or_else(|| tool(table, "poetry")?.get("name"))?.as_str()
}

/// PEP 508 strings of a TOML array. Inline tables (`{include-group = …}`)
/// and the project itself (`own[extra]`) are skipped.
pub(super) fn strings(value: Option<&toml::Value>, context: &str, own: Option<&str>, out: &mut Vec<Declared>) {
    let own = own.map(normalize);
    let texts = value.and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str);
    for text in texts.take(20_000) {
        out.extend(pep508(text, context).filter(|d| own.as_ref() != Some(&d.name)));
    }
}

/// PEP 621 dependencies and extras, PEP 735 groups and build requirements.
pub(super) fn standard_declarations(table: &toml::Table) -> Vec<Declared> {
    let own = project_name(table);
    let project = table.get("project");
    let mut out = Vec::new();
    strings(project.and_then(|p| p.get("dependencies")), "", own, &mut out);
    let extras = project.and_then(|p| p.get("optional-dependencies")).and_then(toml::Value::as_table);
    for (extra, list) in extras.into_iter().flatten().take(1_000) {
        strings(Some(list), &format!("extra:{extra}"), own, &mut out);
    }
    for (group, list) in
        table.get("dependency-groups").and_then(toml::Value::as_table).into_iter().flatten().take(1_000)
    {
        strings(Some(list), group, own, &mut out);
    }
    strings(table.get("build-system").and_then(|b| b.get("requires")), "build", own, &mut out);
    out
}

/// Manifest and declared items of a pyproject.toml for its owner. Runtime
/// pins such as `requires-python` stay in the detail.
pub(super) fn pyproject_result(
    file: &FoundFile<'_>,
    ecosystem: &str,
    table: &toml::Table,
    python: Option<&str>,
    declared: Vec<Declared>,
    default: &Source,
) -> Vec<InventoryItem> {
    let python = python.or_else(|| table.get("project")?.get("requires-python")?.as_str());
    let mut detail =
        format!("Projekt {} · {} Abhängigkeiten", project_name(table).unwrap_or("ohne Namen"), declared.len());
    if let Some(python) = python {
        detail.push_str(&format!(" · Python {python}"));
    }
    if declared.iter().any(|d| d.source.as_ref().unwrap_or(default).0 == SourceClass::PrivateRegistry) {
        detail.push_str(" · private Quelle");
    }
    let mut items = vec![manifest(ecosystem, file, detail)];
    items.extend(declared.into_iter().map(|d| d.item(file, ecosystem, default)));
    keep_repeated(&mut items);
    items
}

/// A pyproject.toml no manager claims: standard tables and Hatch
/// environments. A private index configured for uv, PDM or Poetry still
/// marks unpinned requirements private.
fn pyproject_items(file: &FoundFile<'_>, table: &toml::Table) -> Vec<InventoryItem> {
    let mut declared = standard_declarations(table);
    let own = project_name(table);
    let envs = tool(table, "hatch").and_then(|h| h.get("envs")).and_then(toml::Value::as_table);
    for (env, config) in envs.into_iter().flatten().take(1_000) {
        for key in ["dependencies", "extra-dependencies"] {
            strings(config.get(key), &format!("hatch:{env}"), own, &mut declared);
        }
    }
    let mut candidates = super::uv::indexes(tool(table, "uv")).unpinned;
    candidates.extend(super::pdm::indexes(tool(table, "pdm")).unpinned);
    candidates.extend(super::poetry::indexes(tool(table, "poetry")).unpinned);
    let tools = combine(&candidates);
    let default = if tools.0 == SourceClass::PrivateRegistry { tools } else { UNKNOWN };
    pyproject_result(file, "pip", table, None, declared, &default)
}

/// Storage drops items with the same key: a name listed twice (other
/// markers, other versions) keeps each entry with a running qualifier.
pub(super) fn keep_repeated(items: &mut [InventoryItem]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in items.iter_mut() {
        let count = seen.entry(item.key()).or_default();
        *count += 1;
        if *count > 1 {
            item.evidence.qualifier.push_str(&format!("#{count}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    fn found<'a>(name: &'a str, content: &'a str, siblings: &'a [String]) -> FoundFile<'a> {
        FoundFile { name, rel_dir: "", content, siblings }
    }

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
    fn requirement_lines_name_direct_references_without_keeping_urls() {
        let text = "\
# comment
-e .
-e git+https://deploy:s3cr3t@git.corp.example/team/lib.git@v1#egg=corp-lib
-e ./packages/unnamed
https://files.pythonhosted.org/packages/ab/cd/requests-2.32.3-py3-none-any.whl
./vendor/Tool_Kit-1.4.0.tar.gz
pkg @ https://user:pw@files.corp.example/pkg-1.0.tar.gz
numpy==1.26.4 \\
    --hash=sha256:aaaa \\
    --hash=sha256:bbbb
attrs  # trailing comment
-c constraints.txt
";
        let items = Pip.parse_file(&file("requirements-dev.txt", text)).unwrap();
        assert!(items[0].detail.contains("1 Direktverweise ohne Paketnamen"), "{}", items[0].detail);
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}: {items:?}"));
        assert_eq!(find("corp-lib").source, SourceClass::Git);
        assert!(find("corp-lib").detail.contains("editierbar"));
        let requests = find("requests");
        assert_eq!((requests.version.as_deref(), requests.source), (Some("2.32.3"), SourceClass::PublicRegistry));
        assert_eq!(find("tool-kit").source, SourceClass::Path);
        let pkg = find("pkg");
        assert_eq!(
            (pkg.source, pkg.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("files.corp.example"))
        );
        assert_eq!(find("numpy").version.as_deref(), Some("1.26.4"), "hash options are no part of the version");
        assert_eq!(find("attrs").detail, "ohne Versionsangabe");
        assert_eq!(items.len(), 7, "the project itself and unnamed paths are no items: {items:?}");
        let text = format!("{items:?}");
        for secret in ["s3cr3t", "deploy", "user:pw", "packages/ab", "vendor/"] {
            assert!(!text.contains(secret), "{secret} leaked");
        }
    }

    #[test]
    fn compiled_requirements_are_resolved_and_constraints_have_their_context() {
        let compiled = "#\n# This file is autogenerated by pip-compile with Python 3.12\n# by the following command:\n#\n#    pip-compile requirements.in\n#\nclick==8.1.7\n    # via flask\nflask==3.0.3\n    # via -r requirements.in\n";
        let items = Pip.parse_file(&file("requirements.txt", compiled)).unwrap();
        assert!(items[0].detail.starts_with("Festgelegte Requirements"));
        let flask = items.iter().find(|i| i.name == "flask").unwrap();
        assert_eq!((flask.install_state, flask.version.as_deref()), (Some(InstallState::Resolved), Some("3.0.3")));
        let uv = "# This file was autogenerated by uv via the following command:\n#    uv export --format requirements-txt\nidna==3.7\n";
        let items = Pip.parse_file(&file("requirements.txt", uv)).unwrap();
        assert_eq!(items[1].install_state, Some(InstallState::Resolved));

        let input = Pip.parse_file(&file("requirements-dev.in", "pytest>=8\n-c constraints.txt\n")).unwrap();
        assert!(input[0].detail.starts_with("pip-tools-Eingabe"));
        assert_eq!(input[1].install_state, Some(InstallState::Declared));
        let constraints = Pip.parse_file(&file("constraints.txt", "urllib3<2\n")).unwrap();
        assert_eq!(constraints[1].evidence.context, "constraint");
        assert!(constraints[1].detail.contains("nur Einschränkung"));
    }

    #[test]
    fn repeated_requirement_names_are_all_kept() {
        let text = "numpy==1.24.4; python_version < '3.9'\nnumpy==1.26.4; python_version >= '3.9'\n";
        let items = Pip.parse_file(&file("requirements.txt", text)).unwrap();
        assert_eq!(items.len(), 3);
        assert_ne!(items[1].key(), items[2].key());
    }

    #[test]
    fn setup_cfg_lists_and_extras() {
        let text = "\
[metadata]
name = demo

[options]
python_requires = >=3.8
install_requires =
    requests>=2.28
    importlib-metadata; python_version<\"3.10\"
    # a comment
setup_requires = setuptools_scm; wheel
tests_require = pytest

[options.extras_require]
pdf = ReportLab>=1.2; RXP
dev =
    file: requirements-dev.txt

[flake8]
max-line-length = 120
";
        let items = Pip.parse_file(&file("setup.cfg", text)).unwrap();
        assert!(
            items[0].detail.contains("Projekt demo") && items[0].detail.contains("1 Verweise"),
            "{}",
            items[0].detail
        );
        assert!(items[0].detail.contains("Python >=3.8"));
        let context = |name: &str| items.iter().find(|i| i.name == name).map(|i| i.evidence.context.as_str());
        assert_eq!(context("requests"), Some(""));
        assert_eq!(context("importlib-metadata"), Some(""));
        assert_eq!(context("setuptools-scm"), Some("build"));
        assert_eq!(context("wheel"), Some("build"));
        assert_eq!(context("pytest"), Some("test"));
        assert_eq!(context("reportlab"), Some("extra:pdf"));
        assert_eq!(context("rxp"), Some("extra:pdf"));
        assert_eq!(items.len(), 8, "{items:?}");
        assert!(items.iter().all(|i| i.name != "python-version"));
    }

    #[test]
    fn setup_cfg_of_other_tools_is_no_manifest() {
        let text = "[flake8]\nmax-line-length = 120\n[tool:pytest]\ntestpaths = tests\n[metadata]\nname = demo\n";
        assert!(Pip.parse_file(&file("setup.cfg", text)).unwrap().is_empty());
    }

    #[test]
    fn pyproject_declares_without_resolving() {
        let text = "[project]\nname='demo'\ndependencies=['Django>=5', 'typing_extensions']\n[dependency-groups]\ndev=['pytest']\n";
        let items = Pip.parse_file(&file("pyproject.toml", text)).unwrap();
        assert!(items.iter().any(|i| i.name == "typing-extensions" && i.install_state == Some(InstallState::Declared)));
        assert!(items.iter().any(|i| i.name == "pytest"));
    }

    #[test]
    fn pyproject_groups_extras_build_and_hatch_environments() {
        let text = r#"
[build-system]
requires = ["hatchling>=1.24"]
build-backend = "hatchling.build"

[project]
name = "Demo_App"
requires-python = ">=3.10"
dependencies = ["httpx (>=0.27,<1)", "corp-lib @ git+https://token@git.corp.example/x.git"]

[project.optional-dependencies]
cli = ["rich==13.7.1"]
all = ["demo-app[cli]", "rich==13.7.1"]

[dependency-groups]
test = ["pytest>=8", {include-group = "lint"}]
lint = ["ruff"]

[tool.hatch.envs.docs]
dependencies = ["mkdocs"]
extra-dependencies = ["mkdocs-material"]
"#;
        let items = Pip.parse_file(&file("pyproject.toml", text)).unwrap();
        assert_eq!(items[0].detail, "Projekt Demo_App · 9 Abhängigkeiten · Python >=3.10");
        let find = |name: &str, context: &str| {
            items.iter().find(|i| i.name == name && i.evidence.context == context).unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(find("httpx", "").detail, "Spezifikation >=0.27,<1");
        assert_eq!(find("corp-lib", "").source, SourceClass::Git);
        assert_eq!(find("rich", "extra:cli").version.as_deref(), Some("13.7.1"));
        assert_eq!(find("rich", "extra:all").detail, "Spezifikation ==13.7.1 · Extra all");
        assert_eq!(find("hatchling", "build").detail, "Spezifikation >=1.24 · Build");
        assert_eq!(find("mkdocs-material", "hatch:docs").detail, "ohne Versionsangabe · Hatch-Umgebung docs");
        find("ruff", "lint");
        find("pytest", "test");
        assert!(items.iter().all(|i| i.name != "demo-app"), "the project's own extras are no dependency");
        assert!(items.iter().all(|i| i.name != "python"), "requires-python is a runtime pin");
        assert!(!format!("{items:?}").contains("token"));
    }

    #[test]
    fn private_tool_index_marks_pip_owned_pyproject_private() {
        let text = "[project]\nname='x'\ndependencies=['internal']\n[[tool.uv.index]]\nname='corp'\nurl='https://u:p@pkgs.corp.example/simple'\n";
        let items = Pip.parse_file(&file("pyproject.toml", text)).unwrap();
        assert_eq!(items[1].source, SourceClass::PrivateRegistry);
        assert!(items[0].detail.ends_with("private Quelle"));
        let public = "[project]\nname='x'\ndependencies=['requests']\n[[tool.pdm.source]]\nname='pypi'\nurl='https://pypi.org/simple'\n";
        let items = Pip.parse_file(&file("pyproject.toml", public)).unwrap();
        assert_eq!(items[1].source, SourceClass::Unknown, "pip itself may use another index");
    }

    #[test]
    fn exactly_one_adapter_reports_a_pyproject() {
        use crate::adapters::{pdm::Pdm, poetry::Poetry, uv::Uv};
        let pep621 = "[project]\nname = 'demo'\ndependencies = ['requests>=2']\n";
        let poetry = "[tool.poetry]\nname = 'demo'\n[tool.poetry.dependencies]\npython = '^3.11'\nrequests = '^2.31'\n";
        let adapters: [&dyn Adapter; 4] = [&Pip, &Uv, &Poetry, &Pdm];
        // A lockfile decides; uv and PDM do not read Poetry's own tables.
        for (content, sibling, owner, reads) in [
            (pep621, None, "pip", true),
            (pep621, Some("uv.lock"), "uv", true),
            (pep621, Some("poetry.lock"), "poetry", true),
            (pep621, Some("pdm.lock"), "pdm", true),
            (poetry, None, "poetry", true),
            (poetry, Some("uv.lock"), "uv", false),
            (poetry, Some("pdm.lock"), "pdm", false),
        ] {
            let siblings: Vec<String> = sibling.into_iter().map(str::to_string).collect();
            let file = found("pyproject.toml", content, &siblings);
            let mut reporting = Vec::new();
            for adapter in adapters {
                let items = adapter.parse_file(&file).unwrap();
                if !items.is_empty() {
                    assert!(items.iter().all(|i| i.ecosystem == owner));
                    assert_eq!(items.iter().any(|i| i.name == "requests"), reads, "{items:?}");
                    reporting.push(adapter.descriptor().id);
                }
            }
            assert_eq!(reporting, vec![owner], "{sibling:?}");
        }
        for (sibling, owner) in [(None, "pip"), (Some("poetry.lock"), "poetry"), (Some("uv.lock"), "uv")] {
            let siblings: Vec<String> = sibling.into_iter().map(str::to_string).collect();
            let file = found("pyproject.toml", "[project\nname =", &siblings);
            let failing: Vec<&str> =
                adapters.iter().filter(|a| a.parse_file(&file).is_err()).map(|a| a.descriptor().id).collect();
            assert_eq!(failing, vec![owner], "a broken pyproject.toml is one error");
        }
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

    #[test]
    fn index_sources_combine_honestly() {
        let public = (SourceClass::PublicRegistry, Some("pypi.org".to_string()));
        let private = (SourceClass::PrivateRegistry, Some("pkgs.corp.example".to_string()));
        assert_eq!(combine(&[]), UNKNOWN);
        assert_eq!(combine(&[public.clone(), private.clone()]), private);
        assert_eq!(combine(&[public.clone(), public.clone()]), public);
        let other = (SourceClass::PublicRegistry, Some("files.pythonhosted.org".to_string()));
        assert_eq!(combine(&[public.clone(), other]), (SourceClass::PublicRegistry, None));
        assert_eq!(combine(&[public, UNKNOWN]), UNKNOWN);
        assert_eq!(requirement("-e ."), None);
        assert_eq!(requirement("requests (>=2.0)"), Some(("requests".into(), ">=2.0".into())));
    }
}
