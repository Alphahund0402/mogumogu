//! Git submodules: .gitmodules (declared, git-config syntax). The commit a
//! submodule is pinned to lives in the superproject's tree and index inside
//! `.git`, which is never read; neither are the submodules' own contents or
//! `.git/config` overrides. Of a URL only the host is kept.
use super::docker::count;
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};

pub struct GitSubmodule;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "git-submodule",
    name: "Git-Submodule",
    markers: &[".gitmodules"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[".gitmodules (path, url, branch, update, shallow)"],
    limits: "Kein git-Aufruf; der festgelegte Commit steht im Git-Index des Hauptprojekts und wird nicht gelesen, ebenso Submodul-Inhalte und .git/config. Von der URL wird nur der Host gespeichert.",
};

impl Adapter for GitSubmodule {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        let modules = submodules(file.content);
        if modules.is_empty() {
            return Ok(Vec::new());
        }
        let mut items = vec![manifest(
            "git-submodule",
            file,
            format!("Git-Submodule · {}", count(modules.len(), "Eintrag", "Einträge")),
        )];
        let mut paths = std::collections::HashSet::new();
        for module in modules {
            let path = if module.path.is_empty() { &module.name } else { &module.path };
            let path = path.trim_start_matches("./").trim_end_matches('/');
            if path.is_empty() || !paths.insert(path.to_string()) {
                continue;
            }
            let (source, host) = if module.url.is_empty() {
                (SourceClass::Unknown, None)
            } else {
                match remote(&module.url) {
                    Remote::Local => (SourceClass::Path, None),
                    Remote::Network { host, .. } => (SourceClass::Git, host),
                }
            };
            let mut detail = match module.branch.as_str() {
                "" => "ohne Branch-Angabe".to_string(),
                "." => "Branch wie im Hauptprojekt".to_string(),
                branch => format!("Branch {branch}"),
            };
            match module.update.as_str() {
                "" => {}
                mode @ ("checkout" | "rebase" | "merge" | "none") => detail.push_str(&format!(" · update {mode}")),
                // `!command` is not allowed here; Git ignores it.
                _ => detail.push_str(" · update-Angabe unzulässig (nicht gespeichert)"),
            }
            if module.shallow {
                detail.push_str(" · flach");
            }
            detail.push_str(" · Commit nicht gelesen");
            items.push(
                InventoryItem::new(ItemCategory::Package, "git-submodule", path, file.rel_path())
                    .state(InstallState::Declared)
                    .source(source, host)
                    .detail(detail),
            );
        }
        bounded(items)
    }
}

/// Where a Git URL points. Credentials, port, query and fragment are dropped.
pub(super) enum Remote {
    /// `scheme://host/path` or scp-like `[user@]host:path`.
    Network { host: Option<String>, path: String },
    /// A relative or absolute path, or `file://`.
    Local,
}

pub(super) fn remote(url: &str) -> Remote {
    let url = url.trim();
    if let Some((scheme, rest)) = url.split_once("://") {
        if scheme.eq_ignore_ascii_case("file") {
            return Remote::Local;
        }
        let path = rest.split_once('/').map_or("", |(_, path)| path);
        return Remote::Network { host: crate::privacy::url_host(url), path: repo_path(path) };
    }
    // scp-like syntax; a drive letter (`C:\x`, `C:/x`) is a local path.
    if let Some((authority, path)) = url.split_once(':') {
        let host = authority.rsplit('@').next().unwrap_or(authority);
        let valid = host.len() > 1
            && host.len() <= 253
            && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
        if valid {
            return Remote::Network { host: Some(host.to_ascii_lowercase()), path: repo_path(path) };
        }
    }
    Remote::Local
}

/// `owner/repo` from a URL path: no query, fragment, outer slashes or `.git`.
fn repo_path(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or_default().trim_matches('/');
    path.strip_suffix(".git").unwrap_or(path).to_string()
}

#[derive(Default)]
struct Module {
    name: String,
    path: String,
    url: String,
    branch: String,
    update: String,
    shallow: bool,
}

/// `[submodule "name"]` sections; other sections are skipped.
fn submodules(text: &str) -> Vec<Module> {
    let mut modules: Vec<Module> = Vec::new();
    let mut inside = false;
    for line in text.lines().take(100_000) {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let header = header.rfind(']').map_or(header, |end| &header[..end]);
            // `[submodule "name"]`, or the deprecated `[submodule.name]`.
            let (section, name) = match header.split_once(|c: char| c == '"' || c.is_whitespace()) {
                Some((section, name)) => (section.trim(), name.trim().trim_matches('"')),
                None => header.split_once('.').unwrap_or((header, "")),
            };
            inside = section.eq_ignore_ascii_case("submodule") && !name.is_empty() && modules.len() < 10_000;
            if inside {
                modules.push(Module { name: name.to_string(), ..Module::default() });
            }
            continue;
        }
        let Some(module) = modules.last_mut().filter(|_| inside) else { continue };
        let (key, value) = line.split_once('=').map_or((line, "true".into()), |(k, v)| (k.trim(), config_value(v)));
        match key.to_ascii_lowercase().as_str() {
            "path" => module.path = value,
            "url" => module.url = value,
            "branch" => module.branch = value,
            "update" => module.update = value,
            "shallow" => module.shallow = matches!(value.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"),
            _ => {}
        }
    }
    modules
}

/// A git-config value: quotes removed, escapes applied, comments cut.
fn config_value(raw: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' => match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') | None => {}
                Some(other) => out.push(other),
            },
            '#' | ';' if !quoted => break,
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::file;

    #[test]
    fn submodules_keep_hosts_but_never_urls() {
        let secret = "ghp_secrettoken1234567890abcdefghij";
        let text = format!(
            "[submodule \"libfoo\"]\n\tpath = include/foo\n\turl = https://user:{secret}@github.com/acme/libfoo.git\n\tbranch = main\n[submodule \"vendor/bar\"]\n\tpath = vendor/bar\n\turl = ../bar.git\n\tupdate = rebase\n\tshallow = true\n[submodule \"docs-theme\"]\n\tpath = \"docs/theme\"\n\turl = git@gitlab.example.com:web/theme.git ; mirror\n\tbranch = .\n[submodule \"tools\"]\n\tpath = tools\n\turl = C:/repos/tools\n\tupdate = !git pull --rebase\n[core]\n\tpath = not/a/submodule\n"
        );
        let items = GitSubmodule.parse_file(&file(".gitmodules", &text)).unwrap();
        assert_eq!(items[0].detail, "Git-Submodule · 4 Einträge");
        let foo = items.iter().find(|i| i.name == "include/foo").unwrap();
        assert_eq!((foo.source, foo.source_host.as_deref()), (SourceClass::Git, Some("github.com")));
        assert_eq!(foo.detail, "Branch main · Commit nicht gelesen");
        assert!(foo.version.is_none() && foo.install_state == Some(InstallState::Declared));
        let bar = items.iter().find(|i| i.name == "vendor/bar").unwrap();
        assert_eq!(bar.source, SourceClass::Path);
        assert_eq!(bar.detail, "ohne Branch-Angabe · update rebase · flach · Commit nicht gelesen");
        let theme = items.iter().find(|i| i.name == "docs/theme").unwrap();
        assert_eq!((theme.source, theme.source_host.as_deref()), (SourceClass::Git, Some("gitlab.example.com")));
        assert!(theme.detail.starts_with("Branch wie im Hauptprojekt"));
        let tools = items.iter().find(|i| i.name == "tools").unwrap();
        assert_eq!(tools.source, SourceClass::Path);
        assert!(tools.detail.contains("update-Angabe unzulässig"));
        let debug = format!("{items:?}");
        for absent in [secret, "acme/libfoo", "bar.git", "theme.git", "C:/repos", "git pull", "not/a/submodule"] {
            assert!(!debug.contains(absent), "{absent}");
        }
        assert!(GitSubmodule.parse_file(&file(".gitmodules", "# none\n")).unwrap().is_empty());
    }

    #[test]
    fn remotes_of_every_url_form() {
        let network = |url: &str| match remote(url) {
            Remote::Network { host, path } => Some((host, path)),
            Remote::Local => None,
        };
        assert_eq!(
            network("https://oauth2:token@gitlab.com:8443/group/sub/repo.git/?ref=x#frag"),
            Some((Some("gitlab.com".into()), "group/sub/repo".into()))
        );
        assert_eq!(
            network("ssh://git@host.example/owner/repo"),
            Some((Some("host.example".into()), "owner/repo".into()))
        );
        assert_eq!(network("git@github.com:owner/repo.git"), Some((Some("github.com".into()), "owner/repo".into())));
        for local in ["../lib.git", "./lib", "/srv/git/lib.git", "C:\\repos\\lib", "file:///srv/git/lib.git"] {
            assert!(network(local).is_none(), "{local}");
        }
    }
}
