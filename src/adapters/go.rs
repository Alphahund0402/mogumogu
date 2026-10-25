//! Go modules: go.mod (declared: direct, indirect and `tool` dependencies),
//! go.sum and go.work.sum (versions with a checksum of the module content),
//! go.work (workspace modules and replacements as edges) and
//! vendor/modules.txt (installed). Nothing is downloaded or built. GOPROXY,
//! GOPRIVATE and GOFLAGS are not read, so sources stay unknown except local
//! `replace` targets.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, PackageDependency, SourceClass};
use crate::limits::PACKAGE_FILE_MAX_BYTES;
use std::collections::{BTreeMap, BTreeSet};

pub struct Go;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "go",
    name: "Go-Module",
    markers: &["go.mod", "go.sum", "go.work", "go.work.sum"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "go.mod (require, replace, tool, exclude, retract)",
        "go.sum",
        "go.work (use, replace)",
        "go.work.sum",
        "vendor/modules.txt",
    ],
    limits: "Kein go-Aufruf; Modulcache außerhalb des Projekts nicht gelesen; go.sum und go.work.sum können ältere, nicht mehr genutzte Versionen enthalten; Werkzeuge nur mit der Version ihrer require-Zeile.",
};

impl Adapter for Go {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        match file.name {
            "go.mod" => bounded(go_mod(file)),
            "go.work" => bounded(go_work(file)),
            "go.work.sum" => bounded(go_sum(file, "workspace")),
            _ => bounded(go_sum(file, "")),
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], children: &[String]) -> Option<DirClaim> {
        let module = siblings.iter().any(|s| s == "go.mod" || s == "go.work");
        let vendored = module && children.iter().any(|c| c == "modules.txt");
        (name == "vendor" && vendored).then_some(DirClaim {
            adapter: "go",
            category: ItemCategory::Environment,
            label: "vendor (Go)",
        })
    }

    fn inspect_dir(&self, _claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let mut items = vec![
            InventoryItem::new(ItemCategory::Environment, "go", "vendor", rel_path)
                .detail("Mitgelieferte Module laut modules.txt; nicht übersetzt"),
        ];
        let text = access.read_text(&["modules.txt"], PACKAGE_FILE_MAX_BYTES).unwrap_or_default();
        for line in text.lines().take(200_000) {
            // `# path version` or `# path version => replacement`.
            let Some(rest) = line.strip_prefix("# ") else { continue };
            let mut parts = rest.split_whitespace();
            let (Some(path), Some(version)) = (parts.next(), parts.next()) else { continue };
            if version == "=>" {
                continue;
            }
            items.push(
                InventoryItem::new(ItemCategory::Package, "go", path, format!("{rel_path}/modules.txt"))
                    .version(version)
                    .state(InstallState::Installed)
                    .detail("im vendor-Ordner mitgeliefert"),
            );
        }
        bounded(items)
    }
}

/// Calls `visit(verb, arguments, comment)` for every directive, also for
/// the lines of a `verb ( … )` block.
fn directives(text: &str, mut visit: impl FnMut(&str, &str, &str)) {
    let mut block: Option<&str> = None;
    for raw in text.lines().take(200_000) {
        let (code, comment) = raw.split_once("//").unwrap_or((raw, ""));
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        match block {
            Some(_) if code == ")" => block = None,
            Some(verb) => visit(verb, code, comment),
            None => {
                let (verb, rest) = code.split_once(char::is_whitespace).unwrap_or((code, ""));
                if rest.trim() == "(" {
                    block = Some(verb);
                } else {
                    visit(verb, rest.trim(), comment);
                }
            }
        }
    }
}

/// `old [version] => new [version]`: the old path, the new target and its
/// version (none for a directory).
fn replacement(rest: &str) -> Option<(String, String, Option<String>)> {
    let (old, new) = rest.split_once("=>")?;
    let old = old.split_whitespace().next()?.trim_matches('"').to_string();
    let mut new = new.split_whitespace().map(|w| w.trim_matches('"'));
    let target = new.next()?.to_string();
    Some((old, target, new.next().map(str::to_string)))
}

fn go_mod(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let (mut module, mut go) = ("?".to_string(), "?".to_string());
    let mut requires: Vec<(String, String, bool)> = Vec::new();
    let mut replaced: BTreeMap<String, String> = BTreeMap::new();
    let mut tools: Vec<String> = Vec::new();
    let (mut excluded, mut retracted) = (0, 0);
    directives(file.content, |verb, rest, comment| {
        let mut words = rest.split_whitespace().map(|w| w.trim_matches('"'));
        match verb {
            "module" => module = words.next().unwrap_or("?").to_string(),
            "go" => go = words.next().unwrap_or("?").to_string(),
            "require" => {
                if let (Some(path), Some(version)) = (words.next(), words.next()) {
                    requires.push((path.to_string(), version.to_string(), comment.contains("indirect")));
                }
            }
            "replace" => {
                if let Some((old, new, _)) = replacement(rest) {
                    replaced.entry(old).or_insert(new);
                }
            }
            "tool" => {
                if let Some(path) = words.next().filter(|_| tools.len() < 1_000) {
                    tools.push(path.to_string());
                }
            }
            "exclude" => excluded += 1,
            "retract" => retracted += 1,
            _ => {}
        }
    });
    let mut detail = format!("Modul {module} · Go {go}");
    for (count, label) in
        [(tools.len(), "Werkzeuge"), (excluded, "ausgeschlossene Versionen"), (retracted, "zurückgezogene Versionen")]
    {
        if count > 0 {
            detail.push_str(&format!(" · {count} {label}"));
        }
    }
    let mut items = vec![manifest("go", file, detail)];
    items[0].evidence.dependencies = requires
        .iter()
        .filter(|(.., indirect)| !indirect)
        .map(|(path, version, _)| PackageDependency {
            name: path.clone(),
            requirement: version.clone(),
            kind: "require".into(),
            exact: false,
        })
        .collect();
    let replacement_of = |path: &str| replaced.get(path).map(String::as_str);
    for (path, version, indirect) in &requires {
        let replacement = replacement_of(path);
        let local = replacement.is_some_and(local_path);
        let mut detail = format!("Mindestversion {version}");
        if *indirect {
            detail.push_str(" · indirekt");
        }
        match replacement {
            Some(_) if local => detail.push_str(" · ersetzt durch lokalen Ordner"),
            Some(new) => detail.push_str(&format!(" · ersetzt durch {new}")),
            None => {}
        }
        let source = if local { SourceClass::Path } else { SourceClass::Unknown };
        let mut item = InventoryItem::new(ItemCategory::Package, "go", path.as_str(), file.rel_path())
            .version(version.as_str())
            .state(InstallState::Declared)
            .source(source, None)
            .detail(detail);
        item.evidence.context = if *indirect { "indirect" } else { "direct" }.into();
        items.push(item);
    }
    let required: BTreeMap<&str, &str> =
        requires.iter().map(|(path, version, _)| (path.as_str(), version.as_str())).collect();
    // Real package paths are short; a long one would cost a lookup per `/`.
    for tool in tools.iter().filter(|tool| tool.len() <= 512) {
        // The module that provides the tool's package: its longest required prefix.
        let owner = std::iter::once(tool.as_str())
            .chain(tool.match_indices('/').rev().map(|(at, _)| &tool[..at]))
            .find_map(|prefix| required.get_key_value(prefix));
        let own = tool == &module || tool.strip_prefix(module.as_str()).is_some_and(|rest| rest.starts_with('/'));
        if own && owner.is_none_or(|(path, _)| path.len() <= module.len()) {
            continue; // a command of this module itself
        }
        let mut item = match owner {
            Some((path, version)) => {
                let local = replacement_of(path).is_some_and(local_path);
                let source = if local { SourceClass::Path } else { SourceClass::Unknown };
                InventoryItem::new(ItemCategory::Package, "go", *path, file.rel_path())
                    .version(*version)
                    .source(source, None)
                    .detail(format!("Werkzeug {tool} · Mindestversion {version}"))
            }
            None => InventoryItem::new(ItemCategory::Package, "go", tool.as_str(), file.rel_path())
                .detail(format!("Werkzeug {tool} · Modul ohne require-Zeile")),
        }
        .state(InstallState::Declared);
        item.evidence.context = "tool".into();
        item.evidence.qualifier = tool.clone();
        item.evidence.executables = vec![command_name(tool).to_string()];
        items.push(item);
    }
    items
}

/// The command `go tool` runs: the last path element, skipping a major
/// version suffix such as `/v2`.
fn command_name(path: &str) -> &str {
    let mut parts = path.rsplit('/');
    let last = parts.next().unwrap_or(path);
    let major = last.strip_prefix('v').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    match parts.next() {
        Some(previous) if major => previous,
        _ => last,
    }
}

fn go_work(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let (mut go, mut toolchain) = (None, None);
    let mut edges: Vec<PackageDependency> = Vec::new();
    let mut seen = BTreeSet::new();
    let (mut modules, mut outside, mut replacements) = (0, 0, 0);
    directives(file.content, |verb, rest, _| {
        let first = rest.split_whitespace().next().map(|w| w.trim_matches('"'));
        match verb {
            "go" => go = first.map(str::to_string),
            "toolchain" => toolchain = first.map(str::to_string),
            "use" => match first.and_then(super::maven::relative_path) {
                Some(dir) if seen.insert(dir.clone()) => {
                    modules += 1;
                    edges.push(PackageDependency { name: dir, kind: "use".into(), ..PackageDependency::default() });
                }
                Some(_) => {}
                None => outside += usize::from(first.is_some()),
            },
            "replace" => {
                if let Some((old, new, version)) = replacement(rest) {
                    replacements += 1;
                    let requirement = match version {
                        _ if local_path(&new) => "lokaler Ordner".to_string(),
                        Some(version) => format!("{new} {version}"),
                        None => new,
                    };
                    edges.push(PackageDependency { name: old, requirement, kind: "replace".into(), exact: false });
                }
            }
            _ => {}
        }
    });
    let mut detail = format!("Go-Workspace · Go {}", go.as_deref().unwrap_or("?"));
    if let Some(toolchain) = toolchain {
        detail.push_str(&format!(" · Toolchain {toolchain}"));
    }
    detail.push_str(&format!(" · {modules} Module"));
    if outside > 0 {
        detail.push_str(&format!(" · {outside} außerhalb (Pfad nicht gespeichert)"));
    }
    if replacements > 0 {
        detail.push_str(&format!(" · {replacements} Ersetzungen"));
    }
    let mut manifest = manifest("go", file, detail);
    manifest.evidence.dependencies = edges;
    vec![manifest]
}

fn go_sum(file: &FoundFile<'_>, context: &str) -> Vec<InventoryItem> {
    let detail = if context.is_empty() { "Prüfsummen der Module" } else { "Prüfsummen der Workspace-Module" };
    let mut items = vec![manifest("go", file, detail)];
    let mut seen = BTreeSet::new();
    for line in file.content.lines().take(500_000) {
        let mut parts = line.split_whitespace();
        let (Some(path), Some(version)) = (parts.next(), parts.next()) else { continue };
        // `/go.mod` lines only cover the module's go.mod, not its code.
        if version.ends_with("/go.mod") || !seen.insert((path, version)) {
            continue;
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "go", path, file.rel_path())
            .version(version)
            .state(InstallState::Resolved)
            .detail(format!("Prüfsumme des Modulinhalts in {}", file.name));
        item.evidence.context = context.into();
        items.push(item);
    }
    items
}

/// A `replace` target on disk: `./x`, `../x`, `.`, `..` or an absolute
/// path, also `\`-rooted, UNC (`\\host\share`) or a bare drive (`D:`).
fn local_path(target: &str) -> bool {
    let bytes = target.as_bytes();
    matches!(target, "." | "..")
        || target.starts_with("./")
        || target.starts_with("../")
        || target.starts_with(".\\")
        || target.starts_with("..\\")
        || target.starts_with(['/', '\\'])
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const GO_MOD: &str = "module example.com/app\n\ngo 1.22\n\ntoolchain go1.22.4\n\nrequire (\n\tgithub.com/foo/bar v1.2.3\n\tgolang.org/x/sys v0.15.0 // indirect\n\tgithub.com/remyoudompheng/bigfft v0.0.0-20230129092748-24d4a6f8daec // indirect\n)\n\nrequire github.com/single/mod v0.1.0\n\nreplace github.com/foo/bar => ../bar\nreplace github.com/single/mod v0.1.0 => github.com/fork/mod v0.1.1\n";

    #[test]
    fn go_mod_requires_with_indirect_and_replacements() {
        let items = Go.parse_file(&file("go.mod", GO_MOD)).unwrap();
        assert_eq!(items[0].detail, "Modul example.com/app · Go 1.22");
        assert_eq!(items[0].evidence.dependencies.len(), 2, "only direct requirements are edges");
        let bar = items.iter().find(|i| i.name == "github.com/foo/bar").unwrap();
        assert_eq!((bar.version.as_deref(), bar.source), (Some("v1.2.3"), SourceClass::Path));
        assert_eq!(bar.evidence.context, "direct");
        let sys = items.iter().find(|i| i.name == "golang.org/x/sys").unwrap();
        assert!(sys.detail.contains("indirekt") && sys.source == SourceClass::Unknown);
        let pseudo = items.iter().find(|i| i.name == "github.com/remyoudompheng/bigfft").unwrap();
        assert_eq!(pseudo.version.as_deref(), Some("v0.0.0-20230129092748-24d4a6f8daec"));
        let single = items.iter().find(|i| i.name == "github.com/single/mod").unwrap();
        assert!(single.detail.ends_with("ersetzt durch github.com/fork/mod"), "{}", single.detail);
        assert!(items.iter().all(|i| i.install_state != Some(InstallState::Resolved)));
    }

    #[test]
    fn go_sum_keeps_module_content_once_and_vendor_is_installed() {
        let sum = "github.com/foo/bar v1.2.3 h1:abc=\ngithub.com/foo/bar v1.2.3/go.mod h1:def=\ngithub.com/old/dep v0.9.0/go.mod h1:ghi=\n";
        let items = Go.parse_file(&file("go.sum", sum)).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].install_state, Some(InstallState::Resolved));
        assert_eq!(items[1].version.as_deref(), Some("v1.2.3"));

        let mut dir = FakeDir::default();
        dir.files.insert(
            "modules.txt".into(),
            "# github.com/foo/bar v1.2.3\n## explicit; go 1.20\ngithub.com/foo/bar\n# example.com/local => ../local\n"
                .into(),
        );
        assert!(Go.claim_dir("vendor", &["main.go".into()], &["modules.txt".into()]).is_none());
        let claim = Go.claim_dir("vendor", &["go.mod".into()], &["modules.txt".into()]).unwrap();
        let items = Go.inspect_dir(claim, "vendor", &mut dir).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].install_state, Some(InstallState::Installed));
        assert!(Go.claim_dir("vendor", &["go.work".into()], &["modules.txt".into()]).is_some(), "workspace vendoring");
    }

    #[test]
    fn tool_directives_take_the_version_of_their_module() {
        let text = "module example.com/app\n\ngo 1.24\n\ntool golang.org/x/tools/cmd/stringer\n\ntool (\n\tgithub.com/golangci/golangci-lint/v2/cmd/golangci-lint\n\texample.com/app/cmd/gen\n\tgithub.com/air-verse/air\n\tgolang.org/x/tools/cmd/goimports // formatting\n)\n\nrequire (\n\tgithub.com/golangci/golangci-lint/v2 v2.1.6 // indirect\n\tgolang.org/x/tools v0.33.0 // indirect\n)\n\nexclude golang.org/x/net v1.2.3\nexclude (\n\tgolang.org/x/crypto v1.4.5\n)\nretract [v1.0.0, v1.9.9]\nretract (\n\tv2.0.0 // Published accidentally.\n)\ngodebug default=go1.21\nignore ./node_modules\n";
        let items = Go.parse_file(&file("go.mod", text)).unwrap();
        assert_eq!(
            items[0].detail,
            "Modul example.com/app · Go 1.24 · 5 Werkzeuge · 2 ausgeschlossene Versionen · 2 zurückgezogene Versionen"
        );
        assert!(items[0].evidence.dependencies.is_empty(), "indirect requirements are no edges");
        let tools: Vec<&InventoryItem> = items.iter().filter(|i| i.evidence.context == "tool").collect();
        assert_eq!(tools.len(), 4, "the module's own command is no dependency");
        let stringer = tools.iter().find(|i| i.evidence.qualifier == "golang.org/x/tools/cmd/stringer").unwrap();
        assert_eq!((stringer.name.as_str(), stringer.version.as_deref()), ("golang.org/x/tools", Some("v0.33.0")));
        assert_eq!(stringer.evidence.executables, ["stringer"]);
        assert_eq!(stringer.install_state, Some(InstallState::Declared));
        let lint = tools.iter().find(|i| i.evidence.executables == ["golangci-lint"]).unwrap();
        assert_eq!(
            (lint.name.as_str(), lint.version.as_deref()),
            ("github.com/golangci/golangci-lint/v2", Some("v2.1.6"))
        );
        let air = tools.iter().find(|i| i.name == "github.com/air-verse/air").unwrap();
        assert!(air.version.is_none() && air.detail.ends_with("Modul ohne require-Zeile"));
        let keys: BTreeSet<String> = items.iter().map(InventoryItem::key).collect();
        assert_eq!(keys.len(), items.len(), "two tools of one module stay apart");
        assert_eq!(command_name("example.com/cmd/v2"), "cmd");
        assert_eq!(command_name("example.com/v2x"), "v2x");
    }

    #[test]
    fn workspace_file_lists_modules_and_replacements() {
        let text = "go 1.23.0\n\ntoolchain go1.23.2\n\nuse ./hello\nuse (\n\t./example\n\t../shared // sibling\n\t.\n\t\"./tools/gen\"\n\tC:\\Users\\dev\\other\n\t./hello\n)\n\nreplace golang.org/x/net v1.2.3 => example.com/fork/net v1.4.5\nreplace (\n\tgolang.org/x/text => ./fork/text\n)\ngodebug (\n\tpanicnil=1\n)\n";
        let items = Go.parse_file(&file("go.work", text)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].detail,
            "Go-Workspace · Go 1.23.0 · Toolchain go1.23.2 · 5 Module · 1 außerhalb (Pfad nicht gespeichert) · 2 Ersetzungen"
        );
        let edges: Vec<(&str, &str, &str)> = items[0]
            .evidence
            .dependencies
            .iter()
            .map(|d| (d.kind.as_str(), d.name.as_str(), d.requirement.as_str()))
            .collect();
        assert_eq!(
            edges,
            [
                ("use", "hello", ""),
                ("use", "example", ""),
                ("use", "../shared", ""),
                ("use", ".", ""),
                ("use", "tools/gen", ""),
                ("replace", "golang.org/x/net", "example.com/fork/net v1.4.5"),
                ("replace", "golang.org/x/text", "lokaler Ordner"),
            ]
        );
        assert!(!format!("{items:?}").contains("Users"), "absolute paths are not kept");

        let sum = "golang.org/x/net v0.30.0 h1:abc=\ngolang.org/x/net v0.30.0/go.mod h1:def=\n";
        let items = Go.parse_file(&file("go.work.sum", sum)).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].detail, "Prüfsummen der Workspace-Module");
        assert_eq!(
            (items[1].evidence.context.as_str(), items[1].install_state),
            ("workspace", Some(InstallState::Resolved))
        );
        assert_eq!(items[1].detail, "Prüfsumme des Modulinhalts in go.work.sum");
    }
}
