//! Mix (Elixir): dependency tuples in mix.exs (declared), mix.lock (resolved
//! Hex and Git entries), `deps` (installed folder names, Hex versions from
//! hex_metadata.config) and `_build` (output). mix.exs is code: tuples are
//! matched as text, never evaluated, so dependencies built by functions or
//! variables stay unknown. Neither mix nor elixir is started and `~/.hex` is
//! not read, so Hex repositories other than hexpm keep no host.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use regex::Regex;
use std::sync::LazyLock;

pub struct Mix;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "mix",
    name: "Mix (Elixir)",
    markers: &["mix.exs", "mix.lock"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "mix.exs: deps-Tupel als Text",
        "mix.lock (hex, git)",
        "deps/ (Ordnernamen, Version aus hex_metadata.config)",
        "_build/ als Ausgabe",
    ],
    limits: "Kein mix- oder elixir-Aufruf; mix.exs wird nicht ausgewertet – Abhängigkeiten aus Funktionen oder Variablen bleiben offen; ~/.hex wird nicht gelesen, Hex-Repositories außer hexpm bleiben ohne Host.",
};

/// The head of a `defp deps` clause.
static DEPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*defp?\s+deps\b").expect("valid regex"));
/// `deps: [` written directly in the project keyword list.
static INLINE_DEPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bdeps:\s*\[").expect("valid regex"));
/// The start of a dependency tuple `{:name,`.
static TUPLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\{\s*:(?:"[^"\n]+"|[A-Za-z_]\w*[?!]?)\s*,"#).expect("valid regex"));
/// `@name "value"`: a module attribute holding a string.
static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*@([a-z_]\w*)\s+"([^"]*)"\s*$"#).expect("valid regex"));
static APP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bapp:\s*:([a-z_]\w*)").expect("valid regex"));
static ELIXIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\belixir:\s*"([^"]{1,40})""#).expect("valid regex"));
static UMBRELLA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bapps_path:\s*""#).expect("valid regex"));
/// An app inside an umbrella shares the root's lockfile and deps folder.
static UMBRELLA_APP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\b(?:lockfile|deps_path):\s*"\.\./\.\./"#).expect("valid regex"));
/// The version line of a Hex package's metadata (Erlang terms).
static HEX_VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\{<<"version">>,\s*<<"([0-9A-Za-z.+\-]{1,64})">>\}"#).expect("valid regex"));

impl Adapter for Mix {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("mix.lock") {
            return bounded(lock_items(file)?);
        }
        bounded(project(file))
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        if !siblings.iter().any(|s| s.eq_ignore_ascii_case("mix.exs")) {
            return None;
        }
        match name {
            "deps" => Some(DirClaim { adapter: "mix", category: ItemCategory::Environment, label: "deps (Mix)" }),
            "_build" => Some(DirClaim { adapter: "mix", category: ItemCategory::Output, label: "_build (Mix)" }),
            _ => None,
        }
    }

    fn inspect_dir(&self, claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        if claim.category == ItemCategory::Output {
            let environments: Vec<String> = access
                .list(&[])
                .into_iter()
                .filter(|(n, d)| *d && !n.starts_with('.'))
                .map(|(n, _)| n)
                .take(6)
                .collect();
            let detail = if environments.is_empty() {
                "Generierte Buildausgabe; Größe erst nach Messung bekannt".to_string()
            } else {
                format!("Generierte Buildausgabe ({}); Größe erst nach Messung bekannt", environments.join(", "))
            };
            return Ok(vec![InventoryItem::new(ItemCategory::Output, "mix", "_build", rel_path).detail(detail)]);
        }
        let mut packages = Vec::new();
        for (name, is_dir) in access.list(&[]) {
            if !is_dir || name.starts_with('.') {
                continue;
            }
            let version = access
                .read_text(&[name.as_str(), "hex_metadata.config"], 256 * 1024)
                .and_then(|text| HEX_VERSION.captures(&text).map(|c| c[1].to_string()));
            let detail = match version {
                Some(_) => "installiert laut hex_metadata.config",
                None => "Ordner vorhanden; ohne hex_metadata.config Version unbekannt",
            };
            packages.push(
                InventoryItem::new(ItemCategory::Package, "mix", name, rel_path)
                    .version(version.unwrap_or_default())
                    .state(InstallState::Installed)
                    .detail(detail),
            );
        }
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, "mix", "deps", rel_path)
                .detail(format!("Abhängigkeiten von Mix · {} Ordner; nicht übersetzt", packages.len())),
        ];
        items.extend(packages);
        bounded(items)
    }
}

fn project(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let lines: Vec<&str> = file.content.lines().take(50_000).map(code).collect();
    let attributes: Vec<(&str, &str)> = lines
        .iter()
        .filter_map(|line| ATTRIBUTE.captures(line))
        .filter_map(|c| Some((c.get(1)?.as_str(), c.get(2)?.as_str())))
        .collect();
    let text = lines.join("\n");
    let region = deps_region(&lines).or_else(|| inline_deps(&text)).unwrap_or_default();
    let mut items = Vec::new();
    let mut position = 0;
    while let Some(found) = TUPLE.find_at(&region, position) {
        // An unclosed tuple ends the search; a later one would rescan the rest.
        let Some(end) = closing(&region, found.start()) else { break };
        items.extend(dependency(file, &region[found.start() + 1..end], &attributes));
        position = end + 1;
        if items.len() >= 5_000 {
            break;
        }
    }
    let mut detail = match APP.captures(&text) {
        Some(app) => format!("Mix-Projekt {} (nicht ausgeführt)", &app[1]),
        None => "Mix-Projekt (nicht ausgeführt)".to_string(),
    };
    if UMBRELLA.is_match(&text) {
        detail.push_str(" · Umbrella-Projekt");
    }
    if UMBRELLA_APP.is_match(&text) {
        detail.push_str(" · App eines Umbrella-Projekts");
    }
    if let Some(elixir) = ELIXIR.captures(&text) {
        detail.push_str(&format!(" · Elixir {}", &elixir[1]));
    }
    items.insert(0, manifest("mix", file, detail));
    items
}

/// Bodies of `defp deps` clauses: the lines after the head up to the next
/// line indented no deeper than the head (its `end` or the next function).
fn deps_region(lines: &[&str]) -> Option<String> {
    let mut region = String::new();
    let mut found = false;
    let mut head: Option<usize> = None;
    for line in lines {
        let indent = line.len() - line.trim_start().len();
        if let Some(level) = head {
            if line.trim().is_empty() || indent > level {
                region.push_str(line);
                region.push('\n');
                continue;
            }
            head = None;
        }
        if let Some(found_head) = DEPS.find(line) {
            found = true;
            head = Some(indent);
            region.push_str(&line[found_head.end()..]);
            region.push('\n');
        }
    }
    found.then_some(region)
}

fn inline_deps(text: &str) -> Option<String> {
    let start = INLINE_DEPS.find(text)?.end() - 1;
    let end = closing(text, start)?;
    Some(text[start..=end].to_string())
}

/// One `{:name, requirement, options}` tuple (without its braces).
fn dependency(file: &FoundFile<'_>, inner: &str, attributes: &[(&str, &str)]) -> Option<InventoryItem> {
    let parts = split_top(inner);
    let app = atom(parts.first()?)?;
    let mut requirement = None;
    let (mut variable, mut pattern) = (false, false);
    let mut options: Vec<(&str, &str)> = Vec::new();
    for part in parts.iter().skip(1) {
        if let Some(list) = part.strip_prefix('[').and_then(|p| p.strip_suffix(']')) {
            options.extend(split_top(list).into_iter().filter_map(keyword));
        } else if let Some(pair) = keyword(part) {
            options.push(pair);
        } else if part.starts_with("~r") {
            pattern = true;
        } else if options.is_empty() && requirement.is_none() && !variable {
            requirement = string(part).filter(|s| !s.contains("#{")).or_else(|| {
                let name = part.strip_prefix('@')?;
                attributes.iter().find(|(key, _)| *key == name).map(|(_, value)| *value)
            });
            variable = requirement.is_none();
        }
    }
    let option = |key: &str| options.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    let truthy = |key: &str| option(key) == Some("true");
    let name = option("hex").and_then(|v| atom(v).or_else(|| string(v))).unwrap_or(app);
    let (source, host, origin) = if option("path").is_some() {
        (SourceClass::Path, None, " · lokaler Pfad".to_string())
    } else if truthy("in_umbrella") {
        (SourceClass::Path, None, " · Umbrella-App".to_string())
    } else if option("git").is_some() || option("github").is_some() {
        (SourceClass::Git, None, " · Git".to_string())
    } else if let Some(organization) = option("organization").and_then(string) {
        hex_repo(&format!("hexpm:{organization}"))
    } else if let Some(repo) = option("repo").and_then(string) {
        hex_repo(repo)
    } else {
        (SourceClass::Unknown, None, String::new())
    };
    let mut detail = match requirement {
        Some(requirement) => format!("Spezifikation {requirement}"),
        None if variable => "Version aus Variable".to_string(),
        None if pattern => "Versionsmuster (regulärer Ausdruck, nicht ausgewertet)".to_string(),
        None => "ohne Versionsangabe".to_string(),
    };
    detail.push_str(&origin);
    let mut only = option("only").map(names).unwrap_or_default();
    only.sort();
    only.dedup();
    if !only.is_empty() {
        detail.push_str(&format!(" · nur {}", only.join(", ")));
    }
    if option("runtime") == Some("false") {
        detail.push_str(" · nicht zur Laufzeit");
    }
    if truthy("optional") {
        detail.push_str(" · optional");
    }
    if truthy("override") {
        detail.push_str(" · override");
    }
    let mut item = InventoryItem::new(ItemCategory::Package, "mix", name, file.rel_path())
        .version(requirement.and_then(exact_version).unwrap_or_default())
        .state(InstallState::Declared)
        .source(source, host)
        .detail(detail);
    item.evidence.context = only.join(",");
    item.evidence.qualifier = option("targets").map(names).unwrap_or_default().join(",");
    Some(item)
}

/// `hexpm` is the public hex.pm, `hexpm:org` an organization there and any
/// other name a repository configured outside the project.
fn hex_repo(repo: &str) -> (SourceClass, Option<String>, String) {
    match repo.strip_prefix("hexpm") {
        Some("") => (SourceClass::PublicRegistry, Some("hex.pm".into()), String::new()),
        Some(organization) if organization.starts_with(':') => {
            (SourceClass::PrivateRegistry, Some("hex.pm".into()), format!(" · Hex-Organisation {}", &organization[1..]))
        }
        _ => (SourceClass::PrivateRegistry, None, format!(" · Hex-Repository {repo}")),
    }
}

fn lock_items(file: &FoundFile<'_>) -> AdapterResult {
    let text = file.content.trim();
    if ["<<<<<<<", "=======", ">>>>>>>"].iter().any(|marker| text.contains(marker)) {
        return Err("mix.lock enthält Konfliktmarker".into());
    }
    if text.is_empty() {
        return Ok(vec![manifest("mix", file, "Lockfile leer")]);
    }
    let mut rest = text.strip_prefix("%{").and_then(|t| t.strip_suffix('}')).ok_or("mix.lock ist keine Elixir-Map")?;
    let mut items = Vec::new();
    let (mut hex, mut git) = (0, 0);
    while items.len() <= crate::limits::ITEMS_PER_FILE {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.is_empty() {
            break;
        }
        let (key, after) = lock_key(rest).ok_or("mix.lock: Schlüssel erwartet")?;
        let after = after.trim_start();
        if !after.starts_with('{') {
            return Err("mix.lock: Tupel erwartet".into());
        }
        let end = closing(after, 0).ok_or("mix.lock: Tupel nicht geschlossen")?;
        let parts = split_top(&after[1..end]);
        match parts.first().copied().and_then(atom) {
            Some("hex") => hex += 1,
            Some("git") => git += 1,
            _ => {}
        }
        items.push(lock_entry(file, key, &parts));
        rest = &after[end + 1..];
    }
    items.insert(0, manifest("mix", file, format!("Lockfile · {hex} Hex-Pakete · {git} Git-Abhängigkeiten")));
    Ok(items)
}

/// `"name":` (or `name:`) at the start of `text`, and the text after it.
fn lock_key(text: &str) -> Option<(&str, &str)> {
    if let Some(quoted) = text.strip_prefix('"') {
        let (key, after) = quoted.split_once('"')?;
        return Some((key, after.strip_prefix(':')?));
    }
    let (key, after) = text.split_once(':')?;
    (!key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some((key, after))
}

/// `{:hex, :name, "1.0.0", inner_checksum, managers, deps, repo, checksum}`,
/// `{:git, url, revision, options}` or another SCM. A Git URL is never kept.
fn lock_entry(file: &FoundFile<'_>, key: &str, parts: &[&str]) -> InventoryItem {
    let resolved = |name: &str| {
        InventoryItem::new(ItemCategory::Package, "mix", name, file.rel_path()).state(InstallState::Resolved)
    };
    match parts.first().copied().and_then(atom) {
        Some("hex") => {
            let name = parts.get(1).copied().and_then(atom).unwrap_or(key);
            let repo = parts.get(6).copied().and_then(string).unwrap_or("hexpm");
            let (source, host, origin) = hex_repo(repo);
            let mut item = resolved(name)
                .version(parts.get(2).copied().and_then(string).unwrap_or_default())
                .source(source, host)
                .detail(format!("aufgelöst laut mix.lock{origin}"));
            if name != key {
                item.evidence.qualifier = key.into();
            }
            item.evidence.dependencies = parts.get(5).copied().map(lock_edges).unwrap_or_default();
            item
        }
        Some("git") => {
            let options = parts.get(3).and_then(|o| o.strip_prefix('[')?.strip_suffix(']')).map(split_top);
            let reference = options.into_iter().flatten().filter_map(keyword).find_map(|(k, v)| match k {
                "tag" => Some(format!(" · Tag {}", string(v)?)),
                "branch" => Some(format!(" · Zweig {}", string(v)?)),
                _ => None,
            });
            resolved(key)
                .source(SourceClass::Git, None)
                .detail(format!("aufgelöst laut mix.lock · Git{}", reference.unwrap_or_default()))
        }
        Some("path") => resolved(key).source(SourceClass::Path, None).detail("aufgelöst laut mix.lock · lokaler Pfad"),
        other => resolved(key).detail(format!("aufgelöst laut mix.lock · Quelle {}", other.unwrap_or("unbekannt"))),
    }
}

/// `[{:app, "requirement", [hex: :name, repo: "hexpm", optional: false]}, …]`.
fn lock_edges(list: &str) -> Vec<PackageDependency> {
    let Some(inner) = list.strip_prefix('[').and_then(|l| l.strip_suffix(']')) else { return Vec::new() };
    split_top(inner)
        .into_iter()
        .filter_map(|entry| {
            let parts = split_top(entry.strip_prefix('{')?.strip_suffix('}')?);
            let app = atom(parts.first()?)?;
            let requirement = parts.get(1).copied().and_then(string).unwrap_or_default();
            let options: Vec<(&str, &str)> = parts
                .get(2)
                .and_then(|o| o.strip_prefix('[')?.strip_suffix(']'))
                .map(split_top)
                .unwrap_or_default()
                .into_iter()
                .filter_map(keyword)
                .collect();
            let hex = options.iter().find(|(k, _)| *k == "hex").and_then(|(_, v)| atom(v).or_else(|| string(v)));
            let optional = options.iter().any(|(k, v)| *k == "optional" && *v == "true");
            let exact = exact_version(requirement);
            Some(PackageDependency {
                name: hex.unwrap_or(app).into(),
                requirement: exact.unwrap_or(requirement).into(),
                kind: if optional { "lock, optional" } else { "lock" }.into(),
                exact: exact.is_some(),
            })
        })
        .collect()
}

/// A requirement naming one version: `1.2.3` or `== 1.2.3`.
fn exact_version(requirement: &str) -> Option<&str> {
    let version = requirement.strip_prefix("==").unwrap_or(requirement).trim();
    let valid = version.starts_with(|c: char| c.is_ascii_digit())
        && version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    valid.then_some(version)
}

/// `:name` or `:"name"`.
fn atom(text: &str) -> Option<&str> {
    let rest = text.trim().strip_prefix(':')?;
    let name = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(rest);
    let valid =
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '?' | '!'));
    valid.then_some(name)
}

/// A plain `"…"` literal.
fn string(text: &str) -> Option<&str> {
    text.trim().strip_prefix('"')?.strip_suffix('"').filter(|s| !s.contains('"'))
}

/// `key: value` of a keyword list.
fn keyword(part: &str) -> Option<(&str, &str)> {
    let (key, value) = part.split_once(':')?;
    let valid = !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && value.starts_with(char::is_whitespace);
    valid.then(|| (key, value.trim()))
}

/// Characters outside `"…"` and `'…'` literals (backslash escapes
/// respected); the quotes themselves are left out.
fn unquoted(text: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut quote = None;
    let mut escaped = false;
    text.char_indices().filter(move |&(_, ch)| {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == open {
                quote = None;
            }
            return false;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            return false;
        }
        true
    })
}

/// The code of a Ruby or Elixir line: the text before a `#` comment that
/// does not lie inside a string.
pub(super) fn code(line: &str) -> &str {
    match unquoted(line).find(|&(_, ch)| ch == '#') {
        Some((index, _)) => &line[..index],
        None => line,
    }
}

/// Brackets opened minus brackets closed, outside strings.
pub(super) fn balance(text: &str) -> i64 {
    unquoted(text)
        .map(|(_, ch)| match ch {
            '(' | '[' | '{' => 1,
            ')' | ']' | '}' => -1,
            _ => 0,
        })
        .sum()
}

/// Index of the bracket closing the one at `start`; nesting is bounded.
pub(super) fn closing(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, ch) in unquoted(&text[start..]) {
        match ch {
            '(' | '[' | '{' => {
                depth += 1;
                if depth > 32 {
                    return None;
                }
            }
            ')' | ']' | '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(start + index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits at commas outside strings and brackets.
pub(super) fn split_top(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (index, ch) in unquoted(text) {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts.retain(|part| !part.is_empty());
    parts
}

/// Names in `:dev`, `"dev"`, `[:dev, :test]`, Ruby's `%i[dev test]` or
/// Elixir's `~w(dev test)a`.
pub(super) fn names(value: &str) -> Vec<String> {
    let value = value.trim();
    let value = match value.strip_prefix("~w") {
        Some(sigil) => sigil.trim_end_matches(|c: char| c.is_ascii_alphabetic()),
        None => ["%i", "%I", "%w", "%W"].iter().find_map(|prefix| value.strip_prefix(prefix)).unwrap_or(value),
    };
    value
        .split(|c: char| c.is_whitespace() || ",[](){}".contains(c))
        .map(|word| word.trim_matches([':', '"', '\'']))
        .filter(|word| !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .take(16)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const MIX_EXS: &str = r#"defmodule Demo.MixProject do
  use Mix.Project

  @phoenix_version "~> 1.7.10"

  def project do
    [
      app: :demo,
      version: "0.1.0",
      elixir: "~> 1.15",
      start_permanent: Mix.env() == :prod,
      aliases: aliases(),
      deps: deps()
    ]
  end

  def application do
    [mod: {Demo.Application, []}, extra_applications: [:logger, :runtime_tools]]
  end

  # Specifies your project dependencies.
  # Type `mix help deps` for examples and options.
  defp deps do
    [
      {:phoenix, @phoenix_version},
      {:phoenix_ecto, "~> 4.4"},
      {:postgrex, ">= 0.0.0"},
      {:phoenix_live_reload, "~> 1.2", only: :dev},
      {:floki, ">= 0.30.0", only: :test},
      {:credo, "~> 1.7", only: [:test, :dev], runtime: false},
      # {:commented_out, "~> 1.0"},
      {:heroicons,
       github: "tailwindlabs/heroicons",
       tag: "v2.1.1",
       sparse: "optimized",
       app: false,
       compile: false,
       depth: 1},
      {:gettext, git: "https://deploy:s3cr3tpass@git.corp.example/gettext.git", tag: "0.1"},
      {:local_dependency, path: "path/to/local_dependency"},
      {:sibling, in_umbrella: true},
      {:private_pkg, "~> 1.0", organization: "acme"},
      {:mirror_pkg, "~> 2.0", repo: "my_repo"},
      {:renamed, "1.2.3", hex: :actual_package},
      {:nerves_runtime, "~> 0.13", targets: [:rpi4]},
      {:jason, "~> 1.4", [optional: true, override: true]}
    ]
  end

  defp aliases do
    [setup: ["deps.get", "ecto.setup"], test: ["ecto.create --quiet", "test"]]
  end
end
"#;

    #[test]
    fn mix_exs_tuples_with_options_sources_and_attributes() {
        let items = Mix.parse_file(&file("mix.exs", MIX_EXS)).unwrap();
        assert_eq!(items[0].detail, "Mix-Projekt demo (nicht ausgeführt) · Elixir ~> 1.15");
        assert_eq!(items.len(), 16, "{items:#?}");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(find("phoenix").detail, "Spezifikation ~> 1.7.10");
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        let live = find("phoenix_live_reload");
        assert_eq!((live.evidence.context.as_str(), live.source), ("dev", SourceClass::Unknown));
        let credo = find("credo");
        assert_eq!(credo.evidence.context, "dev,test");
        assert_eq!(credo.detail, "Spezifikation ~> 1.7 · nur dev, test · nicht zur Laufzeit");
        assert_eq!(find("heroicons").source, SourceClass::Git);
        let gettext = find("gettext");
        assert_eq!((gettext.source, gettext.source_host.as_deref()), (SourceClass::Git, None));
        assert_eq!(find("local_dependency").source, SourceClass::Path);
        let sibling = find("sibling");
        assert!(sibling.source == SourceClass::Path && sibling.detail.ends_with("Umbrella-App"));
        let private = find("private_pkg");
        assert_eq!((private.source, private.source_host.as_deref()), (SourceClass::PrivateRegistry, Some("hex.pm")));
        assert!(private.detail.contains("Hex-Organisation acme"));
        let mirror = find("mirror_pkg");
        assert_eq!((mirror.source, mirror.source_host.as_deref()), (SourceClass::PrivateRegistry, None));
        let renamed = find("actual_package");
        assert_eq!(renamed.version.as_deref(), Some("1.2.3"));
        assert_eq!(find("nerves_runtime").evidence.qualifier, "rpi4");
        let jason = find("jason");
        assert!(jason.detail.ends_with("optional · override"), "{}", jason.detail);
        assert!(!items.iter().any(|i| i.name == "commented_out" || i.name.starts_with("Demo")));
        assert!(!format!("{items:?}").contains("s3cr3tpass"));
    }

    #[test]
    fn umbrella_root_inline_deps_and_umbrella_apps() {
        let root = "defmodule Umbrella.MixProject do\n  use Mix.Project\n\n  def project do\n    [apps_path: \"apps\", version: \"0.1.0\", deps: deps()]\n  end\n\n  defp deps do\n    []\n  end\nend\n";
        let items = Mix.parse_file(&file("mix.exs", root)).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].detail.contains("Umbrella-Projekt"));

        let app = "defmodule Web.MixProject do\n  use Mix.Project\n  def project, do: [app: :web, build_path: \"../../_build\", deps_path: \"../../deps\", lockfile: \"../../mix.lock\", deps: [{:core, in_umbrella: true}, {:plug, \"~> 1.15\"}, {:legacy, ~r/1\\.\\d+/}]]\nend\n";
        let items = Mix.parse_file(&file("mix.exs", app)).unwrap();
        assert!(items[0].detail.starts_with("Mix-Projekt web") && items[0].detail.contains("App eines Umbrella"));
        let names: Vec<&str> = items.iter().skip(1).map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["core", "plug", "legacy"]);
        assert!(items[3].detail.starts_with("Versionsmuster"));
    }

    const MIX_LOCK: &str = r#"%{
  "castore": {:hex, :castore, "1.0.5", "9eeebb394cc9a0f3ae56b813459f990abb0a3dedee1be6b27fdb50301930502f", [:mix], [], "hexpm", "8d7c597c3e4a64c395980882d4bca3cebb8d74197c590dc272cfd3b6a6310578"},
  "cowboy": {:hex, :cowboy, "2.10.0", "ff9ffeff91dae4ae270dd975642997afe2a1179d94b1887863e43f681a203e26", [:make, :rebar3], [{:cowlib, "2.12.1", [hex: :cowlib, repo: "hexpm", optional: false]}, {:ranch, "1.8.0", [hex: :ranch, repo: "hexpm", optional: false]}], "hexpm", "3afdccb7183cc6f143cb14d3cf51fa00e53db9ec80cdcd525482f5e99bc41d6b"},
  "gettext": {:git, "https://deploy:s3cr3tpass@git.corp.example/gettext.git", "0123456789abcdef0123456789abcdef01234567", [tag: "0.1"]},
  "heroicons": {:git, "https://github.com/tailwindlabs/heroicons.git", "88ab3a0d790e6a47404cba02800a6b25d2afae50", [tag: "v2.1.1", sparse: "optimized", depth: 1]},
  "mirror_pkg": {:hex, :mirror_pkg, "2.0.0", "abc", [:mix], [], "my_repo", "def"},
  "old_style": {:hex, :old_style, "0.5.0", "abcdef", [:mix], []},
  "phoenix": {:hex, :phoenix, "1.7.10", "02189140a61b2ce85bb633a9b6fd02dff705a5f1596869547aeb2b2b95edd729", [:mix], [{:castore, ">= 0.0.0", [hex: :castore, repo: "hexpm", optional: false]}, {:jason, "~> 1.0", [hex: :jason, repo: "hexpm", optional: true]}, {:plug_crypto, "~> 1.2 or ~> 2.0", [hex: :plug_crypto, repo: "hexpm", optional: false]}], "hexpm", "cf784932e010fd736d656d7fead6a584a4498efefe5b8227e9f383bf15bb79d0"},
  "private_pkg": {:hex, :private_pkg, "1.0.3", "abc", [:mix], [], "hexpm:acme", "def"},
  "renamed": {:hex, :actual_package, "1.2.3", "abc", [:mix], [], "hexpm", "def"},
}
"#;

    #[test]
    fn mix_lock_hex_git_repositories_and_edges() {
        let items = Mix.parse_file(&file("mix.lock", MIX_LOCK)).unwrap();
        assert_eq!(items[0].detail, "Lockfile · 7 Hex-Pakete · 2 Git-Abhängigkeiten");
        assert_eq!(items.len(), 10);
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Resolved)));
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let castore = find("castore");
        assert_eq!(castore.version.as_deref(), Some("1.0.5"));
        assert_eq!((castore.source, castore.source_host.as_deref()), (SourceClass::PublicRegistry, Some("hex.pm")));
        let cowboy = &find("cowboy").evidence.dependencies;
        assert_eq!(
            (cowboy[0].name.as_str(), cowboy[0].requirement.as_str(), cowboy[0].exact),
            ("cowlib", "2.12.1", true)
        );
        let phoenix = &find("phoenix").evidence.dependencies;
        assert_eq!(phoenix.len(), 3);
        assert_eq!((phoenix[1].kind.as_str(), phoenix[1].exact), ("lock, optional", false));
        let heroicons = find("heroicons");
        assert_eq!((heroicons.source, heroicons.version.as_deref()), (SourceClass::Git, None));
        assert!(heroicons.detail.ends_with("Tag v2.1.1"));
        assert_eq!(find("gettext").source, SourceClass::Git);
        assert!(!format!("{items:?}").contains("s3cr3tpass"));
        assert!(!format!("{items:?}").contains("git.corp.example"));
        let private = find("private_pkg");
        assert_eq!((private.source, private.source_host.as_deref()), (SourceClass::PrivateRegistry, Some("hex.pm")));
        let mirror = find("mirror_pkg");
        assert_eq!((mirror.source, mirror.source_host.as_deref()), (SourceClass::PrivateRegistry, None));
        assert_eq!(find("old_style").source, SourceClass::PublicRegistry, "a missing repo is hexpm");
        assert_eq!(find("actual_package").evidence.qualifier, "renamed");
    }

    #[test]
    fn broken_lockfiles_are_errors() {
        assert!(Mix.parse_file(&file("mix.lock", "%{\n  \"x\": {:hex, :x, \"1.0.0\"\n")).is_err());
        assert!(Mix.parse_file(&file("mix.lock", "defmodule X do\nend\n")).is_err());
        assert!(Mix.parse_file(&file("mix.lock", "%{\n<<<<<<< HEAD\n  \"x\": {:hex, :x, \"1.0.0\"},\n}\n")).is_err());
        assert!(Mix.parse_file(&file("mix.lock", "%{\n  \"x\" {:hex}\n}\n")).is_err());
        assert_eq!(Mix.parse_file(&file("mix.lock", "\n")).unwrap().len(), 1);
    }

    #[test]
    fn deps_and_build_only_beside_mix_exs() {
        assert!(Mix.claim_dir("deps", &["mix.lock".into()], &[]).is_none());
        assert!(Mix.claim_dir("_build", &["package.json".into()], &[]).is_none());
        let build = Mix.claim_dir("_build", &["mix.exs".into()], &[]).unwrap();
        assert_eq!(build.category, ItemCategory::Output);
        let mut dir = FakeDir::default();
        dir.dirs.insert(String::new(), vec![("dev".into(), true), ("test".into(), true)]);
        let items = Mix.inspect_dir(build, "_build", &mut dir).unwrap();
        assert!(items.len() == 1 && items[0].detail.contains("(dev, test)"));

        let claim = Mix.claim_dir("deps", &["mix.exs".into(), "mix.lock".into()], &[]).unwrap();
        assert_eq!(claim.category, ItemCategory::Environment);
        let mut dir = FakeDir::default();
        dir.dirs.insert(
            String::new(),
            vec![("jason".into(), true), ("heroicons".into(), true), (".mix".into(), true), ("x.txt".into(), false)],
        );
        dir.files.insert(
            "jason/hex_metadata.config".into(),
            "{<<\"app\">>,<<\"jason\">>}.\n{<<\"name\">>,<<\"jason\">>}.\n{<<\"version\">>,<<\"1.4.1\">>}.\n".into(),
        );
        let items = Mix.inspect_dir(claim, "deps", &mut dir).unwrap();
        assert_eq!(items.len(), 3);
        let jason = items.iter().find(|i| i.name == "jason").unwrap();
        assert_eq!((jason.version.as_deref(), jason.install_state), (Some("1.4.1"), Some(InstallState::Installed)));
        assert!(items.iter().find(|i| i.name == "heroicons").unwrap().version.is_none());
    }

    #[test]
    fn helpers_respect_strings_and_brackets() {
        assert_eq!(code("gem \"a#b\" # comment"), "gem \"a#b\" ");
        assert_eq!(split_top(r#":a, "x, y", [b: 1, c: 2], {d, e}"#), [":a", "\"x, y\"", "[b: 1, c: 2]", "{d, e}"]);
        assert_eq!(closing("{a, \"}\", [b]} tail", 0), Some(12));
        assert_eq!(closing("{{{", 0), None);
        assert_eq!(balance("gem(\"x\","), 1);
        assert_eq!(names("[:dev, :test]"), ["dev", "test"]);
        assert_eq!(names("~w(dev test)a"), ["dev", "test"]);
        assert_eq!(names("%i[ windows jruby ]"), ["windows", "jruby"]);
        assert_eq!(exact_version("== 1.2.3"), Some("1.2.3"));
        assert_eq!(exact_version("~> 1.2"), None);
    }
}
