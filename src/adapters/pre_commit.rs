//! pre-commit and prek: hook repositories of .pre-commit-config.yaml (prek
//! also reads .pre-commit-config.yml and prek.toml) with the revision they
//! are pinned to (declared). Nothing is installed or run, and the hook
//! environments in the user cache are not read. `additional_dependencies`
//! are only counted: their ecosystem follows the hook's language, which the
//! hook repository defines.
use super::docker::{KeyPath, Unique, count};
use super::git_submodule::{Remote, remote};
use super::github_actions::git_ref;
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};
use crate::parsers::{lines, toml_table};

pub struct PreCommit;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "pre-commit",
    name: "pre-commit",
    markers: &[".pre-commit-config.yaml", ".pre-commit-config.yml", "prek.toml"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[".pre-commit-config.yaml/.yml (repos, rev, hooks)", "prek.toml"],
    limits: "pre-commit und prek werden nicht gestartet; Hook-Umgebungen im Benutzercache werden nicht gelesen. additional_dependencies sowie local- und meta-Hooks werden nur gezählt.",
};

impl Adapter for PreCommit {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let repos = if file.name.eq_ignore_ascii_case("prek.toml") {
            toml_repos(file.content)?
        } else {
            yaml_repos(file.content)
        };
        match repos {
            Some(repos) => bounded(items(file, &repos)),
            None => Ok(Vec::new()),
        }
    }
}

/// One `repos` entry.
#[derive(Default)]
struct Repo {
    url: String,
    rev: String,
    hooks: usize,
    dependencies: usize,
}

/// `repos` of the YAML config; `None` without a top-level `repos:` key.
fn yaml_repos(text: &str) -> Option<Vec<Repo>> {
    let mut path = KeyPath::default();
    let (mut found, mut repos) = (false, Vec::<Repo>::new());
    for entry in lines::yaml_entries(text).take(200_000) {
        path.enter(&entry);
        match path.keys() {
            [] => found |= entry.key == Some("repos"),
            ["repos"] => {
                if entry.item && repos.len() < 10_000 {
                    repos.push(Repo::default());
                }
                if let Some(repo) = repos.last_mut() {
                    match entry.key {
                        Some("repo") => repo.url = entry.value.to_string(),
                        Some("rev") => repo.rev = entry.value.to_string(),
                        Some("hooks") => repo.hooks += flow_count(entry.value),
                        _ => {}
                    }
                }
            }
            ["repos", "hooks"] => {
                if let Some(repo) = repos.last_mut() {
                    repo.hooks += usize::from(entry.item);
                    if entry.key == Some("additional_dependencies") {
                        repo.dependencies += flow_count(entry.value);
                    }
                }
            }
            ["repos", "hooks", "additional_dependencies"] => {
                if let Some(repo) = repos.last_mut() {
                    repo.dependencies += usize::from(entry.item);
                }
            }
            _ => {}
        }
        path.push(&entry);
    }
    found.then_some(repos)
}

/// `[[repos]]` of a prek.toml; `None` without a `repos` array.
fn toml_repos(text: &str) -> Result<Option<Vec<Repo>>, String> {
    let table = toml_table(text).map_err(|e| e.to_string())?;
    let Some(list) = table.get("repos").and_then(toml::Value::as_array) else { return Ok(None) };
    let repos = list
        .iter()
        .filter_map(toml::Value::as_table)
        .take(10_000)
        .map(|repo| {
            let text = |key: &str| repo.get(key).and_then(toml::Value::as_str).unwrap_or_default().to_string();
            let hooks = repo.get("hooks").and_then(toml::Value::as_array).map(Vec::as_slice).unwrap_or_default();
            Repo {
                url: text("repo"),
                rev: text("rev"),
                hooks: hooks.len(),
                dependencies: hooks
                    .iter()
                    .filter_map(|hook| hook.get("additional_dependencies")?.as_array())
                    .map(Vec::len)
                    .sum(),
            }
        })
        .collect();
    Ok(Some(repos))
}

/// Items of a one-line flow sequence, also with brackets inside (`pkg[extra]`).
fn flow_count(value: &str) -> usize {
    let Some(inner) = value.trim().strip_prefix('[').and_then(|v| v.strip_suffix(']')) else { return 0 };
    if inner.trim().is_empty() {
        return 0;
    }
    let (mut depth, mut quote, mut items) = (0_usize, None, 1);
    for c in inner.chars() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[' | '{') => depth += 1,
            (None, ']' | '}') => depth = depth.saturating_sub(1),
            (None, ',') if depth == 0 => items += 1,
            _ => {}
        }
    }
    items - usize::from(inner.trim_end().ends_with(','))
}

fn items(file: &FoundFile<'_>, repos: &[Repo]) -> Vec<InventoryItem> {
    let mut unique = Unique::default();
    let (mut special, mut hooks) = (0, 0);
    for repo in repos {
        hooks += repo.hooks;
        if matches!(repo.url.as_str(), "local" | "meta") {
            special += 1;
            continue;
        }
        let (name, source, host) = match remote(&repo.url) {
            Remote::Network { host, path } => (path, SourceClass::Git, host),
            // A path on disk: only its last folder name, never the path.
            Remote::Local => {
                let last = repo.url.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default();
                (last.strip_suffix(".git").unwrap_or(last).to_string(), SourceClass::Path, None)
            }
        };
        if name.is_empty() {
            continue;
        }
        let (version, pinned) = git_ref(&repo.rev);
        let mut detail = count(repo.hooks, "Hook", "Hooks");
        if repo.dependencies > 0 {
            let dependencies = count(repo.dependencies, "zusätzliche Abhängigkeit", "zusätzliche Abhängigkeiten");
            detail.push_str(&format!(" · {dependencies} (nicht ausgewertet)"));
        }
        detail.push_str(match (repo.rev.is_empty(), pinned) {
            (true, _) => " · ohne rev",
            (false, true) => " · auf Commit gepinnt",
            (false, false) => " · Tag oder Branch, nicht gepinnt",
        });
        unique.push(
            InventoryItem::new(ItemCategory::Package, "pre-commit", name, file.rel_path())
                .version(version)
                .state(InstallState::Declared)
                .source(source, host)
                .detail(detail),
        );
    }
    let tool = if file.name.eq_ignore_ascii_case("prek.toml") { "prek" } else { "pre-commit" };
    let mut detail = format!(
        "{tool} · {} · {}",
        count(unique.items.len(), "Repository", "Repositories"),
        count(hooks, "Hook", "Hooks")
    );
    if special > 0 {
        detail.push_str(&format!(" · {special} local/meta (keine Abhängigkeit)"));
    }
    let mut items = vec![manifest("pre-commit", file, detail)];
    items.extend(unique.items);
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    const CONFIG: &str = "default_language_version:\n  python: python3.12\nrepos:\n-   repo: https://github.com/pre-commit/pre-commit-hooks\n    rev: v4.6.0\n    hooks:\n    -   id: trailing-whitespace\n    -   id: end-of-file-fixer\n-   repo: https://github.com/astral-sh/ruff-pre-commit.git\n    rev: 9f3d6d0b6f2a5c4e1d8b7a6c5e4f3a2b1c0d9e8f  # frozen: v0.6.9\n    hooks:\n    -   id: ruff\n        args: [--fix]\n    -   id: ruff-format\n-   repo: https://user:s3cr3t-t0ken@git.corp.example/tools/hooks\n    rev: 1.0.0\n    hooks:\n    -   id: mypy\n        additional_dependencies: [types-requests, \"pydantic[email]>=2\"]\n    -   id: eslint\n        additional_dependencies:\n          - eslint@9.0.0\n          - eslint-plugin-react@7.34.1\n-   repo: local\n    hooks:\n    -   id: pylint\n        name: pylint\n        entry: pylint\n        language: system\n-   repo: meta\n    hooks:\n    -   id: check-hooks-apply\n";

    #[test]
    fn hook_repositories_with_revisions_and_counts() {
        let items = PreCommit.parse_file(&file(".pre-commit-config.yaml", CONFIG)).unwrap();
        assert_eq!(items[0].detail, "pre-commit · 3 Repositories · 8 Hooks · 2 local/meta (keine Abhängigkeit)");
        let hooks = items.iter().find(|i| i.name == "pre-commit/pre-commit-hooks").unwrap();
        assert_eq!((hooks.version.as_deref(), hooks.source), (Some("v4.6.0"), SourceClass::Git));
        assert_eq!(hooks.source_host.as_deref(), Some("github.com"));
        assert_eq!(hooks.detail, "2 Hooks · Tag oder Branch, nicht gepinnt");
        let ruff = items.iter().find(|i| i.name == "astral-sh/ruff-pre-commit").unwrap();
        assert_eq!(ruff.version.as_deref(), Some("9f3d6d0b6f2a"));
        assert_eq!(ruff.detail, "2 Hooks · auf Commit gepinnt");
        let private = items.iter().find(|i| i.name == "tools/hooks").unwrap();
        assert_eq!(private.source_host.as_deref(), Some("git.corp.example"));
        assert_eq!(
            private.detail,
            "2 Hooks · 4 zusätzliche Abhängigkeiten (nicht ausgewertet) · Tag oder Branch, nicht gepinnt"
        );
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        assert_eq!(items.len(), 4, "local and meta are no dependencies");
        let debug = format!("{items:?}");
        assert!(!debug.contains("s3cr3t") && !debug.contains("pylint") && !debug.contains("eslint"));
        assert!(PreCommit.parse_file(&file(".pre-commit-config.yaml", "foo: bar\n")).unwrap().is_empty());
    }

    #[test]
    fn prek_toml_and_local_paths() {
        let text = "[[repos]]\nrepo = \"https://github.com/pre-commit/pre-commit-hooks\"\nrev = \"v6.0.0\"\nhooks = [\n  { id = \"trailing-whitespace\" },\n  { id = \"check-added-large-files\", args = [\"--maxkb=1024\"] },\n]\n\n[[repos]]\nrepo = \"local\"\nhooks = [{ id = \"cargo-fmt\", name = \"cargo fmt\", entry = \"cargo fmt\", language = \"system\" }]\n\n[[repos]]\nrepo = \"C:/Users/someone/hooks.git\"\nrev = \"main\"\nhooks = [{ id = \"x\", additional_dependencies = [\"a\", \"b\"] }]\n";
        let items = PreCommit.parse_file(&file("prek.toml", text)).unwrap();
        assert_eq!(items[0].detail, "prek · 2 Repositories · 4 Hooks · 1 local/meta (keine Abhängigkeit)");
        assert_eq!(items[1].version.as_deref(), Some("v6.0.0"));
        let local = items.iter().find(|i| i.source == SourceClass::Path).unwrap();
        assert_eq!(local.name, "hooks");
        assert!(!format!("{items:?}").contains("someone"), "absolute paths are never stored");
        assert!(PreCommit.parse_file(&file("prek.toml", "[[repos]\n")).is_err());
        assert!(PreCommit.parse_file(&file("prek.toml", "[tool]\nx = 1\n")).unwrap().is_empty());
        assert_eq!(flow_count("[a, \"b[c]\", {d: e},]"), 3);
        assert_eq!(flow_count("[]"), 0);
    }
}
