//! R / renv: renv.lock (resolved packages with their sources and
//! requirements), DESCRIPTION files of R packages and projects (declared)
//! and the renv project library (installed: folder names and `Version:`).
//! R is never started; .Rprofile, activate.R and repository indexes are
//! not read, and links into the renv cache are not followed.
use super::nix::{git_host, short_ref, valid_host};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::parsers::json;
use serde_json::Value;
use std::collections::BTreeSet;

pub struct Renv;

const PUBLIC: &[&str] = &[
    "cloud.r-project.org",
    "cran.r-project.org",
    "cran.rstudio.com",
    "packagemanager.posit.co",
    "packagemanager.rstudio.com",
    "bioconductor.org",
];

static DESCRIPTOR: Descriptor = Descriptor {
    id: "renv",
    name: "R (renv)",
    markers: &["renv.lock", "DESCRIPTION"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "renv.lock (Requirements; DESCRIPTION-Felder ab renv 1.1)",
        "DESCRIPTION (Depends, Imports, LinkingTo, Suggests, Remotes)",
        "renv/library (Ordnernamen, Version aus DESCRIPTION)",
    ],
    limits: "Kein R-Start; .Rprofile, activate.R und Repository-Indizes werden nicht gelesen; Pakete, die renv als Verknüpfung in seinen Cache anlegt (Standard), sind in der Projektbibliothek nicht sichtbar.",
};

/// Dependency fields of a DESCRIPTION file and their contexts.
const FIELDS: [(&str, &str); 4] =
    [("Depends", "depends"), ("Imports", "imports"), ("LinkingTo", "linkingto"), ("Suggests", "suggests")];

impl Adapter for Renv {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case("renv.lock") { bounded(lockfile(file)?) } else { bounded(description(file)) }
    }

    /// `renv/library`: renv's own activate.R lies beside it, renv.lock one
    /// level up; the children are platform or `R-x.y` folders.
    fn claim_dir(&self, name: &str, siblings: &[String], children: &[String]) -> Option<DirClaim> {
        let renv = siblings.iter().any(|s| s == "activate.R");
        let layout = children
            .iter()
            .any(|c| r_version(c) || matches!(c.as_str(), "windows" | "macos") || c.starts_with("linux-"));
        (name == "library" && renv && layout).then_some(DirClaim {
            adapter: "renv",
            category: ItemCategory::Environment,
            label: "renv-Projektbibliothek",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        // [<platform>/]R-<x.y>/<architecture>/<package>
        let mut versions: Vec<Vec<String>> = Vec::new();
        for (first, is_dir) in access.list(&[]).into_iter().take(32) {
            if !is_dir {
                continue;
            }
            if r_version(&first) {
                versions.push(vec![first]);
                continue;
            }
            for (second, is_dir) in access.list(&[&first]).into_iter().take(32) {
                if is_dir && r_version(&second) {
                    versions.push(vec![first.clone(), second]);
                }
            }
        }
        let (mut items, mut labels) = (Vec::new(), BTreeSet::new());
        for version in versions.iter().take(8) {
            let base: Vec<&str> = version.iter().map(String::as_str).collect();
            labels.insert(base[base.len() - 1].trim_start_matches("R-"));
            for (architecture, is_dir) in access.list(&base).into_iter().take(8) {
                if !is_dir {
                    continue;
                }
                let mut path = base.clone();
                path.push(&architecture);
                for (package, is_dir) in access.list(&path).into_iter().take(10_000) {
                    if !is_dir || package.starts_with('.') || package.starts_with("00LOCK") {
                        continue;
                    }
                    let mut description = path.clone();
                    description.extend([package.as_str(), "DESCRIPTION"]);
                    let version = access.read_text(&description, 256 * 1024).and_then(|text| field(&text, "Version"));
                    let detail = if version.is_some() {
                        "installiert laut DESCRIPTION"
                    } else {
                        "Ordner vorhanden; Version unbekannt"
                    };
                    let mut item = InventoryItem::new(ItemCategory::Package, "renv", package.as_str(), rel_path)
                        .version(version.unwrap_or_default())
                        .state(InstallState::Installed)
                        .detail(detail);
                    item.evidence.qualifier = path.join("/");
                    items.push(item);
                }
            }
        }
        let r = if labels.is_empty() { "?".to_string() } else { labels.into_iter().collect::<Vec<_>>().join(", ") };
        let detail = format!("R {r} · {} Pakete als Ordner · verknüpfte Cache-Einträge nicht verfolgt", items.len());
        items.insert(0, InventoryItem::new(ItemCategory::Environment, "renv", "renv/library", rel_path).detail(detail));
        bounded(items)
    }
}

fn r_version(name: &str) -> bool {
    name.strip_prefix("R-").is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
}

fn is_package_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
}

/// Fields of a Debian-control record (an R DESCRIPTION file): continuation
/// lines start with white space, a blank line ends the record. `None` when
/// the text has another shape.
fn dcf(text: &str) -> Option<Vec<(&str, String)>> {
    let mut fields: Vec<(&str, String)> = Vec::new();
    for line in text.lines().take(20_000) {
        if line.trim().is_empty() {
            if fields.is_empty() {
                continue;
            }
            break;
        }
        if line.starts_with([' ', '\t']) {
            let (_, value) = fields.last_mut()?;
            if value.len() < 64 * 1024 {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        let (key, value) = line.split_once(':')?;
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '.' | '_' | '-'))
        {
            return None;
        }
        fields.push((key, value.trim().to_string()));
    }
    Some(fields)
}

fn field(text: &str, key: &str) -> Option<String> {
    dcf(text)?.into_iter().find(|(k, _)| *k == key).map(|(_, v)| v).filter(|v| !v.is_empty())
}

/// `dplyr (>= 1.1.0), rlang` → [(`dplyr`, `>= 1.1.0`), (`rlang`, ``)].
fn requirements(list: &str) -> Vec<(String, String)> {
    list.split(',')
        .take(2_000)
        .filter_map(|entry| {
            let entry = entry.trim();
            let (name, rest) = entry.split_once('(').map_or((entry, ""), |(name, rest)| (name.trim(), rest));
            let requirement = rest.split(')').next().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
            is_package_name(name).then(|| (name.to_string(), requirement))
        })
        .collect()
}

/// One `Remotes` entry reduced to a package name and its source.
struct Remote {
    name: String,
    source: SourceClass,
    host: Option<String>,
    kind: &'static str,
}

/// `[pkg=][type::]user/repo[/subdir][@ref|#pr]`, `git::url`, `url::url`,
/// `local::path`, `bioc::release/Pkg`.
fn remote(spec: &str) -> Option<Remote> {
    let (explicit, spec) = match spec.split_once('=') {
        Some((name, rest)) if is_package_name(name.trim()) => (Some(name.trim()), rest.trim()),
        _ => (None, spec.trim()),
    };
    let (kind, target) = spec.split_once("::").unwrap_or(("github", spec));
    let kind = kind.to_ascii_lowercase();
    let path = target.split(['@', '#', '?']).next().unwrap_or(target).trim_end_matches(['/', '\\']);
    let last = path.rsplit(['/', '\\']).next().unwrap_or("").trim_end_matches(".git");
    let (source, host, label) = match kind.as_str() {
        "github" | "gitlab" | "bitbucket" => {
            let default = match kind.as_str() {
                "gitlab" => "gitlab.com",
                "bitbucket" => "bitbucket.org",
                _ => "github.com",
            };
            let host = if target.contains("://") { git_host(target) } else { Some(default.to_string()) };
            (SourceClass::Git, host, "Git")
        }
        "git" => (SourceClass::Git, git_host(target), "Git"),
        "url" => {
            let (source, host) = classify_url(target, PUBLIC);
            (source, host, "URL")
        }
        "local" => (SourceClass::Path, None, "lokaler Pfad"),
        "bioc" => (SourceClass::PublicRegistry, Some("bioconductor.org".into()), "Bioconductor"),
        "svn" => (SourceClass::Unknown, None, "Subversion"),
        _ => (SourceClass::Unknown, None, "unbekannter Typ"),
    };
    // A tarball is named `<package>_<version>.tar.gz`.
    let guess = if kind == "url" { last.split('_').next().unwrap_or(last) } else { last };
    let name = explicit.unwrap_or(guess);
    is_package_name(name).then(|| Remote { name: name.to_string(), source, host, kind: label })
}

fn description(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let Some(fields) = dcf(file.content) else { return Vec::new() };
    let get = |key: &str| fields.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str());
    let Some(package) = get("Package").filter(|p| !p.is_empty()) else { return Vec::new() };
    // R CMD INSTALL adds `Built:`: an installed copy is no project manifest.
    if get("Built").is_some() || !FIELDS.iter().any(|&(field, _)| get(field).is_some()) {
        return Vec::new();
    }
    let remotes: Vec<Remote> = get("Remotes")
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(1_000)
        .filter_map(remote)
        .collect();
    let mut detail = format!("R-Paket {package}");
    if let Some(version) = get("Version") {
        detail.push_str(&format!(" {version}"));
    }
    let mut items = Vec::new();
    for (field, context) in FIELDS {
        for (name, requirement) in requirements(get(field).unwrap_or("")) {
            if name == "R" {
                if !requirement.is_empty() {
                    detail.push_str(&format!(" · erfordert R {requirement}"));
                }
                continue;
            }
            let mut text = if requirement.is_empty() {
                "ohne Versionsangabe".to_string()
            } else {
                format!("Spezifikation {requirement}")
            };
            text.push_str(&format!(" · {field}"));
            let remote = remotes.iter().find(|r| r.name == name);
            if let Some(remote) = remote {
                text.push_str(&format!(" · Quelle laut Remotes: {}", remote.kind));
            }
            let (source, host) = remote.map_or((SourceClass::Unknown, None), |r| (r.source, r.host.clone()));
            let mut item = InventoryItem::new(ItemCategory::Package, "renv", name, file.rel_path())
                .state(InstallState::Declared)
                .source(source, host)
                .detail(text);
            item.evidence.context = context.into();
            items.push(item);
        }
    }
    let unmatched: Vec<&Remote> = remotes.iter().filter(|r| !items.iter().any(|i| i.name == r.name)).collect();
    for remote in unmatched {
        let mut item = InventoryItem::new(ItemCategory::Package, "renv", remote.name.as_str(), file.rel_path())
            .state(InstallState::Declared)
            .source(remote.source, remote.host.clone())
            .detail(format!("Remotes-Eintrag ({}) ohne passende Abhängigkeit", remote.kind));
        item.evidence.context = "remotes".into();
        items.push(item);
    }
    items.insert(0, manifest("renv", file, detail));
    items
}

fn lockfile(file: &FoundFile<'_>) -> AdapterResult {
    let value = json::parse(file.content, 64).map_err(|e| e.to_string())?;
    let packages = value["Packages"].as_object().ok_or("renv.lock ohne Packages")?;
    let repositories: Vec<(&str, &str)> = value["R"]["Repositories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r["Name"].as_str()?, r["URL"].as_str()?)))
        .take(256) // searched once per package
        .collect();
    let mut detail = String::from("Lockfile");
    if let Some(r) = value["R"]["Version"].as_str() {
        detail.push_str(&format!(" · R {r}"));
    }
    if let Some(bioconductor) = value["Bioconductor"]["Version"].as_str() {
        detail.push_str(&format!(" · Bioconductor {bioconductor}"));
    }
    let private =
        repositories.iter().filter(|(_, url)| classify_url(url, PUBLIC).0 != SourceClass::PublicRegistry).count();
    detail.push_str(&format!(" · {} Repositories", repositories.len()));
    if private > 0 {
        detail.push_str(&format!(" (davon {private} nicht öffentlich)"));
    }
    let mut items = vec![manifest("renv", file, detail)];
    for (key, record) in packages.iter().take(50_000) {
        let name = record["Package"].as_str().unwrap_or(key);
        let (source, host, origin) = lock_source(record, &repositories);
        let mut item = InventoryItem::new(ItemCategory::Package, "renv", name, file.rel_path())
            .version(record["Version"].as_str().unwrap_or_default())
            .state(InstallState::Resolved)
            .source(source, host)
            .detail(format!("aufgelöst laut renv.lock{origin}"));
        item.evidence.dependencies = lock_edges(record);
        items.push(item);
    }
    Ok(items)
}

/// Source of a lockfile record from `Source`, `Repository` and `Remote*`.
fn lock_source(record: &Value, repositories: &[(&str, &str)]) -> (SourceClass, Option<String>, String) {
    let text = |key: &str| record[key].as_str().unwrap_or("");
    match text("Source") {
        "Repository" => {
            let name = text("Repository");
            if name.contains("://") {
                let (source, host) = classify_url(name, PUBLIC);
                return (source, host, String::new());
            }
            match repositories.iter().find(|(n, _)| *n == name) {
                Some((_, url)) => {
                    let (source, host) = classify_url(url, PUBLIC);
                    (source, host, format!(" · Repository {name}"))
                }
                None if name.is_empty() => (SourceClass::Unknown, None, String::new()),
                None => (SourceClass::Unknown, None, format!(" · Repository {name} (nicht aufgeführt)")),
            }
        }
        "Bioconductor" => (SourceClass::PublicRegistry, Some("bioconductor.org".into()), " · Bioconductor".into()),
        forge @ ("GitHub" | "GitLab" | "Bitbucket") => {
            let mut origin = format!(" · {forge}");
            if !text("RemoteUsername").is_empty() && !text("RemoteRepo").is_empty() {
                origin.push_str(&format!(" {}/{}", text("RemoteUsername"), text("RemoteRepo")));
            }
            if let Some(reference) = short_ref(text("RemoteRef")).filter(|r| r != "HEAD") {
                origin.push_str(&format!(" · Ref {reference}"));
            }
            if let Some(sha) = short_ref(text("RemoteSha")) {
                origin.push_str(&format!(" · Commit {sha}"));
            }
            (SourceClass::Git, forge_host(text("RemoteHost"), forge), origin)
        }
        "git" | "Git" => (SourceClass::Git, git_host(text("RemoteUrl")), " · Git".into()),
        "Local" => (SourceClass::Path, None, " · lokal".into()),
        "URL" => {
            let (source, host) = classify_url(text("RemoteUrl"), PUBLIC);
            (source, host, " · URL".into())
        }
        "" => (SourceClass::Unknown, None, String::new()),
        other => (SourceClass::Unknown, None, format!(" · Quelle {other}")),
    }
}

/// `api.github.com`, `https://github.example.com/api/v3` or the default.
fn forge_host(remote_host: &str, forge: &str) -> Option<String> {
    let host = if remote_host.contains("://") {
        crate::privacy::url_host(remote_host)
    } else {
        valid_host(remote_host.split('/').next().unwrap_or(""))
    };
    match host.as_deref() {
        None => Some(match forge {
            "GitLab" => "gitlab.com".into(),
            "Bitbucket" => "bitbucket.org".into(),
            _ => "github.com".into(),
        }),
        Some("api.github.com") => Some("github.com".into()),
        Some("api.bitbucket.org") => Some("bitbucket.org".into()),
        Some(_) => host,
    }
}

/// `Requirements` (renv < 1.1) or `Depends`/`Imports`/`LinkingTo` (1.1+).
fn lock_edges(record: &Value) -> Vec<PackageDependency> {
    let mut seen = BTreeSet::new();
    let mut edges = Vec::new();
    for name in record["Requirements"].as_array().into_iter().flatten().filter_map(Value::as_str).take(1_000) {
        if seen.insert(name.to_string()) {
            edges.push(PackageDependency {
                name: name.into(),
                requirement: String::new(),
                kind: "Requirements".into(),
                exact: false,
            });
        }
    }
    for field in ["Depends", "Imports", "LinkingTo"] {
        let entries: Vec<(String, String)> = match &record[field] {
            Value::Array(list) => list.iter().take(1_000).filter_map(Value::as_str).flat_map(requirements).collect(),
            Value::String(text) => requirements(text),
            _ => Vec::new(),
        };
        for (name, requirement) in entries {
            if name != "R" && seen.insert(name.clone()) {
                edges.push(PackageDependency { name, requirement, kind: field.into(), exact: false });
            }
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const LOCK: &str = r#"{
  "R": {
    "Version": "4.4.2",
    "Repositories": [
      { "Name": "CRAN", "URL": "https://cloud.r-project.org" },
      { "Name": "Internal", "URL": "https://reader:s3cr3t@rpkgs.corp.example/latest" }
    ]
  },
  "Bioconductor": { "Version": "3.20" },
  "Packages": {
    "markdown": {
      "Package": "markdown",
      "Version": "1.0",
      "Source": "Repository",
      "Repository": "CRAN",
      "Hash": "4584a57f565dd7987d59dda3a02cfb41",
      "Requirements": ["utils", "xfun"]
    },
    "ggplot2": {
      "Package": "ggplot2",
      "Version": "3.5.1",
      "Source": "Repository",
      "Repository": "CRAN",
      "Depends": ["R (>= 3.5)"],
      "Imports": ["cli", "grDevices", "gtable (>= 0.1.1)", "rlang (>= 1.1.0)"],
      "Suggests": ["covr", "testthat (>= 3.1.5)"],
      "Maintainer": "Thomas Lin Pedersen <thomas.pedersen@posit.co>"
    },
    "corpstats": { "Package": "corpstats", "Version": "0.4.1", "Source": "Repository", "Repository": "Internal" },
    "mime": {
      "Package": "mime",
      "Version": "0.12.1",
      "Source": "GitHub",
      "RemoteType": "github",
      "RemoteHost": "api.github.com",
      "RemoteUsername": "yihui",
      "RemoteRepo": "mime",
      "RemoteRef": "main",
      "RemoteSha": "1763e0dcb72fb58d97bab97bb834fc71f1e012bc",
      "Requirements": ["tools"]
    },
    "Biobase": {
      "Package": "Biobase",
      "Version": "2.66.0",
      "Source": "Bioconductor",
      "git_url": "https://git.bioconductor.org/packages/Biobase",
      "Depends": ["R (>= 2.10)", "BiocGenerics (>= 0.27.1)", "utils"]
    },
    "mylocal": { "Package": "mylocal", "Version": "0.0.1", "Source": "Local", "RemoteType": "local", "RemoteUrl": "C:/Users/alice/pkgs/mylocal" },
    "orphan": { "Package": "orphan", "Version": "1.0", "Source": "Repository", "Repository": "Gone" }
  }
}"#;

    #[test]
    fn lockfile_resolves_sources_and_requirements() {
        let items = Renv.parse_file(&file("renv.lock", LOCK)).unwrap();
        assert_eq!(
            items[0].detail,
            "Lockfile · R 4.4.2 · Bioconductor 3.20 · 2 Repositories (davon 1 nicht öffentlich)"
        );
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let markdown = find("markdown");
        assert_eq!((markdown.version.as_deref(), markdown.install_state), (Some("1.0"), Some(InstallState::Resolved)));
        assert_eq!(
            (markdown.source, markdown.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("cloud.r-project.org"))
        );
        let edges: Vec<&str> = markdown.evidence.dependencies.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(edges, ["utils", "xfun"]);
        let ggplot = find("ggplot2");
        let edges: Vec<(&str, &str)> =
            ggplot.evidence.dependencies.iter().map(|d| (d.name.as_str(), d.requirement.as_str())).collect();
        assert_eq!(
            edges,
            [("cli", ""), ("grDevices", ""), ("gtable", ">= 0.1.1"), ("rlang", ">= 1.1.0")],
            "R and Suggests are no edges"
        );
        let corp = find("corpstats");
        assert_eq!(
            (corp.source, corp.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("rpkgs.corp.example"))
        );
        let mime = find("mime");
        assert_eq!((mime.source, mime.source_host.as_deref()), (SourceClass::Git, Some("github.com")));
        assert_eq!(mime.detail, "aufgelöst laut renv.lock · GitHub yihui/mime · Ref main · Commit 1763e0dcb72f");
        let biobase = find("Biobase");
        assert_eq!(
            (biobase.source, biobase.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("bioconductor.org"))
        );
        assert_eq!(biobase.evidence.dependencies.len(), 2);
        assert_eq!(find("mylocal").source, SourceClass::Path);
        let orphan = find("orphan");
        assert_eq!(orphan.source, SourceClass::Unknown);
        assert!(orphan.detail.ends_with("Repository Gone (nicht aufgeführt)"));
        let text = format!("{items:?}");
        for secret in ["s3cr3t", "reader", "/latest", "alice", "git.bioconductor.org", "4584a57f", "thomas.pedersen"] {
            assert!(!text.contains(secret), "{secret} must not be stored");
        }
    }

    #[test]
    fn broken_lockfile_is_an_error() {
        assert!(Renv.parse_file(&file("renv.lock", "{\"R\": {")).is_err());
        assert!(Renv.parse_file(&file("renv.lock", "{\"R\": {\"Version\": \"4.3.1\"}}")).is_err());
    }

    const DESCRIPTION: &str = "Package: tidyexample
Title: Tidy Example Tools
Version: 0.2.0.9000
Authors@R:
    person(\"Jane\", \"Doe\", , \"jane@example.com\", role = c(\"aut\", \"cre\"))
Description: Tools for examples, spanning
    more than one line.
License: MIT + file LICENSE
Depends:
    R (>= 4.1.0)
Imports:
    dplyr (>= 1.1.0),
    rlang,
    Rcpp (>= 1.0.10),
    corpdata,
    stats
LinkingTo:
    Rcpp
Suggests:
    testthat (>= 3.0.0),
    knitr
Remotes:
    tidyverse/dplyr,
    corpdata=git::https://ci:hunter2@git.corp.example/r/corpdata-pkg.git,
    url::https://cran.r-project.org/src/contrib/Archive/knitr/knitr_1.45.tar.gz,
    local::../extras/helper
Config/testthat/edition: 3
Encoding: UTF-8
";

    #[test]
    fn description_declares_fields_and_remotes() {
        let items = Renv.parse_file(&file("DESCRIPTION", DESCRIPTION)).unwrap();
        assert_eq!(items[0].detail, "R-Paket tidyexample 0.2.0.9000 · erfordert R >= 4.1.0");
        let find = |name: &str, context: &str| {
            items
                .iter()
                .find(|i| i.name == name && i.evidence.context == context)
                .unwrap_or_else(|| panic!("{name} {context}: {items:#?}"))
        };
        let dplyr = find("dplyr", "imports");
        assert_eq!(dplyr.detail, "Spezifikation >= 1.1.0 · Imports · Quelle laut Remotes: Git");
        assert_eq!((dplyr.source, dplyr.source_host.as_deref()), (SourceClass::Git, Some("github.com")));
        assert_eq!(dplyr.install_state, Some(InstallState::Declared));
        assert_eq!(find("rlang", "imports").source, SourceClass::Unknown);
        assert!(find("Rcpp", "imports").detail.starts_with("Spezifikation >= 1.0.10"));
        assert_eq!(find("Rcpp", "linkingto").detail, "ohne Versionsangabe · LinkingTo");
        let corp = find("corpdata", "imports");
        assert_eq!((corp.source, corp.source_host.as_deref()), (SourceClass::Git, Some("git.corp.example")));
        let knitr = find("knitr", "suggests");
        assert_eq!(
            (knitr.source, knitr.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("cran.r-project.org"))
        );
        assert_eq!(find("testthat", "suggests").detail, "Spezifikation >= 3.0.0 · Suggests");
        let helper = find("helper", "remotes");
        assert_eq!(helper.source, SourceClass::Path);
        assert!(!items.iter().any(|i| i.name == "R" || i.name == "Jane"));
        assert_eq!(items.len(), 10, "{items:#?}");
        let text = format!("{items:?}");
        for secret in ["hunter2", "corpdata-pkg", "Archive/knitr", "../extras", "jane@example.com"] {
            assert!(!text.contains(secret), "{secret} must not be stored");
        }
    }

    #[test]
    fn foreign_description_files_are_ignored() {
        let git = "Unnamed repository; edit this file 'description' to name the repository.\n";
        assert!(Renv.parse_file(&file("description", git)).unwrap().is_empty());
        assert!(Renv.parse_file(&file("DESCRIPTION", "Package: x\nVersion: 1.0\n")).unwrap().is_empty());
        assert!(Renv.parse_file(&file("DESCRIPTION", "Imports: dplyr\n")).unwrap().is_empty(), "no Package field");
        let installed = "Package: rlang\nVersion: 1.1.4\nImports: utils\nBuilt: R 4.4.1; x86_64-w64-mingw32; 2024-06-04 10:12:00 UTC; windows\n";
        assert!(
            Renv.parse_file(&file("DESCRIPTION", installed)).unwrap().is_empty(),
            "installed copies are no manifests"
        );
        assert!(
            Renv.parse_file(&file("DESCRIPTION", "  indented first line\nPackage: x\nImports: y\n"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn project_library_beside_activate_script() {
        let siblings = ["activate.R".to_string(), "settings.json".to_string()];
        assert!(Renv.claim_dir("library", &siblings, &["windows".into()]).is_some());
        assert!(Renv.claim_dir("library", &siblings, &["R-4.1".into()]).is_some());
        assert!(Renv.claim_dir("library", &["notes.md".into()], &["R-4.1".into()]).is_none());
        assert!(Renv.claim_dir("library", &siblings, &["books".into()]).is_none());
        let mut dir = FakeDir::default();
        dir.dirs.insert(String::new(), vec![("windows".into(), true), ("R-4.1".into(), true)]);
        dir.dirs.insert("windows".into(), vec![("R-4.4".into(), true)]);
        dir.dirs.insert("windows/R-4.4".into(), vec![("x86_64-w64-mingw32".into(), true)]);
        dir.dirs.insert(
            "windows/R-4.4/x86_64-w64-mingw32".into(),
            vec![("rlang".into(), true), ("00LOCK-cli".into(), true), ("cli".into(), true), (".renv".into(), true)],
        );
        dir.files.insert(
            "windows/R-4.4/x86_64-w64-mingw32/rlang/DESCRIPTION".into(),
            "Package: rlang\nVersion: 1.1.4\nRepository: CRAN\n".into(),
        );
        dir.dirs.insert("R-4.1".into(), vec![("x86_64-w64-mingw32".into(), true)]);
        dir.dirs.insert("R-4.1/x86_64-w64-mingw32".into(), vec![("rlang".into(), true)]);
        let claim = Renv.claim_dir("library", &siblings, &["windows".into()]).unwrap();
        let items = Renv.inspect_dir(claim, "renv/library", &mut dir).unwrap();
        assert_eq!(items[0].detail, "R 4.1, 4.4 · 3 Pakete als Ordner · verknüpfte Cache-Einträge nicht verfolgt");
        let rlang: Vec<_> = items.iter().filter(|i| i.name == "rlang").collect();
        assert_eq!(rlang.len(), 2, "one per R version");
        assert_ne!(rlang[0].key(), rlang[1].key());
        let current = rlang.iter().find(|i| i.evidence.qualifier.starts_with("windows")).unwrap();
        assert_eq!((current.version.as_deref(), current.install_state), (Some("1.1.4"), Some(InstallState::Installed)));
        let cli = items.iter().find(|i| i.name == "cli").unwrap();
        assert_eq!((cli.version.as_deref(), cli.detail.as_str()), (None, "Ordner vorhanden; Version unbekannt"));
    }
}
