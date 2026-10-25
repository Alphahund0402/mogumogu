//! Container images: Dockerfile and Containerfile (FROM, COPY --from,
//! RUN --mount from, the syntax directive), Compose files (services.*.image),
//! devcontainer.json (image, features) and the images of GitLab CI and
//! Bitbucket Pipelines. Nothing is pulled, built or resolved: a tag is not
//! turned into a digest, and variables come only from defaults in the same
//! file (ARG before the first FROM, `${VAR:-default}`, GitLab `variables:`).
//! `.env` files, YAML anchors, `extends` and `include` are not evaluated; a
//! `COPY --from` name without tag, registry or path counts as a build context.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::{json, lines};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub struct Docker;

pub(super) const PUBLIC: &[&str] = &[
    "docker.io",
    "registry-1.docker.io",
    "mcr.microsoft.com",
    "ghcr.io",
    "quay.io",
    "gcr.io",
    "registry.k8s.io",
    "public.ecr.aws",
];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "docker",
    name: "Container-Images",
    markers: &[
        "Dockerfile",
        "Dockerfile.*",
        "*.dockerfile",
        "Containerfile",
        "Containerfile.*",
        "compose*.yaml",
        "compose*.yml",
        "docker-compose*.yaml",
        "docker-compose*.yml",
        "devcontainer.json",
        ".devcontainer.json",
        ".gitlab-ci.yml",
        "*.gitlab-ci.yml",
        "bitbucket-pipelines.yml",
    ],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "Dockerfile/Containerfile (FROM, COPY --from, RUN --mount, syntax)",
        "compose.yaml/docker-compose.yml (services.*.image)",
        "devcontainer.json (image, features)",
        ".gitlab-ci.yml (image, services)",
        "bitbucket-pipelines.yml (image, Dienste)",
    ],
    limits: "Kein Docker-, Podman- oder Registry-Aufruf; Images werden weder gezogen noch gebaut, Tags nicht zu Digests aufgelöst. Variablen nur aus Standardwerten derselben Datei (ARG vor dem ersten FROM, ${VAR:-Standard}, GitLab-variables); .env, YAML-Anker, extends und include werden nicht ausgewertet.",
};

impl Adapter for Docker {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let name = file.name.to_ascii_lowercase();
        let yaml = name.ends_with(".yml") || name.ends_with(".yaml");
        match name.as_str() {
            "devcontainer.json" | ".devcontainer.json" => bounded(devcontainer(file)?),
            "bitbucket-pipelines.yml" => bounded(bitbucket(file)),
            _ if name.ends_with(".gitlab-ci.yml") => bounded(gitlab(file)),
            _ if yaml && (name.starts_with("compose") || name.starts_with("docker-compose")) => bounded(compose(file)),
            _ => bounded(dockerfile(file)),
        }
    }
}

/// `1 Stufe`, `2 Stufen`.
pub(super) fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Keys above an entry of [`lines::yaml_entries`], tracked by indentation.
#[derive(Default)]
pub(super) struct KeyPath<'a> {
    indents: Vec<usize>,
    keys: Vec<&'a str>,
}

impl<'a> KeyPath<'a> {
    /// Leaves the mappings `entry` is no part of; `keys()` are then its parents.
    pub fn enter(&mut self, entry: &lines::YamlEntry<'a>) {
        while self.indents.last().is_some_and(|indent| *indent >= entry.indent) {
            self.indents.pop();
            self.keys.pop();
        }
    }

    pub fn keys(&self) -> &[&'a str] {
        &self.keys
    }

    /// Makes the key of `entry` the parent of deeper entries.
    pub fn push(&mut self, entry: &lines::YamlEntry<'a>) {
        if let Some(key) = entry.key
            && self.keys.len() < 32
        {
            self.indents.push(entry.indent);
            self.keys.push(key);
        }
    }
}

/// Items of one file, unique by storage key: a name repeated in the same
/// place keeps its version (or `?`) as qualifier, an exact repeat is dropped.
#[derive(Default)]
pub(super) struct Unique {
    pub items: Vec<InventoryItem>,
    keys: HashSet<String>,
    exact: HashSet<(String, Option<String>)>,
}

impl Unique {
    pub fn push(&mut self, mut item: InventoryItem) {
        let mut key = item.key();
        if !self.exact.insert((key.clone(), item.version.clone())) {
            return;
        }
        if self.keys.contains(&key) {
            let version = item.version.as_deref().unwrap_or("?");
            item.evidence.qualifier = match item.evidence.qualifier.as_str() {
                "" => version.to_string(),
                qualifier => format!("{qualifier}|{version}"),
            };
            key = item.key();
        }
        if self.keys.insert(key) {
            self.items.push(item);
        }
    }

    /// More items than a file may hold: the result is an error anyway.
    pub fn full(&self) -> bool {
        self.items.len() > crate::limits::ITEMS_PER_FILE
    }
}

/// Stands in for a variable whose value is not in the file.
const UNRESOLVED: char = '\u{1}';
/// Longest text that is substituted at all; references and defaults are short.
const EXPANDED_MAX: usize = 4_096;

/// Defaults declared in the same file (`ARG`, GitLab `variables:`); `None`
/// is a declaration without a value.
pub(super) type Vars = HashMap<String, Option<String>>;

struct Expanded {
    text: String,
    /// A default from the file or from `${VAR:-default}` was used.
    defaulted: bool,
}

impl Expanded {
    fn unresolved() -> Self {
        Self { text: UNRESOLVED.to_string(), defaulted: false }
    }
}

fn lookup<'v>(vars: &'v Vars, name: &str) -> Option<&'v str> {
    vars.get(name).and_then(|value| value.as_deref())
}

/// Substitutes `$VAR`, `${VAR}`, `${VAR:-word}`, `${VAR-word}`, `${VAR:+word}`
/// and `${VAR+word}`; whatever `vars` does not answer becomes [`UNRESOLVED`],
/// as do expressions (`${{ … }}`, `$[[ … ]]`) and over-long results. `escape`
/// before `$` keeps it.
fn expand(text: &str, vars: &Vars, escape: Option<char>, depth: usize) -> Expanded {
    if text.len() > EXPANDED_MAX {
        return Expanded::unresolved();
    }
    let mut out = String::with_capacity(text.len());
    let mut defaulted = false;
    let mut rest = text;
    while let Some(index) = rest.find(|c: char| c == '$' || Some(c) == escape) {
        if out.len() > EXPANDED_MAX {
            return Expanded::unresolved();
        }
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        if !tail.starts_with('$') {
            let marker = tail.chars().next().unwrap_or('\\');
            let after = &tail[marker.len_utf8()..];
            match after.strip_prefix('$') {
                Some(next) => {
                    out.push('$');
                    rest = next;
                }
                None => {
                    out.push(marker);
                    rest = after;
                }
            }
            continue;
        }
        let after = &tail[1..];
        if let Some(next) = after.strip_prefix('$') {
            out.push('$');
            rest = next;
        } else if let Some(expression) = after.strip_prefix("{{") {
            out.push(UNRESOLVED);
            rest = expression.find("}}").map_or("", |end| &expression[end + 2..]);
        } else if let Some(expression) = after.strip_prefix("[[") {
            out.push(UNRESOLVED);
            rest = expression.find("]]").map_or("", |end| &expression[end + 2..]);
        } else if let Some(body) = after.strip_prefix('{') {
            let Some(end) = closing_brace(body) else {
                out.push(UNRESOLVED);
                rest = "";
                break;
            };
            let (value, used) = resolve(&body[..end], vars, escape, depth);
            out.push_str(&value);
            defaulted |= used;
            rest = &body[end + 1..];
        } else {
            let length = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
            if length == 0 {
                out.push('$');
            } else {
                match lookup(vars, &after[..length]) {
                    Some(value) => {
                        out.push_str(value);
                        defaulted = true;
                    }
                    None => out.push(UNRESOLVED),
                }
            }
            rest = &after[length..];
        }
    }
    out.push_str(rest);
    if out.len() > EXPANDED_MAX {
        return Expanded::unresolved();
    }
    Expanded { text: out, defaulted }
}

/// The `}` that closes a `${`, skipping nested braces.
fn closing_brace(body: &str) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(index),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The inside of `${…}`: a name and an optional `:-`, `-`, `:+` or `+` word.
fn resolve(inner: &str, vars: &Vars, escape: Option<char>, depth: usize) -> (String, bool) {
    let length = inner.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(inner.len());
    let (name, modifier) = inner.split_at(length);
    let value = lookup(vars, name).filter(|value| !value.is_empty());
    let word = |word: &str| {
        if depth >= 4 {
            return (UNRESOLVED.to_string(), false);
        }
        (expand(word, vars, escape, depth + 1).text, true)
    };
    if name.is_empty() {
        return (UNRESOLVED.to_string(), false);
    }
    if let Some(default) = modifier.strip_prefix(":-").or_else(|| modifier.strip_prefix('-')) {
        return match value {
            Some(value) => (value.to_string(), true),
            None => word(default),
        };
    }
    if let Some(alternative) = modifier.strip_prefix(":+").or_else(|| modifier.strip_prefix('+')) {
        return if value.is_some() { word(alternative) } else { (UNRESOLVED.to_string(), false) };
    }
    match value {
        Some(value) => (value.to_string(), true),
        None => (UNRESOLVED.to_string(), false),
    }
}

/// One image reference: `[host[:port]/]path[:tag][@digest]`.
struct Reference<'a> {
    host: Option<&'a str>,
    path: &'a str,
    tag: Option<&'a str>,
    digest: Option<&'a str>,
}

impl<'a> Reference<'a> {
    fn parse(text: &'a str) -> Self {
        let (rest, digest) = match text.split_once('@') {
            Some((rest, digest)) => (rest, Some(digest)),
            None => (text, None),
        };
        let slash = rest.rfind('/');
        let (name, tag) = match rest.rfind(':') {
            Some(colon) if slash.is_none_or(|slash| colon > slash) => (&rest[..colon], Some(&rest[colon + 1..])),
            _ => (rest, None),
        };
        // The first component is a registry when it looks like a host.
        let (host, path) = match name.split_once('/') {
            Some((first, path)) if first.contains(['.', ':']) || first == "localhost" => (Some(first), path),
            _ => (None, name),
        };
        Self { host, path, tag, digest }
    }
}

fn valid_host(host: &str) -> bool {
    let (name, port) = host.split_once(':').unwrap_or((host, ""));
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        && port.chars().all(|c| c.is_ascii_digit())
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 255
        && path.split('/').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && tag.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// `sha256:` and the first 12 hex digits; a full digest would read as a secret.
fn short_digest(digest: &str) -> Option<String> {
    let (algorithm, hex) = digest.split_once(':')?;
    let valid = !algorithm.is_empty()
        && algorithm.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '_' | '-'))
        && hex.len() >= 12
        && hex.chars().all(|c| c.is_ascii_hexdigit());
    valid.then(|| format!("{algorithm}:{}", hex[..12].to_ascii_lowercase()))
}

/// Image items of one file and the references whose name stayed unknown.
pub(super) struct Images {
    rel_path: String,
    escape: Option<char>,
    /// Detail note when a default from the file filled in a variable.
    defaulted: &'static str,
    pub vars: Vars,
    pub unique: Unique,
    pub unresolved: usize,
    /// References already handled in a place.
    seen: HashSet<(String, String, String)>,
}

impl Images {
    pub fn new(rel_path: String, escape: Option<char>, defaulted: &'static str) -> Self {
        let (vars, unique, seen) = (Vars::new(), Unique::default(), HashSet::new());
        Self { rel_path, escape, defaulted, vars, unique, unresolved: 0, seen }
    }

    /// Adds the image that `raw` names once `vars` are substituted. Invalid
    /// references are skipped, ones with a variable for a name counted.
    pub fn add(&mut self, raw: &str, context: &str, qualifier: &str, label: &str) {
        let raw = raw.trim();
        if raw.is_empty()
            || self.unique.full()
            || !self.seen.insert((raw.to_string(), context.to_string(), qualifier.to_string()))
        {
            return;
        }
        let expanded = expand(raw, &self.vars, self.escape, 0);
        let reference = Reference::parse(&expanded.text);
        let alias = raw.starts_with('*');
        if alias || reference.host.is_some_and(|host| host.contains(UNRESOLVED)) || reference.path.contains(UNRESOLVED)
        {
            self.unresolved += 1;
            return;
        }
        let host = reference.host.map(str::to_ascii_lowercase);
        if !host.as_deref().is_none_or(valid_host) || !valid_path(reference.path) {
            return;
        }
        let variable = [reference.tag, reference.digest].iter().flatten().any(|part| part.contains(UNRESOLVED));
        let tag = reference.tag.filter(|tag| !tag.contains(UNRESOLVED));
        if tag.is_some_and(|tag| !valid_tag(tag)) {
            return;
        }
        let digest = match reference.digest.filter(|digest| !digest.contains(UNRESOLVED)) {
            Some(digest) => match short_digest(digest) {
                Some(short) => Some(short),
                None => return,
            },
            None => None,
        };
        let hub =
            host.as_deref().is_none_or(|host| matches!(host, "docker.io" | "index.docker.io" | "registry-1.docker.io"));
        let (name, registry) = match host {
            Some(host) if !hub => (format!("{host}/{}", reference.path), host),
            _ => (reference.path.strip_prefix("library/").unwrap_or(reference.path).to_string(), "docker.io".into()),
        };
        let (source, source_host) = classify_url(&format!("https://{registry}/"), PUBLIC);
        let fact = if digest.is_some() {
            "per Digest gepinnt"
        } else if variable {
            "Version aus Variable"
        } else if tag.is_some() {
            "Tag, nicht gepinnt"
        } else {
            "ohne Tag (latest)"
        };
        let mut detail = format!("{label} · {fact}");
        if expanded.defaulted {
            detail.push_str(" · ");
            detail.push_str(self.defaulted);
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "docker", name, self.rel_path.as_str())
            .version(tag.map(str::to_string).or(digest).unwrap_or_default())
            .state(InstallState::Declared)
            .source(source, source_host)
            .detail(detail);
        item.evidence.context = context.into();
        item.evidence.qualifier = qualifier.into();
        self.unique.push(item);
    }

    /// Manifest detail addition for references without a known name.
    pub fn note(&self, origin: &str) -> String {
        match self.unresolved {
            0 => String::new(),
            n => format!(" · {} aus {origin} (nicht aufgelöst)", count(n, "Image", "Images")),
        }
    }
}

/// `[a, b]` or a single scalar; nested flow collections count as one.
fn list_len(value: &str) -> usize {
    lines::flow_list(value).map_or(usize::from(!value.is_empty()), |items| items.len())
}

/// An image as a YAML value: a scalar after any anchor (`&base python:3`), or
/// `key` of a one-line flow mapping (`{name: postgres:16, alias: db}`).
/// Aliases (`*base`) stay as written and count as unresolved.
pub(super) fn image_value<'a>(value: &'a str, key: &str) -> &'a str {
    let value = match value.strip_prefix('&') {
        Some(anchored) => anchored.split_once(' ').map_or("", |(_, rest)| rest.trim().trim_matches(['"', '\''])),
        None => value,
    };
    let Some(inner) = value.strip_prefix('{').and_then(|v| v.strip_suffix('}')) else { return value };
    inner
        .split(',')
        .find_map(|part| {
            let (name, value) = part.split_once(':')?;
            (name.trim().trim_matches(['"', '\'']) == key).then(|| value.trim().trim_matches(['"', '\'']))
        })
        .unwrap_or_default()
}

/// A Dockerfile `# key=value` parser directive; unknown keys end the header.
fn directive(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.trim().strip_prefix('#')?.split_once('=')?;
    let key = key.trim();
    ["syntax", "escape", "check"].iter().any(|known| known.eq_ignore_ascii_case(key)).then(|| (key, value.trim()))
}

/// Instructions with continuation lines joined; comments, empty lines and
/// heredoc bodies are dropped. Keywords are upper-case.
fn instructions(lines: &[&str], escape: char) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut index = 0;
    // Lines that heredoc terminators may be searched in, for the whole file.
    let mut budget = 1_000_000;
    while index < lines.len() {
        let line = lines[index].trim();
        index += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(head) = line.strip_suffix(escape) {
            current.push_str(head);
            current.push(' ');
            continue;
        }
        current.push_str(line);
        let instruction = std::mem::take(&mut current);
        let (keyword, rest) = instruction.split_once(char::is_whitespace).unwrap_or((&instruction, ""));
        let keyword = keyword.to_ascii_uppercase();
        if matches!(keyword.as_str(), "RUN" | "COPY" | "ADD") {
            index = skip_heredocs(rest, lines, index, &mut budget);
        }
        out.push((keyword, rest.trim().to_string()));
    }
    if let Some((keyword, rest)) = current.trim().split_once(char::is_whitespace) {
        out.push((keyword.to_ascii_uppercase(), rest.trim().to_string()));
    }
    out
}

/// The line after the bodies of the heredocs (`<<EOF`, `<<-"EOF"`) that an
/// instruction opens; unchanged when a terminator is missing.
fn skip_heredocs(args: &str, lines: &[&str], start: usize, budget: &mut usize) -> usize {
    let mut index = start;
    let mut rest = args;
    while let Some(at) = rest.find("<<") {
        rest = &rest[at + 2..];
        if rest.starts_with('<') {
            rest = rest.trim_start_matches('<');
            continue;
        }
        let (tabs, word) = match rest.strip_prefix('-') {
            Some(word) => (true, word),
            None => (false, rest),
        };
        let word = word.trim_start_matches(['"', '\'']);
        let end = word.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(word.len());
        let delimiter = &word[..end];
        if delimiter.is_empty() || delimiter.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let window = &lines[index..lines.len().min(index + *budget)];
        let body = window.iter().position(|line| {
            let line = if tabs { line.trim_start_matches('\t') } else { line };
            line.trim_end() == delimiter
        });
        *budget -= body.map_or(window.len(), |offset| offset + 1);
        match body {
            Some(offset) => index += offset + 1,
            None => return start,
        }
    }
    index
}

/// Whitespace-separated words; quotes keep spaces and are removed.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut quoted = false;
    for c in text.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => word.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                quoted = true;
            }
            None if c.is_whitespace() => {
                if !word.is_empty() || quoted {
                    out.push(std::mem::take(&mut word));
                    quoted = false;
                    if out.len() >= 1_000 {
                        return out;
                    }
                }
            }
            None => word.push(c),
        }
    }
    if !word.is_empty() || quoted {
        out.push(word);
    }
    out
}

fn dockerfile(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let lines: Vec<&str> = file.content.lines().take(200_000).collect();
    let (mut escape, mut syntax, mut start) = ('\\', None, 0);
    while let Some((key, value)) = lines.get(start).and_then(|line| directive(line)) {
        if key.eq_ignore_ascii_case("escape") && value == "`" {
            escape = '`';
        } else if key.eq_ignore_ascii_case("syntax") {
            syntax = Some(value);
        }
        start += 1;
    }
    let instructions = instructions(&lines[start..], escape);
    // A Dockerfile starts with FROM, after directives, comments and global ARGs.
    if !instructions.first().is_some_and(|(keyword, _)| keyword == "FROM" || keyword == "ARG") {
        return Vec::new();
    }
    let stages: Vec<Option<String>> =
        instructions.iter().filter(|(keyword, _)| keyword == "FROM").map(|(_, rest)| stage_name(rest)).collect();
    // FROM may name an earlier stage, COPY --from any stage.
    let named: HashSet<&str> = stages.iter().flatten().map(String::as_str).collect();
    let mut earlier: HashSet<&str> = HashSet::new();
    let mut images = Images::new(file.rel_path(), Some(escape), "Standardwert aus ARG");
    if let Some(syntax) = syntax {
        images.add(syntax, "syntax", "", "Dockerfile-Frontend");
    }
    let (mut stage, mut downloads) = (0, 0);
    for (keyword, rest) in &instructions {
        match keyword.as_str() {
            // Only ARGs before the first FROM reach FROM lines.
            "ARG" if stage == 0 => {
                for word in words(rest) {
                    let (name, value) = match word.split_once('=') {
                        Some((name, value)) => (name, Some(value)),
                        None => (word.as_str(), None),
                    };
                    if images.vars.len() >= 1_000 && !images.vars.contains_key(name) {
                        continue;
                    }
                    let value = value
                        .filter(|_| !crate::privacy::is_sensitive_key(name))
                        .map(|value| expand(value, &images.vars, Some(escape), 0).text);
                    images.vars.insert(name.to_string(), value);
                }
            }
            "FROM" => {
                let words = words(rest);
                let image = words.iter().find(|word| !word.starts_with("--"));
                let lower = image.map(|image| image.to_ascii_lowercase()).unwrap_or_default();
                if let Some(image) = image.filter(|_| lower != "scratch" && !earlier.contains(lower.as_str())) {
                    let context = stages[stage].clone().unwrap_or_else(|| stage.to_string());
                    images.add(image, &context, "", "Basisimage");
                }
                if let Some(name) = &stages[stage] {
                    earlier.insert(name.as_str());
                }
                stage += 1;
            }
            "COPY" | "RUN" => {
                for word in words(rest).iter().take_while(|word| word.starts_with("--")) {
                    let (from, context, label) = if keyword == "COPY" {
                        (word.strip_prefix("--from="), "copy-from", "Quelle für COPY --from")
                    } else {
                        let mount = word.strip_prefix("--mount=").unwrap_or_default();
                        (
                            mount.split(',').find_map(|part| part.strip_prefix("from=")),
                            "mount-from",
                            "Quelle für RUN --mount",
                        )
                    };
                    let Some(from) = from else { continue };
                    let expanded = expand(from, &images.vars, Some(escape), 0).text.to_ascii_lowercase();
                    let looks_like_image = expanded.contains([':', '/', '@', UNRESOLVED]);
                    if looks_like_image && expanded.parse::<usize>().is_err() && !named.contains(expanded.as_str()) {
                        images.add(from, context, "", label);
                    }
                }
            }
            "ADD" => {
                // Sources before the destination that name a URL or Git remote.
                let words = words(rest);
                let sources = words.iter().filter(|word| !word.starts_with("--")).collect::<Vec<_>>();
                downloads += sources
                    .iter()
                    .take(sources.len().saturating_sub(1))
                    .filter(|source| source.contains("://") || source.starts_with("git@"))
                    .count();
            }
            _ => {}
        }
    }
    let kind = if file.name.to_ascii_lowercase().contains("containerfile") { "Containerfile" } else { "Dockerfile" };
    let mut detail = format!(
        "{kind} · {} · {}",
        count(stages.len(), "Stufe", "Stufen"),
        count(images.unique.items.len(), "Image", "Images")
    );
    if downloads > 0 {
        detail.push_str(&format!(" · {} aus dem Netz (nicht gelesen)", count(downloads, "ADD-Quelle", "ADD-Quellen")));
    }
    detail.push_str(&images.note("Variablen"));
    let mut items = vec![manifest("docker", file, detail)];
    items.extend(images.unique.items);
    items
}

/// The lower-case `AS name` of a FROM instruction.
fn stage_name(rest: &str) -> Option<String> {
    let words = words(rest);
    let mut plain = words.iter().filter(|word| !word.starts_with("--")).skip(1);
    match (plain.next(), plain.next()) {
        (Some(keyword), Some(name)) if keyword.eq_ignore_ascii_case("as") => Some(name.to_ascii_lowercase()),
        _ => None,
    }
}

struct Service<'a> {
    name: &'a str,
    image: Option<&'a str>,
    build: bool,
}

fn compose(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut path = KeyPath::default();
    let (mut recognized, mut includes) = (false, 0);
    let mut services: Vec<Service> = Vec::new();
    for entry in lines::yaml_entries(file.content).take(500_000) {
        path.enter(&entry);
        match (path.keys(), entry.key) {
            ([], Some("services")) => recognized = true,
            ([], Some("include")) => {
                recognized = true;
                includes += list_len(entry.value);
            }
            (["include"], _) => includes += usize::from(entry.item),
            (["services"], Some(name)) if services.len() < 10_000 => {
                services.push(Service { name, image: None, build: false })
            }
            (["services", name], Some(key @ ("image" | "build"))) => {
                // A service's keys follow it directly; past the cap it is missing.
                if let Some(service) = services.last_mut().filter(|service| service.name == *name) {
                    match key {
                        "image" => service.image = Some(image_value(entry.value, "name")).filter(|i| !i.is_empty()),
                        _ => service.build = true,
                    }
                }
            }
            _ => {}
        }
        path.push(&entry);
    }
    if !recognized {
        return Vec::new();
    }
    let mut images = Images::new(file.rel_path(), None, "Standardwert aus Variable");
    let (mut built, mut without) = (0, 0);
    for service in &services {
        match (service.image, service.build) {
            // An image beside `build` names the service's own build result.
            (_, true) => built += 1,
            (Some(image), false) => images.add(image, service.name, "", "Dienst-Image"),
            (None, false) => without += 1,
        }
    }
    let mut detail = format!("Compose-Datei · {}", count(services.len(), "Dienst", "Dienste"));
    if built > 0 {
        detail.push_str(&format!(" · {built} mit eigenem build"));
    }
    if without > 0 {
        detail.push_str(&format!(" · {without} ohne Image-Angabe"));
    }
    if includes > 0 {
        detail.push_str(&format!(" · {} (nicht gefolgt)", count(includes, "include-Eintrag", "include-Einträge")));
    }
    detail.push_str(&images.note("Variablen oder YAML-Ankern"));
    let mut items = vec![manifest("docker", file, detail)];
    items.extend(images.unique.items);
    items
}

fn devcontainer(file: &FoundFile<'_>) -> AdapterResult {
    // `.devcontainer/devcontainer.json`, `.devcontainer/<folder>/devcontainer.json`
    // or `.devcontainer.json`.
    let dir = file.rel_dir.to_ascii_lowercase();
    let mut parents = dir.rsplit('/');
    let located = file.name.eq_ignore_ascii_case(".devcontainer.json")
        || parents.next() == Some(".devcontainer")
        || parents.next() == Some(".devcontainer");
    if !located {
        return Ok(Vec::new());
    }
    let value = json::parse_jsonc(file.content, 64).map_err(|e| e.to_string())?;
    let Some(config) = value.as_object() else { return Ok(Vec::new()) };
    let text = |key: &str| config.get(key).and_then(Value::as_str);
    let image = text("image");
    let dockerfile =
        config.get("build").and_then(|b| b.get("dockerfile")).and_then(Value::as_str).or(text("dockerFile"));
    let compose = config.get("dockerComposeFile").is_some_and(|v| v.is_string() || v.is_array());
    let features = config.get("features").and_then(Value::as_object);
    if image.is_none() && dockerfile.is_none() && !compose && features.is_none() {
        return Ok(Vec::new());
    }
    let mut images = Images::new(file.rel_path(), None, "Standardwert aus Variable");
    let mut detail = String::from("Dev-Container");
    if let Some(image) = image {
        images.add(image, "devcontainer", "", "Dev-Container-Image");
        detail.push_str(" · Image");
    }
    if dockerfile.is_some() {
        detail.push_str(" · Image aus eigenem Dockerfile");
    }
    if compose {
        detail.push_str(&format!(" · Compose-Dienst {}", text("service").unwrap_or("?")));
    }
    let mut archives = 0;
    // Feature options (`"version": "lts"`) pin tools, not features: not read.
    for id in features.into_iter().flat_map(|features| features.keys()).take(1_000) {
        if id.contains("://") {
            archives += 1;
        } else if id.starts_with("./") || id.starts_with("../") {
            let mut item = InventoryItem::new(ItemCategory::Package, "docker", id.as_str(), file.rel_path())
                .state(InstallState::Declared)
                .source(SourceClass::Path, None)
                .detail("Dev-Container-Feature · lokaler Ordner");
            item.evidence.context = "devcontainer-feature".into();
            images.unique.push(item);
        } else if id.contains('/') {
            images.add(id, "devcontainer-feature", "", "Dev-Container-Feature");
        } else {
            let mut item = InventoryItem::new(ItemCategory::Package, "docker", id.as_str(), file.rel_path())
                .state(InstallState::Declared)
                .detail("Dev-Container-Feature · veraltete Kurzform ohne Registry");
            item.evidence.context = "devcontainer-feature".into();
            images.unique.push(item);
        }
    }
    if let Some(features) = features {
        detail.push_str(&format!(" · {}", count(features.len(), "Feature", "Features")));
    }
    if archives > 0 {
        detail.push_str(&format!(" · {archives} als Archiv-URL (nicht ausgewertet)"));
    }
    detail.push_str(&images.note("Variablen"));
    let mut items = vec![manifest("docker", file, detail)];
    items.extend(images.unique.items);
    Ok(items)
}

/// Top-level GitLab keywords that are not jobs.
const GITLAB_GLOBAL: &[&str] = &[
    "default",
    "include",
    "stages",
    "variables",
    "workflow",
    "spec",
    "image",
    "services",
    "cache",
    "before_script",
    "after_script",
];

/// Top-level `variables:` with a plain value (`NAME: value` or
/// `NAME: {value: …}`); secret-looking names are never kept.
fn gitlab_variables(text: &str) -> Vars {
    let mut vars = Vars::new();
    let mut path = KeyPath::default();
    for entry in lines::yaml_entries(text).take(500_000) {
        path.enter(&entry);
        let value = match (path.keys(), entry.key) {
            (["variables"], Some(name)) if !entry.value.is_empty() => Some((name, entry.value)),
            (["variables", name], Some("value")) => Some((*name, entry.value)),
            _ => None,
        };
        if let Some((name, value)) = value
            && !crate::privacy::is_sensitive_key(name)
            && (vars.len() < 1_000 || vars.contains_key(name))
        {
            let value = expand(value, &vars, None, 0).text;
            vars.insert(name.to_string(), Some(value));
        }
        path.push(&entry);
    }
    vars
}

fn gitlab(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut images = Images::new(file.rel_path(), None, "Standardwert aus variables");
    images.vars = gitlab_variables(file.content);
    let mut path = KeyPath::default();
    let (mut keys, mut jobs, mut includes, mut components) = (0, 0, 0, 0);
    for entry in lines::yaml_entries(file.content).take(500_000) {
        path.enter(&entry);
        let parents = path.keys();
        match (parents, entry.key) {
            ([], Some("include")) => includes += list_len(entry.value),
            (["include"], key) => {
                includes += usize::from(entry.item);
                components += usize::from(key == Some("component"));
            }
            ([], Some(key)) if !GITLAB_GLOBAL.contains(&key) => jobs += 1,
            _ => {}
        }
        keys += usize::from(parents.is_empty() && entry.key.is_some());
        // Global `image` and `services` are the deprecated form of `default:`.
        let (owner, tail) = match parents {
            [] | ["image" | "services", ..] => ("default", parents),
            [owner, tail @ ..] => (*owner, tail),
        };
        if owner != "default" && GITLAB_GLOBAL.contains(&owner) {
            path.push(&entry);
            continue;
        }
        match (tail, entry.key) {
            ([], Some("image")) | (["image"], Some("name")) => gitlab_image(&mut images, entry.value, owner, false),
            ([], Some("services")) => {
                for service in lines::flow_list(entry.value).unwrap_or_default() {
                    gitlab_image(&mut images, service, owner, true);
                }
            }
            (["services"], None) if entry.item => gitlab_image(&mut images, entry.value, owner, true),
            (["services"], Some("name")) => gitlab_image(&mut images, entry.value, owner, true),
            _ => {}
        }
        path.push(&entry);
    }
    if keys == 0 {
        return Vec::new();
    }
    let mut detail = format!("GitLab-CI · {}", count(jobs, "Job", "Jobs"));
    if includes > 0 {
        detail.push_str(&format!(
            " · {} (davon {}, nicht gefolgt)",
            count(includes, "include-Eintrag", "include-Einträge"),
            count(components, "Komponente", "Komponenten")
        ));
    }
    detail.push_str(&images.note("Variablen oder YAML-Ankern"));
    let mut items = vec![manifest("docker", file, detail)];
    items.extend(images.unique.items);
    items
}

/// A job's image or service. The Dependency Proxy prefix stands for Docker Hub.
fn gitlab_image(images: &mut Images, value: &str, owner: &str, service: bool) {
    let value = image_value(value, "name");
    let proxied = ["CI_DEPENDENCY_PROXY_GROUP_IMAGE_PREFIX", "CI_DEPENDENCY_PROXY_DIRECT_GROUP_IMAGE_PREFIX"]
        .iter()
        .find_map(|prefix| {
            value.strip_prefix(&format!("${{{prefix}}}/")).or_else(|| value.strip_prefix(&format!("${prefix}/")))
        });
    let label = match (service, proxied.is_some()) {
        (false, false) => "CI-Image",
        (false, true) => "CI-Image über Dependency Proxy",
        (true, false) => "CI-Dienst",
        (true, true) => "CI-Dienst über Dependency Proxy",
    };
    images.add(proxied.unwrap_or(value), owner, "", label);
}

/// A Bitbucket step while its keys are read.
struct Step<'a> {
    indent: usize,
    number: usize,
    name: Option<&'a str>,
    image: Option<&'a str>,
    pipes: Vec<&'a str>,
}

fn bitbucket(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut images = Images::new(file.rel_path(), None, "Standardwert aus Variable");
    let mut path = KeyPath::default();
    let (mut recognized, mut steps, mut pipes) = (false, 0, 0);
    let mut step: Option<Step> = None;
    for entry in lines::yaml_entries(file.content).take(500_000) {
        path.enter(&entry);
        if step.as_ref().is_some_and(|step| entry.indent <= step.indent) {
            flush_step(step.take(), &mut images);
        }
        match (path.keys(), entry.key) {
            ([], Some("pipelines" | "definitions")) => recognized = true,
            ([], Some("image")) | (["image"], Some("name")) => {
                recognized = true;
                images.add(image_value(entry.value, "name"), "default", "", "CI-Image");
            }
            (["definitions", "services", service], Some("image"))
            | (["definitions", "services", service, "image"], Some("name")) => {
                images.add(image_value(entry.value, "name"), service, "", "CI-Dienst")
            }
            (_, Some("step")) => {
                flush_step(step.take(), &mut images);
                steps += 1;
                step = Some(Step { indent: entry.indent, number: steps, name: None, image: None, pipes: Vec::new() });
            }
            ([.., "step"], Some("name")) => {
                if let Some(step) = step.as_mut() {
                    step.name = Some(entry.value).filter(|name| !name.is_empty());
                }
            }
            ([.., "step"], Some("image")) | ([.., "step", "image"], Some("name")) => {
                if let Some(step) = step.as_mut() {
                    step.image = Some(image_value(entry.value, "name"));
                }
            }
            ([.., "script" | "after-script"], Some("pipe")) => {
                pipes += 1;
                if let (Some(step), Some(image)) = (step.as_mut(), entry.value.strip_prefix("docker://"))
                    && step.pipes.len() < 100
                {
                    step.pipes.push(image);
                }
            }
            _ => {}
        }
        path.push(&entry);
    }
    flush_step(step.take(), &mut images);
    if !recognized {
        return Vec::new();
    }
    let mut detail = format!("Bitbucket Pipelines · {}", count(steps, "Schritt", "Schritte"));
    if pipes > 0 {
        detail.push_str(&format!(" · {} (nur docker:// als Image gelesen)", count(pipes, "Pipe", "Pipes")));
    }
    detail.push_str(&images.note("Variablen oder YAML-Ankern"));
    let mut items = vec![manifest("docker", file, detail)];
    items.extend(images.unique.items);
    items
}

fn flush_step(step: Option<Step<'_>>, images: &mut Images) {
    let Some(step) = step else { return };
    let context = step.name.map_or_else(|| format!("Schritt {}", step.number), str::to_string);
    if let Some(image) = step.image {
        images.add(image, &context, "", "CI-Image");
    }
    for pipe in step.pipes {
        images.add(pipe, &context, "pipe", "Pipe-Image");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    fn find<'a>(items: &'a [InventoryItem], name: &str) -> &'a InventoryItem {
        items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name} in {items:#?}"))
    }

    const DOCKERFILE: &str = r#"# syntax=docker/dockerfile:1.7
# escape=\
ARG PYTHON_VERSION=3.12
ARG REGISTRY=registry.corp.example:5000
ARG NODE_TAG
ARG NPM_TOKEN=npm_secret_never_stored
FROM --platform=$BUILDPLATFORM python:${PYTHON_VERSION}-slim AS build
RUN <<EOF
FROM not-an-image:1
EOF
COPY --from=ghcr.io/astral-sh/uv:0.4.18 /uv /bin/uv
ADD --checksum=sha256:24454f830cdb571e2c4ad15481119c43b3cafd48dd869a9b2945d1036d1dc68d https://github.com/moby/buildkit/archive/v0.12.0.tar.gz /src/
ADD git@github.com:acme/private.git#main /repo
ADD ./local.txt https-notes.txt /app/
RUN --mount=type=cache,target=/root/.cache \
    # a comment inside the continuation
    --mount=type=bind,from=docker.io/library/busybox:1.36,source=/bin,target=/tools \
    uv sync

from node:${NODE_TAG} as Assets
FROM ${REGISTRY}/team/runtime@sha256:0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef
COPY --from=build /app /app
COPY --from=0 /x /x
COPY --from=assets /dist /dist
COPY --from=deps /lock /lock
FROM scratch
FROM build AS final
FROM $UNKNOWN_BASE
FROM ${NPM_TOKEN}
"#;

    #[test]
    fn dockerfile_stages_arguments_and_copy_sources() {
        let items = Docker.parse_file(&file("Dockerfile", DOCKERFILE)).unwrap();
        assert_eq!(
            items[0].detail,
            "Dockerfile · 7 Stufen · 6 Images · 2 ADD-Quellen aus dem Netz (nicht gelesen) · 2 Images aus Variablen (nicht aufgelöst)",
            "{items:#?}"
        );
        let syntax = find(&items, "docker/dockerfile");
        assert_eq!((syntax.version.as_deref(), syntax.evidence.context.as_str()), (Some("1.7"), "syntax"));
        let python = find(&items, "python");
        assert_eq!(python.version.as_deref(), Some("3.12-slim"));
        assert_eq!(python.evidence.context, "build");
        assert_eq!(python.detail, "Basisimage · Tag, nicht gepinnt · Standardwert aus ARG");
        assert_eq!((python.source, python.source_host.as_deref()), (SourceClass::PublicRegistry, Some("docker.io")));
        let uv = find(&items, "ghcr.io/astral-sh/uv");
        assert_eq!((uv.version.as_deref(), uv.evidence.context.as_str()), (Some("0.4.18"), "copy-from"));
        assert_eq!(uv.source_host.as_deref(), Some("ghcr.io"));
        let busybox = find(&items, "busybox");
        assert_eq!((busybox.version.as_deref(), busybox.evidence.context.as_str()), (Some("1.36"), "mount-from"));
        let node = find(&items, "node");
        assert!(node.version.is_none() && node.detail == "Basisimage · Version aus Variable", "{}", node.detail);
        assert_eq!(node.evidence.context, "assets");
        let runtime = find(&items, "registry.corp.example:5000/team/runtime");
        assert_eq!(runtime.version.as_deref(), Some("sha256:0123456789ab"));
        assert_eq!(runtime.evidence.context, "2");
        assert!(runtime.detail.starts_with("Basisimage · per Digest gepinnt"), "{}", runtime.detail);
        assert_eq!(
            (runtime.source, runtime.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("registry.corp.example"))
        );
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        let text = format!("{items:?}");
        for absent in ["not-an-image", "scratch", "npm_secret", "deps", "0123456789abcdef0123", "buildkit", "acme"] {
            assert!(!text.contains(absent), "{absent}");
        }
    }

    #[test]
    fn dockerfile_escape_directive_and_foreign_files() {
        let windows = "# escape=`\nFROM mcr.microsoft.com/windows/servercore:ltsc2022 `\n    AS base\nCOPY C:\\src\\ C:\\app\\\nFROM mcr.microsoft.com/dotnet/runtime:8.0\n";
        let items = Docker.parse_file(&file("Containerfile", windows)).unwrap();
        assert_eq!(items[0].detail, "Containerfile · 2 Stufen · 2 Images");
        let core = find(&items, "mcr.microsoft.com/windows/servercore");
        assert_eq!((core.version.as_deref(), core.evidence.context.as_str()), (Some("ltsc2022"), "base"));
        assert_eq!(core.source, SourceClass::PublicRegistry);
        assert_eq!(find(&items, "mcr.microsoft.com/dotnet/runtime").evidence.context, "1");

        let plain = Docker.parse_file(&file("api.dockerfile", "FROM docker.io/library/alpine\n")).unwrap();
        assert_eq!(plain[1].name, "alpine");
        assert_eq!(plain[1].detail, "Basisimage · ohne Tag (latest)");
        let notes = "# Dockerfile notes\n\nUse this with care.\n```dockerfile\nFROM python:3.12\n```\n";
        assert!(Docker.parse_file(&file("Dockerfile.md", notes)).unwrap().is_empty());
        assert!(Docker.parse_file(&file("Dockerfile.dockerignore", "node_modules\n*.log\n")).unwrap().is_empty());
        assert!(Docker.parse_file(&file("Dockerfile", "")).unwrap().is_empty());
    }

    #[test]
    fn compose_images_with_defaults_and_build_only_services() {
        let text = "name: shop\nservices:\n  web:\n    build: .\n    image: shop/web:dev\n  api:\n    build:\n      context: ./api\n  db:\n    image: \"postgres:${POSTGRES_VERSION:-16}-alpine\"\n    environment:\n      POSTGRES_PASSWORD: example\n  cache:\n    image: ${CACHE_IMAGE}\n  worker:\n    image: ghcr.io/acme/worker:${TAG}\n  queue:\n    image: &queue rabbitmq:3-management # comment\n  extended:\n    extends:\n      file: common.yaml\n      service: base\ninclude:\n  - ./monitoring/compose.yaml\nx-defaults: &defaults\n  image: never/used:1\n";
        let items = Docker.parse_file(&file("compose.yaml", text)).unwrap();
        assert_eq!(
            items[0].detail,
            "Compose-Datei · 7 Dienste · 2 mit eigenem build · 1 ohne Image-Angabe · 1 include-Eintrag (nicht gefolgt) · 1 Image aus Variablen oder YAML-Ankern (nicht aufgelöst)"
        );
        let db = find(&items, "postgres");
        assert_eq!((db.version.as_deref(), db.evidence.context.as_str()), (Some("16-alpine"), "db"));
        assert_eq!(db.detail, "Dienst-Image · Tag, nicht gepinnt · Standardwert aus Variable");
        let worker = find(&items, "ghcr.io/acme/worker");
        assert!(worker.version.is_none() && worker.detail.contains("Version aus Variable"));
        assert_eq!(find(&items, "rabbitmq").version.as_deref(), Some("3-management"));
        assert_eq!(items.len(), 4, "{items:#?}");
        assert!(!format!("{items:?}").contains("shop/web") && !format!("{items:?}").contains("never/used"));

        let override_file = "services:\n  web:\n    image: nginx:1.27\n";
        let items = Docker.parse_file(&file("docker-compose.override.yml", override_file)).unwrap();
        assert_eq!(find(&items, "nginx").version.as_deref(), Some("1.27"));
        assert!(Docker.parse_file(&file("compose.yaml", "foo: bar\n")).unwrap().is_empty());
    }

    #[test]
    fn devcontainer_image_features_and_location() {
        let text = r#"{
  // Dev container for the API
  "name": "API",
  "image": "mcr.microsoft.com/devcontainers/python:1-3.12-bookworm",
  "features": {
    "ghcr.io/devcontainers/features/node:1": { "version": "lts" },
    "ghcr.io/devcontainers/features/docker-in-docker@sha256:4fd3d4ad06c3d6ffb2d0a5c2e1e4b7b2c9d0e1f2a3b4c5d6e7f8091a2b3c4d5e": {},
    "./local-feature": {},
    "https://github.com/acme/features/releases/download/v1/devcontainer-feature-x.tgz": {},
    "common": "latest",
  },
  "postCreateCommand": "pip install -r requirements.txt",
}"#;
        let found = FoundFile { name: "devcontainer.json", rel_dir: ".devcontainer", content: text, siblings: &[] };
        let items = Docker.parse_file(&found).unwrap();
        assert_eq!(items[0].detail, "Dev-Container · Image · 5 Features · 1 als Archiv-URL (nicht ausgewertet)");
        let image = find(&items, "mcr.microsoft.com/devcontainers/python");
        assert_eq!(
            (image.version.as_deref(), image.evidence.context.as_str()),
            (Some("1-3.12-bookworm"), "devcontainer")
        );
        let node = find(&items, "ghcr.io/devcontainers/features/node");
        assert_eq!((node.version.as_deref(), node.evidence.context.as_str()), (Some("1"), "devcontainer-feature"));
        let dind = find(&items, "ghcr.io/devcontainers/features/docker-in-docker");
        assert_eq!(dind.version.as_deref(), Some("sha256:4fd3d4ad06c3"));
        assert_eq!(find(&items, "./local-feature").source, SourceClass::Path);
        let legacy = find(&items, "common");
        assert_eq!((legacy.version.as_deref(), legacy.source), (None, SourceClass::Unknown), "options are tool pins");
        assert!(!format!("{items:?}").contains("lts"));
        assert!(!format!("{items:?}").contains("devcontainer-feature-x"));

        let nested = FoundFile { rel_dir: "svc/.devcontainer/python", ..found };
        assert_eq!(Docker.parse_file(&nested).unwrap().len(), items.len());
        let compose = r#"{"dockerComposeFile": ["../compose.yaml"], "service": "app"}"#;
        let root = FoundFile { name: ".devcontainer.json", rel_dir: "", content: compose, siblings: &[] };
        assert_eq!(Docker.parse_file(&root).unwrap()[0].detail, "Dev-Container · Compose-Dienst app");
        let elsewhere = FoundFile { rel_dir: "config", ..found };
        assert!(Docker.parse_file(&elsewhere).unwrap().is_empty(), "not a devcontainer location");
        let other = FoundFile { content: r#"{"settings": {}}"#, ..found };
        assert!(Docker.parse_file(&other).unwrap().is_empty());
        let broken = FoundFile { content: "{\"image\": ", ..found };
        assert!(Docker.parse_file(&broken).is_err());
    }

    #[test]
    fn gitlab_images_services_variables_and_includes() {
        let text = "include:\n  - local: /ci/templates.yml\n  - component: $CI_SERVER_FQDN/acme/components/sast@1.2.0\nvariables:\n  PYTHON_IMAGE: python:3.12-slim\n  DEPLOY_TOKEN: glpat-0123456789abcdefghij\n  BASE:\n    value: debian:12\n    description: Base image\ndefault:\n  image: alpine:3.20\n  services:\n    - docker:27-dind\nstages: [test, deploy]\n.python:\n  image: $PYTHON_IMAGE\ntest:\n  extends: .python\n  services:\n    - name: postgres:16\n      alias: db\n    - redis:7\n  script:\n    - pytest\ndeploy:\n  image:\n    name: registry.gitlab.example.com/acme/deployer:2.1\n    entrypoint: [\"\"]\n  script:\n    - deploy\nbuild:\n  image: $CI_REGISTRY_IMAGE/builder:latest\n  script: [make]\nleak:\n  image: $DEPLOY_TOKEN\n  script: [x]\nbase:\n  image: ${BASE}\n  services: [\"mysql:8\"]\n  script: [x]\nproxy:\n  image: ${CI_DEPENDENCY_PROXY_GROUP_IMAGE_PREFIX}/node:20\n  script: [x]\n.img: &ruby_image ruby:3.3\nalias:\n  image: *ruby_image\n  services:\n    - {name: mariadb:11, alias: db}\n  script: [x]\nflow:\n  image: {name: \"elixir:1.17\", entrypoint: [\"\"]}\n  script: [x]\n";
        let items = Docker.parse_file(&file(".gitlab-ci.yml", text)).unwrap();
        assert_eq!(
            items[0].detail,
            "GitLab-CI · 10 Jobs · 2 include-Einträge (davon 1 Komponente, nicht gefolgt) · 3 Images aus Variablen oder YAML-Ankern (nicht aufgelöst)"
        );
        assert_eq!(find(&items, "mariadb").evidence.context, "alias");
        assert_eq!(find(&items, "elixir").version.as_deref(), Some("1.17"));
        let proxied = find(&items, "node");
        assert_eq!((proxied.version.as_deref(), proxied.source_host.as_deref()), (Some("20"), Some("docker.io")));
        assert_eq!(proxied.detail, "CI-Image über Dependency Proxy · Tag, nicht gepinnt");
        let alpine = find(&items, "alpine");
        assert_eq!((alpine.version.as_deref(), alpine.evidence.context.as_str()), (Some("3.20"), "default"));
        let dind = find(&items, "docker");
        assert_eq!((dind.version.as_deref(), dind.evidence.context.as_str()), (Some("27-dind"), "default"));
        let python = find(&items, "python");
        assert_eq!(python.evidence.context, ".python");
        assert_eq!(python.detail, "CI-Image · Tag, nicht gepinnt · Standardwert aus variables");
        assert_eq!(find(&items, "postgres").evidence.context, "test");
        assert_eq!(find(&items, "redis").detail, "CI-Dienst · Tag, nicht gepinnt");
        let deployer = find(&items, "registry.gitlab.example.com/acme/deployer");
        assert_eq!((deployer.version.as_deref(), deployer.source), (Some("2.1"), SourceClass::PrivateRegistry));
        assert_eq!(find(&items, "debian").version.as_deref(), Some("12"));
        assert_eq!(find(&items, "mysql").evidence.context, "base");
        assert!(!format!("{items:?}").contains("glpat"), "secret variables are never substituted");
        assert!(Docker.parse_file(&file("build.gitlab-ci.yml", "lint:\n  image: node:22\n")).unwrap().len() == 2);
        assert!(Docker.parse_file(&file(".gitlab-ci.yml", "# empty\n")).unwrap().is_empty());
    }

    #[test]
    fn bitbucket_default_step_service_and_pipe_images() {
        let text = "image: atlassian/default-image:4\ndefinitions:\n  services:\n    postgres:\n      image: postgres:16\n    private:\n      image:\n        name: acme/private-db:1.0\n        username: $DOCKER_USER\n        password: hunter2-literal\n  steps:\n    - step: &lint\n        name: Lint\n        image: python:3.12\n        script:\n          - ruff check .\npipelines:\n  default:\n    - step: *lint\n    - parallel:\n        - step:\n            image: node:20\n            script:\n              - npm test\n              - pipe: atlassian/slack-notify:2.1.0\n              - pipe: docker://acme/notify:1.0\n    - step:\n        name: Deploy\n        image:\n          name: 123456789012.dkr.ecr.eu-central-1.amazonaws.com/deployer:3\n          aws:\n            access-key: $AWS_KEY\n        script:\n          - ./deploy.sh\n";
        let items = Docker.parse_file(&file("bitbucket-pipelines.yml", text)).unwrap();
        assert_eq!(items[0].detail, "Bitbucket Pipelines · 4 Schritte · 2 Pipes (nur docker:// als Image gelesen)");
        let default = find(&items, "atlassian/default-image");
        assert_eq!((default.version.as_deref(), default.evidence.context.as_str()), (Some("4"), "default"));
        assert_eq!(find(&items, "postgres").evidence.context, "postgres");
        assert_eq!(find(&items, "acme/private-db").evidence.context, "private");
        assert_eq!(find(&items, "python").evidence.context, "Lint");
        assert_eq!(find(&items, "node").evidence.context, "Schritt 3");
        let pipe = find(&items, "acme/notify");
        assert_eq!((pipe.evidence.context.as_str(), pipe.evidence.qualifier.as_str()), ("Schritt 3", "pipe"));
        let deployer = find(&items, "123456789012.dkr.ecr.eu-central-1.amazonaws.com/deployer");
        assert_eq!((deployer.evidence.context.as_str(), deployer.source), ("Deploy", SourceClass::PrivateRegistry));
        assert!(!format!("{items:?}").contains("hunter2"));
        assert!(Docker.parse_file(&file("bitbucket-pipelines.yml", "foo: bar\n")).unwrap().is_empty());
    }

    #[test]
    fn references_variables_and_repeated_names() {
        let mut images = Images::new("x".into(), None, "Standardwert aus Variable");
        images.vars.insert("A".into(), Some("1".into()));
        images.vars.insert("EMPTY".into(), None);
        for raw in [
            "index.docker.io/library/redis:${A}",
            "redis:${EMPTY:-2}",
            "redis:${A}",
            "localhost:5000/app",
            "${{ matrix.image }}",
            "$[[ inputs.image ]]:1",
            "redis:${A:+x}${MISSING+y}",
            "Not An Image",
            "user:password@host/x",
        ] {
            images.add(raw, "c", "", "L");
        }
        let names: Vec<(&str, Option<&str>, &str)> = images
            .unique
            .items
            .iter()
            .map(|i| (i.name.as_str(), i.version.as_deref(), i.evidence.qualifier.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("redis", Some("1"), ""),
                ("redis", Some("2"), "2"),
                ("localhost:5000/app", None, ""),
                ("redis", None, "?")
            ]
        );
        assert_eq!(images.unresolved, 2);
        assert!(!format!("{:?}", images.unique.items).contains("password"));
        let localhost = &images.unique.items[2];
        assert_eq!(
            (localhost.source, localhost.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("localhost"))
        );

        // Doubling defaults stop being substituted instead of growing.
        let mut vars = Vars::new();
        vars.insert("L0".into(), Some("a".repeat(3_000)));
        for level in 1..20 {
            let doubled = expand(&format!("${{L{}}}${{L{}}}", level - 1, level - 1), &vars, None, 0).text;
            vars.insert(format!("L{level}"), Some(doubled));
        }
        assert_eq!(vars["L1"].as_deref(), Some(UNRESOLVED.to_string().as_str()));
        assert!(vars.values().flatten().all(|value| value.len() <= EXPANDED_MAX));
    }

    #[test]
    fn oversized_files_stop_early_and_end_as_errors() {
        let text: String = (0..25_000).map(|i| format!("FROM img{i} AS s{i}\n")).collect();
        assert!(Docker.parse_file(&file("Dockerfile", &text)).is_err(), "more items than a file may hold");
        let repeated = "FROM a AS b\n".to_string() + &"COPY --from=x/y:1 a b\n".repeat(50_000);
        let items = Docker.parse_file(&file("Dockerfile", &repeated)).unwrap();
        assert_eq!(items.len(), 3, "a repeated reference is one item");
        // Heredocs without a terminator hide nothing.
        let open = "FROM a\n".to_string() + &"RUN <<A\nFROM b\n".repeat(5_000);
        let detail = &Docker.parse_file(&file("Dockerfile", &open)).unwrap()[0].detail;
        assert_eq!(detail, "Dockerfile · 5001 Stufen · 5001 Images");
    }
}
