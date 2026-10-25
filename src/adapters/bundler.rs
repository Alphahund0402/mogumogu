//! Bundler: Gemfile/gems.rb and `*.gemspec` (declared), Gemfile.lock and
//! gems.locked (resolved) and `vendor/bundle` (installed folder names).
//! Gemfiles and gemspecs are Ruby code: lines are matched as text, never
//! evaluated, so requirements from variables, `eval_gemfile` and conditions
//! stay unknown. Neither ruby nor bundle is started, and `.bundle/`, whose
//! configuration can hold credentials, is never read.
use super::mix::{balance, code, names, split_top};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use regex::Regex;
use std::sync::LazyLock;

pub struct Bundler;

const PUBLIC: &[&str] = &["rubygems.org", "index.rubygems.org"];

type Source = (SourceClass, Option<String>);

static DESCRIPTOR: Descriptor = Descriptor {
    id: "bundler",
    name: "Bundler",
    markers: &["Gemfile", "Gemfile.lock", "gems.rb", "gems.locked", "*.gemspec"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "Gemfile/gems.rb: gem-Zeilen, Gruppen- und Quellblöcke als Text",
        "Gemfile.lock/gems.locked (GEM, GIT, PATH, PLUGIN SOURCE, DEPENDENCIES, PLATFORMS)",
        "*.gemspec: add_dependency als Text",
        "vendor/bundle (Ordnernamen)",
    ],
    limits: "Kein ruby- oder bundle-Aufruf; Gemfile und gemspec werden nicht ausgewertet – Versionen aus Variablen, eval_gemfile und Bedingungen bleiben offen; .bundle/ (Konfiguration, Zugangsdaten) wird nie gelesen.",
};

/// `gem "name", …` (or `plugin`), with or without parentheses.
static GEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(gem|plugin)[\s(]+["']([^"'\s]+)["']\s*(.*)$"#).expect("valid regex"));
/// A block of the DSL: `group :test do`, `source "…" do`, `platforms :jruby do` …
static BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(group|platforms?|source|git|github|path|install_if|env)\b(.*?)\bdo(?:\s*\|[^|]*\|)?$")
        .expect("valid regex")
});
/// Any other block that a later `end` closes, also `x = if …`.
static OTHER_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\bdo(?:\s*\|[^|]*\|)?$|^(if|unless|case|while|until|begin|def|class|module|for)\b|=\s*(?:if|unless|case|begin)\b",
    )
    .expect("valid regex")
});
static END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^end\b").expect("valid regex"));
static SOURCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^source[\s(]+["']([^"']+)["']"#).expect("valid regex"));
static RUBY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^ruby[\s(]+(?:["']([^"']+)["']|file:)"#).expect("valid regex"));
static GIT_SOURCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^git_source[\s(]*:(\w+)").expect("valid regex"));
/// A trailing `if`/`unless` condition.
static MODIFIER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|\s)(?:if|unless)\s").expect("valid regex"));
/// `key: value`, `:key => value` or `"key" => value`.
static OPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^[:"']?([A-Za-z_]\w*)["']?(?::\s+|:$|\s*=>\s*)(.*)$"#).expect("valid regex"));
/// `spec.add_dependency "name", "req"` or the generated `%q<name>.freeze` form.
static SPEC_DEPENDENCY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\.add_(runtime_|development_)?dependency[\s(]+(?:["']([^"']+)["']|%q[<({\[]([^>)}\]]+)[>)}\]])(.*)$"#)
        .expect("valid regex")
});
static SPEC_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\.name\s*=\s*["']([^"']+)["']"#).expect("valid regex"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]*)"|'([^']*)'"#).expect("valid regex"));

impl Adapter for Bundler {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name.to_ascii_lowercase().as_str() {
            "gemfile.lock" | "gems.locked" => bounded(lockfile(file)?),
            "gemfile" => bounded(gemfile(file)),
            "gems.rb" if is_gemfile(file) => bounded(gemfile(file)),
            "gems.rb" => Ok(Vec::new()),
            _ if file.content.contains("Gem::Specification") => bounded(gemspec(file)),
            _ => Ok(Vec::new()),
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], children: &[String]) -> Option<DirClaim> {
        let beside = |names: &[&str]| siblings.iter().any(|s| names.iter().any(|n| s.eq_ignore_ascii_case(n)));
        match name {
            // Settings that can hold credentials: claimed so that nothing below is read.
            ".bundle" if beside(&["Gemfile", "gems.rb", "Gemfile.lock", "gems.locked"]) => {
                Some(DirClaim { adapter: "bundler", category: ItemCategory::Environment, label: ".bundle" })
            }
            "vendor" if beside(&["Gemfile.lock", "gems.locked"]) && children.iter().any(|c| c == "bundle") => {
                Some(DirClaim { adapter: "bundler", category: ItemCategory::Environment, label: "vendor/bundle" })
            }
            _ => None,
        }
    }

    /// Folder names only: no file below a claimed folder is opened.
    fn inspect_dir(&self, claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let (root, detail): (&[&str], &str) = if claim.label == ".bundle" {
            (&[], "Bundler-Einstellungen, keine Datei gelesen (können Zugangsdaten enthalten)")
        } else {
            (&["bundle"], "Installierte Gems laut Ordnernamen")
        };
        let packages = installed(access, root, rel_path);
        let detail = match packages.len() {
            0 if claim.label == ".bundle" => detail.to_string(),
            count => format!("{detail} · {count} Gems; nicht geladen"),
        };
        let mut items =
            vec![InventoryItem::new(ItemCategory::Environment, "bundler", claim.label, rel_path).detail(detail)];
        items.extend(packages);
        bounded(items)
    }
}

/// Gems installed below `root/<engine>/<abi>/`: `gems/<name>-<version>` and
/// Git checkouts `bundler/gems/<name>-<revision>`, both by folder name.
fn installed(access: &mut dyn DirAccess, root: &[&str], rel_path: &str) -> Vec<InventoryItem> {
    let mut packages = Vec::new();
    for (engine, is_dir) in access.list(root).into_iter().take(8) {
        if !is_dir || engine.starts_with('.') {
            continue;
        }
        let mut engine_path = root.to_vec();
        engine_path.push(&engine);
        for (abi, is_dir) in access.list(&engine_path).into_iter().take(16) {
            if !is_dir || !abi.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            let mut base = engine_path.clone();
            base.push(&abi);
            let rel = format!("{rel_path}/{}", base.join("/"));
            for (dir, is_dir) in access.list(&[base.as_slice(), &["gems"]].concat()) {
                let Some((name, version, platform)) = split_gem_dir(&dir).filter(|_| is_dir) else { continue };
                let mut detail = String::from("Ordner der Installation; nicht geladen");
                if !platform.is_empty() {
                    detail.push_str(&format!(" · Plattform {platform}"));
                }
                let mut item = InventoryItem::new(ItemCategory::Package, "bundler", name, format!("{rel}/gems"))
                    .version(version)
                    .state(InstallState::Installed)
                    .detail(detail);
                item.evidence.qualifier = dir.clone();
                packages.push(item);
            }
            for (dir, is_dir) in access.list(&[base.as_slice(), &["bundler", "gems"]].concat()) {
                let Some((name, _)) = dir.rsplit_once('-').filter(|(_, rev)| is_dir && revision(rev)) else {
                    continue;
                };
                let mut item =
                    InventoryItem::new(ItemCategory::Package, "bundler", name, format!("{rel}/bundler/gems"))
                        .state(InstallState::Installed)
                        .source(SourceClass::Git, None)
                        .detail("Git-Checkout der Installation; Version unbekannt");
                item.evidence.qualifier = dir.clone();
                packages.push(item);
            }
        }
    }
    packages
}

/// A `gems.rb` is Bundler's when gems.locked lies beside it or it reads like
/// a Gemfile: DSL statements and no class or module of its own.
fn is_gemfile(file: &FoundFile<'_>) -> bool {
    if file.siblings.iter().any(|s| s.eq_ignore_ascii_case("gems.locked")) {
        return true;
    }
    let mut dsl = false;
    for line in file.content.lines().take(20_000) {
        if ["class ", "module ", "def "].iter().any(|word| line.starts_with(word)) {
            return false;
        }
        let code = code(line).trim_start();
        dsl |= GEM.is_match(code) || SOURCE.is_match(code) || code.starts_with("gemspec");
    }
    dsl
}

/// Ruby statements without comments. A line that ends with a comma or
/// leaves a bracket open continues on the next one (at most ten).
fn statements(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending = String::new();
    let mut joined = 0;
    let mut documentation = false;
    for line in text.lines().take(20_000) {
        if documentation {
            documentation = !line.starts_with("=end");
            continue;
        }
        if line.starts_with("=begin") {
            documentation = true;
            continue;
        }
        if line == "__END__" {
            break;
        }
        let code = code(line).trim();
        if code.is_empty() {
            continue;
        }
        if !pending.is_empty() {
            pending.push(' ');
        }
        pending.push_str(code);
        if (pending.ends_with(',') || balance(&pending) > 0) && joined < 10 {
            joined += 1;
            continue;
        }
        out.push(std::mem::take(&mut pending));
        joined = 0;
    }
    if !pending.is_empty() {
        out.push(pending);
    }
    out
}

/// Groups, platforms, source and condition of the enclosing blocks.
#[derive(Clone, Default)]
struct Frame {
    groups: Vec<String>,
    platforms: Vec<String>,
    source: Option<Source>,
    conditional: bool,
}

/// One `gem` or `plugin` statement.
struct Gem {
    name: String,
    plugin: bool,
    requirements: Vec<String>,
    variable: bool,
    frame: Frame,
}

fn gemfile(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut frames: Vec<Frame> = Vec::new();
    let mut overflow = 0_usize;
    let mut globals: Vec<Source> = Vec::new();
    let mut git_sources: Vec<String> = Vec::new();
    let mut gems = Vec::new();
    let (mut ruby, mut gemspec, mut evals) = (None, false, 0);
    for statement in statements(file.content) {
        let s = statement.as_str();
        if END.is_match(s) {
            if overflow > 0 {
                overflow -= 1;
            } else {
                frames.pop();
            }
            continue;
        }
        let top = frames.last().cloned().unwrap_or_default();
        if let Some(c) = GEM.captures(s) {
            gems.push(gem(&c[2], &c[1] == "plugin", &c[3], top, &git_sources));
            continue;
        }
        let opened = if let Some(c) = BLOCK.captures(s) {
            Some(dsl_block(&c[1], &c[2], top))
        } else {
            if let Some(c) = SOURCE.captures(s) {
                globals.push(classify_url(&c[1], PUBLIC));
            } else if let Some(c) = GIT_SOURCE.captures(s) {
                git_sources.push(c[1].to_string());
            } else if let Some(c) = RUBY.captures(s) {
                ruby = Some(c.get(1).map_or("aus Datei", |m| m.as_str()).to_string());
            } else if s == "gemspec" || s.starts_with("gemspec ") || s.starts_with("gemspec(") {
                gemspec = true;
            } else if s.starts_with("eval_gemfile") {
                evals += 1;
            }
            OTHER_BLOCK.captures(s).filter(|_| !s.ends_with(" end")).map(|c| {
                let condition = matches!(c.get(1).map(|m| m.as_str()), Some("if" | "unless" | "case"));
                Frame { conditional: top.conditional || condition, ..top }
            })
        };
        if let Some(frame) = opened {
            if frames.len() < 32 {
                frames.push(frame);
            } else {
                overflow += 1;
            }
        }
    }
    let global = match globals.as_slice() {
        [] => (SourceClass::Unknown, None),
        [single] => single.clone(),
        [first, ..] if globals.iter().all(|(class, _)| *class == SourceClass::PublicRegistry) => first.clone(),
        _ => (SourceClass::Unknown, None),
    };
    let mut detail = format!("{} (nicht ausgeführt) · {} Gems", file.name, gems.len());
    if let Some(host) = &global.1 {
        detail.push_str(&format!(" · Quelle {host}"));
    }
    if let Some(ruby) = ruby {
        detail.push_str(&format!(" · Ruby {ruby}"));
    }
    if gemspec {
        detail.push_str(" · gemspec eingebunden");
    }
    if evals > 0 {
        detail.push_str(&format!(" · {evals} eval_gemfile nicht gefolgt"));
    }
    let mut items = vec![manifest("bundler", file, detail)];
    items.extend(gems.into_iter().map(|gem| gem_item(file, gem, &global)));
    items
}

fn dsl_block(kind: &str, args: &str, mut frame: Frame) -> Frame {
    match kind {
        "group" => extend(&mut frame.groups, arguments(args)),
        "platforms" | "platform" => extend(&mut frame.platforms, arguments(args)),
        "source" => frame.source = Some(first_string(args).map_or((SourceClass::Unknown, None), registry)),
        "git" | "github" => frame.source = Some((SourceClass::Git, None)),
        "path" => frame.source = Some((SourceClass::Path, None)),
        _ => frame.conditional = true,
    }
    frame
}

/// Names among the positional arguments of a DSL call such as
/// `group :development, :test, optional: true`. Every statement copies its
/// frame, so the names stay few.
fn arguments(args: &str) -> Vec<String> {
    let args = args.trim().trim_start_matches('(').trim_end_matches(')');
    split_top(args).into_iter().filter(|a| !OPTION.is_match(a)).flat_map(names).take(16).collect()
}

/// Adds the names of a nested block once, keeping the list short.
fn extend(list: &mut Vec<String>, names: Vec<String>) {
    for name in names {
        if list.len() < 32 && !list.contains(&name) {
            list.push(name);
        }
    }
}

fn first_string(text: &str) -> Option<&str> {
    QUOTED.captures(text).and_then(|c| c.get(1).or(c.get(2))).map(|m| m.as_str())
}

fn registry(url: &str) -> Source {
    classify_url(url, PUBLIC)
}

/// The text of a `"…"` or `'…'` literal, also with `.freeze`.
fn literal(text: &str) -> Option<&str> {
    let text = text.trim().trim_end_matches(".freeze");
    let quote = text.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    text[1..].strip_suffix(quote).filter(|inner| !inner.contains(quote))
}

fn gem(name: &str, plugin: bool, rest: &str, mut frame: Frame, git_sources: &[String]) -> Gem {
    let (rest, modifier) = match MODIFIER.find(rest) {
        Some(found) => (&rest[..found.start()], true),
        None => (rest, false),
    };
    let mut rest = rest.trim().trim_start_matches(',').trim();
    if balance(rest) < 0 {
        rest = rest.strip_suffix(')').unwrap_or(rest); // `gem("name", …)`
    }
    frame.conditional |= modifier;
    let mut gem = Gem { name: name.into(), plugin, requirements: Vec::new(), variable: false, frame };
    let mut options = false;
    for argument in split_top(rest) {
        if let Some(c) = OPTION.captures(argument) {
            options = true;
            let value = c.get(2).map_or("", |m| m.as_str());
            let frame = &mut gem.frame;
            match &c[1] {
                "group" | "groups" => frame.groups.extend(names(value)),
                "platform" | "platforms" => frame.platforms.extend(names(value)),
                "path" => frame.source = Some((SourceClass::Path, None)),
                "git" | "github" | "gist" | "bitbucket" => frame.source = Some((SourceClass::Git, None)),
                "source" => frame.source = Some(first_string(value).map_or((SourceClass::Unknown, None), registry)),
                "install_if" => frame.conditional = true,
                key if git_sources.iter().any(|g| g == key) => frame.source = Some((SourceClass::Git, None)),
                _ => {}
            }
        } else if !options {
            match literal(argument) {
                Some(requirement) if !requirement.contains("#{") => gem.requirements.push(requirement.to_string()),
                _ => gem.variable = true,
            }
        }
    }
    gem
}

fn gem_item(file: &FoundFile<'_>, gem: Gem, global: &Source) -> InventoryItem {
    let Gem { name, plugin, requirements, variable, frame } = gem;
    let (source, host) = frame.source.unwrap_or_else(|| global.clone());
    let mut groups = frame.groups;
    groups.sort();
    groups.dedup();
    let mut platforms = frame.platforms;
    platforms.sort();
    platforms.dedup();
    let mut detail = if variable {
        "Version aus Variable".to_string()
    } else if requirements.is_empty() {
        "ohne Versionsangabe".to_string()
    } else {
        format!("Spezifikation {}", requirements.join(", "))
    };
    if plugin {
        detail.push_str(" · Bundler-Plugin");
    }
    if !groups.is_empty() {
        detail.push_str(&format!(" · Gruppe {}", groups.join(", ")));
    }
    if !platforms.is_empty() {
        detail.push_str(&format!(" · Plattformen {}", platforms.join(", ")));
    }
    detail.push_str(match source {
        SourceClass::Path => " · lokaler Pfad",
        SourceClass::Git => " · Git",
        SourceClass::PrivateRegistry => " · eigene Quelle",
        _ => "",
    });
    if frame.conditional {
        detail.push_str(" · bedingt");
    }
    let mut item = InventoryItem::new(ItemCategory::Package, "bundler", name, file.rel_path())
        .version(exact(&requirements).unwrap_or_default())
        .state(InstallState::Declared)
        .source(source, host)
        .detail(detail);
    item.evidence.context = if plugin { "plugin".to_string() } else { groups.join(",") };
    item.evidence.qualifier = platforms.join(",");
    item
}

/// The version of a single exact requirement: `"1.2.3"` or `"= 1.2.3"`.
fn exact<S: AsRef<str>>(requirements: &[S]) -> Option<&str> {
    let [only] = requirements else { return None };
    let only = only.as_ref();
    let version = only.strip_prefix('=').unwrap_or(only).trim();
    let valid = version.starts_with(|c: char| c.is_ascii_digit())
        && version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
    valid.then_some(version)
}

fn gemspec(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let mut items = Vec::new();
    let mut name = None;
    for line in file.content.lines().take(20_000) {
        let code = code(line);
        if name.is_none() {
            name = SPEC_NAME.captures(code).map(|c| c[1].to_string());
        }
        let Some(c) = SPEC_DEPENDENCY.captures(code) else { continue };
        let Some(gem) = c.get(2).or(c.get(3)) else { continue };
        let development = c.get(1).is_some_and(|m| m.as_str() == "development_");
        let requirements: Vec<&str> = QUOTED
            .captures_iter(c.get(4).map_or("", |m| m.as_str()))
            .filter_map(|q| q.get(1).or(q.get(2)))
            .map(|m| m.as_str())
            .collect();
        let mut detail = if requirements.is_empty() {
            "ohne Versionsangabe".to_string()
        } else {
            format!("Spezifikation {}", requirements.join(", "))
        };
        if development {
            detail.push_str(" · dev");
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "bundler", gem.as_str(), file.rel_path())
            .version(exact(&requirements).unwrap_or_default())
            .state(InstallState::Declared)
            .detail(detail);
        item.evidence.context = if development { "dev" } else { "" }.into();
        items.push(item);
    }
    let detail = match name {
        Some(name) => format!("Gem-Spezifikation {name} (nicht ausgeführt)"),
        None => "Gem-Spezifikation (nicht ausgeführt)".to_string(),
    };
    items.insert(0, manifest("bundler", file, detail));
    items
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Other,
    Source,
    Dependencies,
    Platforms,
    Ruby,
    Bundled,
}

/// Where the dependency lines below a lockfile spec belong.
#[derive(Clone, Copy)]
enum Target {
    None,
    Own,
    Spec(usize),
}

struct Spec {
    name: String,
    version: String,
    platform: String,
    source: Source,
    origin: String,
    edges: Vec<PackageDependency>,
}

fn lockfile(file: &FoundFile<'_>) -> AdapterResult {
    let text = file.content;
    if ["<<<<<<<", "=======", ">>>>>>>", "|||||||"].iter().any(|marker| text.contains(marker)) {
        return Err(format!("{} enthält Konfliktmarker", file.name));
    }
    if text.trim().is_empty() {
        return Ok(vec![manifest("bundler", file, "Lockfile leer")]);
    }
    let mut section = Section::Other;
    let mut known = false;
    let (mut kind, mut remotes, mut reference) = ("", Vec::new(), None);
    let (mut source, mut origin): (Source, String) = ((SourceClass::Unknown, None), String::new());
    let mut own = false;
    let mut target = Target::None;
    let mut specs: Vec<Spec> = Vec::new();
    let mut own_edges = Vec::new();
    let mut direct: Vec<(&str, &str, bool)> = Vec::new();
    let mut platforms: Vec<&str> = Vec::new();
    let (mut ruby, mut bundled) = (None, None);
    for line in text.lines().take(500_000) {
        let line = line.trim_end();
        let content = line.trim_start_matches(' ');
        if content.is_empty() {
            continue;
        }
        let indent = line.len() - content.len();
        if indent == 0 {
            target = Target::None;
            section = match line {
                "GEM" | "GIT" | "PATH" | "PLUGIN SOURCE" => {
                    kind = line;
                    remotes.clear();
                    reference = None;
                    (source, origin, own) = ((SourceClass::Unknown, None), String::new(), false);
                    Section::Source
                }
                "DEPENDENCIES" => Section::Dependencies,
                "PLATFORMS" => Section::Platforms,
                "RUBY VERSION" => Section::Ruby,
                "BUNDLED WITH" => Section::Bundled,
                _ => Section::Other,
            };
            known |= section != Section::Other || matches!(line, "CHECKSUMS" | "CONTENT ADDRESSES");
            continue;
        }
        match section {
            Section::Source if indent == 2 => {
                if content == "specs:" {
                    (source, origin) = lock_source(kind, &remotes, reference);
                    own = kind == "PATH" && remotes == ["."];
                } else if let Some((key, value)) = content.split_once(": ") {
                    match key {
                        "remote" => remotes.push(value),
                        "tag" | "branch" => reference = Some((key, value)),
                        _ => {}
                    }
                }
            }
            Section::Source if indent == 4 => {
                target = match spec_line(content) {
                    Some(_) if own => Target::Own,
                    Some((name, version, platform)) => {
                        specs.push(Spec {
                            name: name.into(),
                            version: version.into(),
                            platform: platform.into(),
                            source: source.clone(),
                            origin: origin.clone(),
                            edges: Vec::new(),
                        });
                        Target::Spec(specs.len() - 1)
                    }
                    None => Target::None,
                };
            }
            Section::Source if indent == 6 => match target {
                Target::Own => own_edges.push(PackageDependency { kind: "gemspec".into(), ..lock_edge(content) }),
                Target::Spec(index) => specs[index].edges.push(lock_edge(content)),
                Target::None => {}
            },
            Section::Dependencies if indent == 2 => direct.push(lock_dependency(content)),
            Section::Platforms => platforms.push(content),
            Section::Ruby => ruby = Some(content),
            Section::Bundled => bundled = Some(content),
            _ => {}
        }
    }
    if !known {
        return Err(format!("{} ist kein Bundler-Lockfile", file.name));
    }
    let mut detail = String::from("Lockfile");
    if !platforms.is_empty() {
        detail.push_str(&format!(" · Plattformen {}", platforms.join(", ")));
    }
    if let Some(ruby) = ruby {
        detail.push_str(&format!(" · Ruby {}", ruby.trim_start_matches("ruby ")));
    }
    if let Some(bundled) = bundled {
        detail.push_str(&format!(" · Bundler {bundled}"));
    }
    let mut items = vec![manifest("bundler", file, detail)];
    items[0].evidence.dependencies = direct
        .iter()
        .map(|(name, requirement, _)| PackageDependency {
            name: (*name).into(),
            requirement: (*requirement).into(),
            kind: "DEPENDENCIES".into(),
            exact: false,
        })
        .chain(own_edges)
        .collect();
    let own_source: std::collections::HashMap<&str, bool> = direct.iter().map(|(name, _, own)| (*name, *own)).collect();
    for spec in specs {
        let mut detail = format!("aufgelöst laut {}", file.name);
        if !spec.platform.is_empty() {
            detail.push_str(&format!(" · Plattform {}", spec.platform));
        }
        detail.push_str(&spec.origin);
        match own_source.get(spec.name.as_str()) {
            Some(true) => detail.push_str(" · direkt, eigene Quelle"),
            Some(false) => detail.push_str(" · direkt"),
            None => {}
        }
        let (class, host) = spec.source;
        let mut item = InventoryItem::new(ItemCategory::Package, "bundler", spec.name, file.rel_path())
            .version(spec.version)
            .state(InstallState::Resolved)
            .source(class, host)
            .detail(detail);
        item.evidence.qualifier = spec.platform;
        item.evidence.dependencies = spec.edges;
        items.push(item);
    }
    bounded(items)
}

/// Source and detail suffix of a lockfile section. A GEM section with
/// several remotes (Bundler 1) cannot tell which spec came from where.
fn lock_source(kind: &str, remotes: &[&str], reference: Option<(&str, &str)>) -> (Source, String) {
    match kind {
        "GIT" => {
            let origin = match reference {
                Some(("tag", tag)) => format!(" · Git-Tag {tag}"),
                Some((_, branch)) => format!(" · Git-Zweig {branch}"),
                None => " · Git".to_string(),
            };
            ((SourceClass::Git, None), origin)
        }
        "PATH" => ((SourceClass::Path, None), " · lokaler Pfad".to_string()),
        "GEM" => {
            let sources: Vec<Source> = remotes.iter().map(|remote| registry(remote)).collect();
            match sources.as_slice() {
                [single] => (single.clone(), String::new()),
                [first, ..] if sources.iter().all(|(class, _)| *class == SourceClass::PublicRegistry) => {
                    (first.clone(), String::new())
                }
                [] => ((SourceClass::Unknown, None), String::new()),
                _ => ((SourceClass::Unknown, None), " · mehrere Quellen".to_string()),
            }
        }
        _ => ((SourceClass::Unknown, None), " · Plugin-Quelle".to_string()),
    }
}

/// `name (1.2.3)` or `name (1.2.3-x86_64-linux)`: the version ends at the
/// first dash, as Bundler reads it.
fn spec_line(text: &str) -> Option<(&str, &str, &str)> {
    let (name, rest) = text.split_once(" (")?;
    let (inside, _) = rest.split_once(')')?;
    let (version, platform) = inside.split_once('-').unwrap_or((inside, ""));
    (!name.is_empty() && !version.is_empty()).then_some((name, version, platform))
}

/// `name`, `name (>= 1.0, < 2)`, optionally with `!` for a gem from a
/// source of its own.
fn lock_dependency(text: &str) -> (&str, &str, bool) {
    let pinned = text.ends_with('!');
    let text = text.trim_end_matches('!');
    match text.split_once(" (") {
        Some((name, requirement)) => (name, requirement.trim_end_matches(')'), pinned),
        None => (text, "", pinned),
    }
}

fn lock_edge(text: &str) -> PackageDependency {
    let (name, requirement, _) = lock_dependency(text);
    let exact = requirement.strip_prefix("= ").filter(|v| !v.contains(','));
    PackageDependency {
        name: name.into(),
        requirement: exact.unwrap_or(requirement).into(),
        kind: "lock".into(),
        exact: exact.is_some(),
    }
}

/// `nokogiri-1.16.0-x86_64-linux` → name, version and platform. The name
/// ends at the first dash before a version whose remainder, if any, starts
/// with a letter: `devise-2fa-1.0.0`, `sorbet-static-0.5.1-universal-darwin-20`.
fn split_gem_dir(dir: &str) -> Option<(&str, &str, &str)> {
    dir.match_indices('-').map(|(index, _)| index).filter(|index| *index > 0).find_map(|index| {
        let rest = &dir[index + 1..];
        let (version, platform) = rest.split_once('-').unwrap_or((rest, ""));
        let numeric = version.starts_with(|c: char| c.is_ascii_digit())
            && version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
        let platform_ok = platform.is_empty() || platform.starts_with(|c: char| c.is_ascii_alphabetic());
        (numeric && platform_ok).then(|| (&dir[..index], version, platform))
    })
}

fn revision(text: &str) -> bool {
    (7..=40).contains(&text.len()) && text.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const GEMFILE: &str = r#"source "https://rubygems.org"
git_source(:stash) { |repo| "https://stash.corp.example/#{repo}.git" }

ruby "3.2.2"

# Bundle edge Rails instead: gem "rails", github: "rails/rails", branch: "main"
gem "rails", "~> 7.1.2"
gem "pg", "~> 1.1"
gem "nokogiri", ">= 1.6", "< 2.0"
gem "tzinfo-data", platforms: %i[ windows jruby ]
gem "bootsnap", require: false # Reduces boot times through caching
gem "rspec", :groups => [:development, :test]
gem("sidekiq-pro", source: "https://deploy:s3cr3tpass@gems.contribsys.com/")
gem "my_engine", path: "engines/my_engine"
gem "kaminari", git: "https://x-access-token:s3cr3tpass@github.com/kaminari/kaminari.git", branch: "master"
gem "internal_tool", stash: "forks/internal_tool"
gem "exact", "1.2.3"
gem "dynamic", RAILS_VERSION
gem "pasteboard" if RUBY_PLATFORM.include?("darwin")

group :development, :test do
  gem "debug", platforms: %i[ mri windows ]
  gem "rubocop-rails",
      require: false
end

group :development do
  gem "web-console"
  profiler = if ENV["PROFILE"]
    "stackprof"
  end
  platforms :mri do
    gem "stackprof"
  end
  gem "listen"
end

if ENV["CI"]
  gem "simplecov"
end

source "https://gems.example.com" do
  gem "private_gem"
end

git "https://github.com/rails/rails.git" do
  gem "railties"
end

install_if -> { RUBY_PLATFORM =~ /darwin/ } do
  gem "terminal-notifier"
end

gemspec
eval_gemfile "Gemfile.local"
plugin "bundler-graph"
gem "after_blocks"
"#;

    #[test]
    fn gemfile_groups_platforms_sources_and_blocks() {
        let items = Bundler.parse_file(&file("Gemfile", GEMFILE)).unwrap();
        assert_eq!(
            items[0].detail,
            "Gemfile (nicht ausgeführt) · 24 Gems · Quelle rubygems.org · Ruby 3.2.2 · gemspec eingebunden · 1 eval_gemfile nicht gefolgt"
        );
        assert_eq!(items.len(), 25);
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let rails = find("rails");
        assert_eq!(rails.detail, "Spezifikation ~> 7.1.2");
        assert_eq!((rails.source, rails.source_host.as_deref()), (SourceClass::PublicRegistry, Some("rubygems.org")));
        assert_eq!((rails.version.as_deref(), rails.evidence.context.as_str()), (None, ""));
        assert_eq!(find("nokogiri").detail, "Spezifikation >= 1.6, < 2.0");
        let tzinfo = find("tzinfo-data");
        assert_eq!(tzinfo.evidence.qualifier, "jruby,windows");
        assert!(tzinfo.detail.contains("Plattformen jruby, windows"));
        assert_eq!(find("bootsnap").detail, "ohne Versionsangabe");
        assert_eq!(find("rspec").evidence.context, "development,test");
        let sidekiq = find("sidekiq-pro");
        assert_eq!(
            (sidekiq.source, sidekiq.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("gems.contribsys.com"))
        );
        assert_eq!(find("my_engine").source, SourceClass::Path);
        assert_eq!((find("kaminari").source, find("kaminari").source_host.as_deref()), (SourceClass::Git, None));
        assert_eq!(find("internal_tool").source, SourceClass::Git, "custom git_source");
        assert_eq!(find("exact").version.as_deref(), Some("1.2.3"));
        assert!(find("dynamic").detail.starts_with("Version aus Variable"));
        assert!(find("pasteboard").detail.ends_with("bedingt"));
        let debug = find("debug");
        assert_eq!(
            (debug.evidence.context.as_str(), debug.evidence.qualifier.as_str()),
            ("development,test", "mri,windows")
        );
        assert_eq!(find("rubocop-rails").evidence.context, "development,test", "continued line");
        assert_eq!(find("web-console").evidence.context, "development");
        let stackprof = find("stackprof");
        assert_eq!(
            (stackprof.evidence.context.as_str(), stackprof.evidence.qualifier.as_str()),
            ("development", "mri")
        );
        assert_eq!(find("listen").evidence.context, "development", "`x = if … end` keeps the group open");
        assert!(find("simplecov").detail.ends_with("bedingt"));
        let private = find("private_gem");
        assert_eq!(
            (private.source, private.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("gems.example.com"))
        );
        assert_eq!(find("railties").source, SourceClass::Git);
        assert!(find("terminal-notifier").detail.ends_with("bedingt"));
        assert_eq!(find("bundler-graph").evidence.context, "plugin");
        let after = find("after_blocks");
        assert_eq!((after.evidence.context.as_str(), after.detail.as_str()), ("", "ohne Versionsangabe"));
        assert!(!format!("{items:?}").contains("s3cr3tpass"));
        assert!(!items.iter().any(|i| i.name == "ruby" || i.name == "Gemfile.local"));
    }

    #[test]
    fn gems_rb_only_when_it_is_a_gemfile() {
        let gems_rb = "source 'https://rubygems.org'\ngem 'rack', '~> 3.0'\n";
        let items = Bundler.parse_file(&file("gems.rb", gems_rb)).unwrap();
        assert_eq!(items.len(), 2);
        let foreign = "module Gems\n  def self.list\n    gem 'not-a-declaration'\n  end\nend\n";
        assert!(Bundler.parse_file(&file("gems.rb", foreign)).unwrap().is_empty());
        let siblings = ["gems.locked".to_string()];
        let found = FoundFile { name: "gems.rb", rel_dir: "", content: "gem 'rack'\n", siblings: &siblings };
        assert_eq!(Bundler.parse_file(&found).unwrap().len(), 2);
        assert!(Bundler.parse_file(&file("notes.gemspec", "just text")).unwrap().is_empty());
    }

    const LOCK: &str = "GIT
  remote: https://x-access-token:s3cr3tpass@github.com/kaminari/kaminari.git
  revision: 0123456789abcdef0123456789abcdef01234567
  branch: master
  specs:
    kaminari (1.2.2)
      activesupport (>= 4.1.0)
      kaminari-core (= 1.2.2)
    kaminari-core (1.2.2)

PATH
  remote: .
  specs:
    my_gem (0.1.0)
      rack (~> 3.0)

PATH
  remote: engines/my_engine
  specs:
    my_engine (0.0.1)
      rails (>= 7.0)

GEM
  remote: https://rubygems.org/
  specs:
    activesupport (7.1.2)
      concurrent-ruby (~> 1.0, >= 1.0.2)
      i18n (>= 1.6, < 2)
    concurrent-ruby (1.2.2)
    i18n (1.14.1)
      concurrent-ruby (~> 1.0)
    nokogiri (1.16.0-arm64-darwin)
      racc (~> 1.4)
    nokogiri (1.16.0-x86_64-linux)
      racc (~> 1.4)
    racc (1.7.3)
    rack (3.0.8)

GEM
  remote: https://deploy:s3cr3tpass@gems.contribsys.com/
  specs:
    sidekiq-pro (7.2.0)
      sidekiq (>= 7.2.0)

PLATFORMS
  arm64-darwin-23
  x86_64-linux

DEPENDENCIES
  kaminari!
  my_engine!
  my_gem!
  nokogiri (>= 1.6, < 2.0)
  sidekiq-pro!

CHECKSUMS
  activesupport (7.1.2) sha256=abc
  nokogiri (1.16.0-x86_64-linux) sha256=def

RUBY VERSION
   ruby 3.2.2p53

BUNDLED WITH
   2.5.4
";

    #[test]
    fn lockfile_sources_platforms_and_edges() {
        let items = Bundler.parse_file(&file("Gemfile.lock", LOCK)).unwrap();
        assert_eq!(
            items[0].detail,
            "Lockfile · Plattformen arm64-darwin-23, x86_64-linux · Ruby 3.2.2p53 · Bundler 2.5.4"
        );
        assert_eq!(items.len(), 12, "the project's own gem (PATH .) is no dependency");
        assert!(!items.iter().any(|i| i.name == "my_gem"));
        let edges = &items[0].evidence.dependencies;
        assert_eq!(edges.len(), 6);
        assert!(edges.iter().any(|d| d.name == "rack" && d.kind == "gemspec"));
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Resolved)));
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let kaminari = find("kaminari");
        assert_eq!((kaminari.source, kaminari.version.as_deref()), (SourceClass::Git, Some("1.2.2")));
        assert_eq!(kaminari.detail, "aufgelöst laut Gemfile.lock · Git-Zweig master · direkt, eigene Quelle");
        let core = &kaminari.evidence.dependencies[1];
        assert_eq!((core.name.as_str(), core.requirement.as_str(), core.exact), ("kaminari-core", "1.2.2", true));
        assert_eq!(find("my_engine").source, SourceClass::Path);
        let nokogiri: Vec<&InventoryItem> = items.iter().filter(|i| i.name == "nokogiri").collect();
        assert_eq!(nokogiri.len(), 2);
        assert_eq!(nokogiri[0].version.as_deref(), Some("1.16.0"));
        assert_eq!(nokogiri[1].evidence.qualifier, "x86_64-linux");
        assert!(nokogiri[0].detail.ends_with("Plattform arm64-darwin · direkt"), "{}", nokogiri[0].detail);
        let rack = find("rack");
        assert_eq!((rack.source, rack.source_host.as_deref()), (SourceClass::PublicRegistry, Some("rubygems.org")));
        let concurrent = &find("activesupport").evidence.dependencies[0];
        assert_eq!((concurrent.requirement.as_str(), concurrent.exact), ("~> 1.0, >= 1.0.2", false));
        let sidekiq = find("sidekiq-pro");
        assert_eq!(
            (sidekiq.source, sidekiq.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("gems.contribsys.com"))
        );
        assert!(!format!("{items:?}").contains("s3cr3tpass"));
    }

    #[test]
    fn older_lockfiles_merged_sources_and_broken_input() {
        let lock = "GEM\n  remote: https://rubygems.org/\n  remote: https://gems.corp.example/\n  specs:\n    rake (13.0.6)\n\nPLATFORMS\n  ruby\n\nDEPENDENCIES\n  rake\n";
        let items = Bundler.parse_file(&file("gems.locked", lock)).unwrap();
        assert_eq!(items[0].detail, "Lockfile · Plattformen ruby");
        assert_eq!(items[1].source, SourceClass::Unknown, "two remotes: origin of a spec is unknown");
        assert!(items[1].detail.contains("mehrere Quellen"));
        assert!(Bundler.parse_file(&file("Gemfile.lock", "GEM\n<<<<<<< HEAD\n  specs:\n")).is_err());
        assert!(Bundler.parse_file(&file("Gemfile.lock", "not a lockfile\n")).is_err());
        assert_eq!(Bundler.parse_file(&file("Gemfile.lock", "\n")).unwrap().len(), 1);
    }

    #[test]
    fn gemspec_dependencies_in_both_styles() {
        let spec = r#"# frozen_string_literal: true
require_relative "lib/my_gem/version"

Gem::Specification.new do |spec|
  spec.name = "my_gem"
  spec.version = MyGem::VERSION
  spec.required_ruby_version = ">= 3.0.0"
  spec.add_dependency "activesupport", ">= 6.1", "< 8"
  spec.add_runtime_dependency("rack", "~> 3.0")
  spec.add_development_dependency "rspec", "~> 3.12"
  # spec.add_dependency "commented", "1.0"
  s.add_runtime_dependency(%q<concurrent-ruby>.freeze, ["= 1.2.2".freeze])
end
"#;
        let items = Bundler.parse_file(&file("my_gem.gemspec", spec)).unwrap();
        assert_eq!(items[0].detail, "Gem-Spezifikation my_gem (nicht ausgeführt)");
        let names: Vec<&str> = items.iter().skip(1).map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["activesupport", "rack", "rspec", "concurrent-ruby"]);
        assert_eq!(items[1].detail, "Spezifikation >= 6.1, < 8");
        assert_eq!(
            (items[3].evidence.context.as_str(), items[3].detail.as_str()),
            ("dev", "Spezifikation ~> 3.12 · dev")
        );
        assert_eq!(items[4].version.as_deref(), Some("1.2.2"));
    }

    #[test]
    fn vendor_bundle_only_beside_a_lockfile() {
        let lock = ["Gemfile".to_string(), "Gemfile.lock".to_string()];
        assert!(Bundler.claim_dir("vendor", &lock, &["javascript".into()]).is_none());
        assert!(Bundler.claim_dir("vendor", &["Gemfile".into()], &["bundle".into()]).is_none());
        let claim = Bundler.claim_dir("vendor", &lock, &["bundle".into(), "javascript".into()]).unwrap();
        let mut dir = FakeDir::default();
        dir.dirs.insert("bundle".into(), vec![("ruby".into(), true), ("config".into(), false)]);
        dir.dirs.insert("bundle/ruby".into(), vec![("3.2.0".into(), true)]);
        dir.dirs.insert(
            "bundle/ruby/3.2.0/gems".into(),
            vec![
                ("rack-3.0.8".into(), true),
                ("nokogiri-1.16.0-x86_64-linux".into(), true),
                ("sorbet-static-0.5.11000-universal-darwin-20".into(), true),
                ("devise-2fa-1.0.0".into(), true),
                ("README".into(), false),
            ],
        );
        dir.dirs.insert("bundle/ruby/3.2.0/bundler/gems".into(), vec![("kaminari-0123456789ab".into(), true)]);
        let items = Bundler.inspect_dir(claim, "vendor", &mut dir).unwrap();
        assert_eq!(items.len(), 6);
        let installed: Vec<(&str, Option<&str>)> =
            items.iter().skip(1).map(|i| (i.name.as_str(), i.version.as_deref())).collect();
        assert_eq!(
            installed,
            [
                ("rack", Some("3.0.8")),
                ("nokogiri", Some("1.16.0")),
                ("sorbet-static", Some("0.5.11000")),
                ("devise-2fa", Some("1.0.0")),
                ("kaminari", None),
            ]
        );
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Installed)));
        assert_eq!(items[5].source, SourceClass::Git);
        assert_eq!(items[1].rel_path, "vendor/bundle/ruby/3.2.0/gems");
        assert!(items[2].detail.ends_with("Plattform x86_64-linux"));
    }

    /// Fails the test as soon as a file is opened.
    struct NoReads(FakeDir);

    impl DirAccess for NoReads {
        fn read_text(&mut self, path: &[&str], _max: u64) -> Option<String> {
            panic!("{path:?} must not be read")
        }
        fn list(&mut self, path: &[&str]) -> Vec<(String, bool)> {
            self.0.list(path)
        }
    }

    #[test]
    fn dot_bundle_is_claimed_but_never_read() {
        assert!(Bundler.claim_dir(".bundle", &["README.md".into()], &["config".into()]).is_none());
        let claim = Bundler.claim_dir(".bundle", &["Gemfile".into()], &["config".into(), "ruby".into()]).unwrap();
        let mut dir = FakeDir::default();
        dir.files.insert("config".into(), "BUNDLE_GEMS__CONTRIBSYS__COM: \"deploy:s3cr3tpass\"\n".into());
        dir.dirs.insert(String::new(), vec![("config".into(), false), ("ruby".into(), true), ("plugin".into(), true)]);
        dir.dirs.insert("ruby".into(), vec![("2.7.0".into(), true)]);
        dir.dirs.insert("plugin".into(), vec![("index".into(), false), ("gems".into(), true)]);
        dir.dirs.insert("ruby/2.7.0/gems".into(), vec![("rake-13.0.6".into(), true)]);
        let items = Bundler.inspect_dir(claim, ".bundle", &mut NoReads(dir)).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!((items[1].name.as_str(), items[1].rel_path.as_str()), ("rake", ".bundle/ruby/2.7.0/gems"));
        assert!(!format!("{items:?}").contains("s3cr3tpass"));
        let mut settings_only = FakeDir::default();
        settings_only.dirs.insert(String::new(), vec![("config".into(), false)]);
        let items = Bundler.inspect_dir(claim, ".bundle", &mut NoReads(settings_only)).unwrap();
        assert_eq!(items.len(), 1);
        assert!(!items[0].detail.contains("Gems"), "{}", items[0].detail);
    }
}
