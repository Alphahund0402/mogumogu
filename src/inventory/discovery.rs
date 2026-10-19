//! Filename-only discovery within an approved root. Findings carry their
//! evidence and spatial context; no manifest or discovered instruction is read.
use crate::domain::{DiscoveryEvidence, DiscoveryKind, InventoryItem, ItemCategory};
use crate::fsread::{self, Budget};
use crate::platform::{DirEntryInfo, DirHandle};
use crate::storage::GenerationResult;
use std::ffi::OsStr;
use std::rc::Rc;

const SKIP: &[&str] = &[".git", ".hg", ".svn", "$RECYCLE.BIN", "System Volume Information"];

struct Pending {
    parent: Rc<DirHandle>,
    name: Option<String>,
    rel: String,
    depth: u32,
    project: Option<String>,
}

pub(super) fn scan(root: Rc<DirHandle>, budget: &mut Budget) -> GenerationResult {
    let mut result = GenerationResult::default();
    let mut pending = vec![Pending { parent: root, name: None, rel: String::new(), depth: 0, project: None }];
    let mut links = 0_u64;
    let mut placeholders = 0_u64;
    let mut excluded = 0_u64;
    while let Some(next) = pending.pop() {
        if budget.exhausted() {
            break;
        }
        let dir = match next.name {
            Some(name) => match next.parent.open_dir(OsStr::new(&name)) {
                Ok(child) => Rc::new(child),
                Err(e) => {
                    report(&mut result, &next.rel, e);
                    continue;
                }
            },
            None => next.parent,
        };
        let entries = match fsread::list(&dir, budget) {
            Ok((entries, _)) => entries,
            Err(e) => {
                report(&mut result, &next.rel, e);
                continue;
            }
        };
        let inspection = inspect_directory(&next.rel, &entries, next.project.as_deref());
        result.items.extend(inspection.items);
        for entry in entries {
            if !budget.entry() {
                break;
            }
            if entry.is_reparse() {
                links += 1;
                continue;
            }
            if entry.is_placeholder() {
                placeholders += 1;
                continue;
            }
            let Some(name) = entry.name_str() else {
                report(&mut result, &next.rel, "Dateiname nicht als Unicode darstellbar");
                continue;
            };
            if !entry.is_dir() {
                continue;
            }
            if inspection.leaf || SKIP.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                excluded += 1;
                continue;
            }
            if next.depth >= budget.limits().max_depth {
                budget.limit_reason.get_or_insert_with(|| "Tiefenlimit bei Entwicklungssuche erreicht".into());
                continue;
            }
            pending.push(Pending {
                parent: Rc::clone(&dir),
                name: Some(name.to_string()),
                rel: fsread::join(&next.rel, name),
                depth: next.depth + 1,
                project: inspection.project.clone(),
            });
        }
    }
    result.notes.push(format!(
        "Nur Namen/Metadaten; keine Nutzung oder Entbehrlichkeit belegt. {links} Links, {placeholders} Platzhalter und {excluded} Unterordner ausgelassen."
    ));
    result
}

/// Shared with the ordinary project scanner, without a second traversal or
/// content read. Adapter-claimed stores remain bounded leaves in discovery.
pub(super) struct DirectoryInspection {
    pub items: Vec<InventoryItem>,
    pub project: Option<String>,
    pub leaf: bool,
}

pub(super) fn inspect_directory(
    rel: &str,
    entries: &[DirEntryInfo],
    inherited_project: Option<&str>,
) -> DirectoryInspection {
    let safe = entries.iter().filter(|e| !e.is_reparse() && !e.is_placeholder());
    let markers = safe.clone().filter_map(project_marker).take(12).collect::<Vec<_>>();
    let mut project = if markers.is_empty() { inherited_project.map(str::to_string) } else { Some(rel.to_string()) };
    let mut items = Vec::new();
    let base = rel.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    let has_file =
        |name: &str| safe.clone().any(|e| !e.is_dir() && e.name_str().is_some_and(|n| n.eq_ignore_ascii_case(name)));
    let dev_file = safe.clone().any(|e| !e.is_dir() && e.name_str().is_some_and(is_source));
    let has_dev_context = project.is_some() || dev_file;
    let environment = has_file("pyvenv.cfg");
    let directory_kind = if environment {
        Some((DiscoveryKind::Environment, "Umgebungsmarker pyvenv.cfg", true))
    } else if base == "node_modules" {
        Some((DiscoveryKind::Dependencies, "Ordnername node_modules; Installationszustand nicht geprüft", true))
    } else if matches!(base.as_str(), ".npm" | "_cacache" | ".pnpm-store" | ".uv-cache") {
        Some((DiscoveryKind::Cache, "Werkzeugtypischer Cachename; gemeinsame Nutzung unbekannt", true))
    } else if has_dev_context {
        directory_hint(&base)
    } else {
        None
    };
    if directory_kind.is_some_and(|(kind, _, _)| kind != DiscoveryKind::Scratchpad) {
        project = inherited_project.map(str::to_string);
    }
    // Recognize environments, stores and outputs before checking manifest
    // markers: a dependency's package.json is not a new user project.
    if let Some((kind, evidence, _)) = directory_kind {
        items.push(finding(kind, rel, true, None, None, project.as_deref(), evidence));
    } else if !markers.is_empty() {
        items.push(finding(
            DiscoveryKind::Project,
            rel,
            true,
            None,
            None,
            Some(rel),
            &format!("Projektmarker: {}. Nur Namen; Inhalt und Nutzung unbekannt.", markers.join(", ")),
        ));
    }
    let leaf = directory_kind.is_some_and(|(_, _, leaf)| leaf);
    if !leaf {
        for entry in safe.filter(|e| !e.is_dir()) {
            let Some(name) = entry.name_str() else { continue };
            let lower = name.to_ascii_lowercase();
            let kind = if lower.ends_with(".ipynb") {
                Some((DiscoveryKind::Notebook, "Dateiendung .ipynb; Inhalt und Zweck unbekannt"))
            } else if project.is_none() && is_source(name) {
                Some((DiscoveryKind::Script, "Quellcode-/Skriptendung ohne gefundenen Projektmarker"))
            } else if has_dev_context && [".sqlite", ".sqlite3", ".db", ".duckdb"].iter().any(|e| lower.ends_with(e)) {
                Some((DiscoveryKind::Database, "Datenbankendung im Entwicklungskontext; Testzweck unbekannt"))
            } else {
                None
            };
            if let Some((kind, evidence)) = kind {
                items.push(finding(
                    kind,
                    &fsread::join(rel, name),
                    false,
                    i64::try_from(entry.size).ok(),
                    Some(entry.modified),
                    project.as_deref(),
                    evidence,
                ));
            }
        }
    }
    DirectoryInspection { items, project, leaf }
}

fn project_marker(entry: &DirEntryInfo) -> Option<String> {
    let name = entry.name_str()?;
    let lower = name.to_ascii_lowercase();
    let matches = if entry.is_dir() {
        matches!(lower.as_str(), ".git" | ".hg" | ".svn")
    } else {
        matches!(
            lower.as_str(),
            "cargo.toml"
                | "package.json"
                | "pyproject.toml"
                | "uv.lock"
                | "requirements.txt"
                | "setup.py"
                | "setup.cfg"
                | "pipfile"
                | "go.mod"
                | "pom.xml"
                | "build.gradle"
                | "build.gradle.kts"
                | "composer.json"
                | "gemfile"
                | "mix.exs"
                | "pubspec.yaml"
                | "cmakelists.txt"
                | "makefile"
                | "packages.config"
                | ".git"
        ) || [".csproj", ".fsproj", ".vbproj", ".sln", ".slnx", ".code-workspace"].iter().any(|e| lower.ends_with(e))
    };
    matches.then(|| name.to_string())
}

fn is_source(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        ".py", ".pyw", ".ps1", ".sh", ".bash", ".bat", ".cmd", ".js", ".mjs", ".cjs", ".ts", ".rb", ".pl", ".lua",
        ".sql", ".r", ".rs", ".go", ".java", ".cs", ".cpp", ".c", ".ipynb",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

fn directory_hint(base: &str) -> Option<(DiscoveryKind, &'static str, bool)> {
    match base {
        "scratch" | "scratchpad" | "scratchpads" | "experiment" | "experiments" | "playground" | "sandbox" => Some((
            DiscoveryKind::Scratchpad,
            "Experimenttypischer Name mit Entwicklungshinweisen; Zweck unbekannt",
            false,
        )),
        ".venv" | "venv" | ".tox" | ".nox" | "conda-meta" => Some((
            DiscoveryKind::Environment,
            "Umgebungstypischer Name; kein Installations- oder Nutzungsnachweis",
            true,
        )),
        "target" | "dist" | "build" | "bin" | "obj" | "out" | ".next" | ".nuxt" => {
            Some((DiscoveryKind::BuildOutput, "Ausgabetypischer Name im Entwicklungskontext; Erzeuger unbekannt", true))
        }
        "coverage" | "test-results" | "test-output" | "playwright-report" | "htmlcov" => {
            Some((DiscoveryKind::TestOutput, "Testausgabetypischer Name; Wiederherstellbarkeit unbekannt", true))
        }
        "testdata" | "test-data" | "fixtures" | "__fixtures__" => Some((
            DiscoveryKind::TestData,
            "Testdatentypischer Name; kann selbst gepflegte Quelldaten enthalten",
            false,
        )),
        ".cache" | "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" | ".gradle" | ".turbo"
        | ".parcel-cache" => {
            Some((DiscoveryKind::Cache, "Cachetypischer Name im Entwicklungskontext; Nutzung unbekannt", true))
        }
        _ => None,
    }
}

/// A project scanner may safely identify a known cache/environment by its
/// name while retaining its existing rule to never enter that directory.
pub(super) fn skipped_directory(rel: &str) -> Option<InventoryItem> {
    let base = rel.rsplit('/').next()?.to_ascii_lowercase();
    let (kind, evidence, _) = directory_hint(&base)?;
    Some(finding(kind, rel, true, None, None, Some(""), evidence))
}

fn finding(
    kind: DiscoveryKind,
    rel: &str,
    is_dir: bool,
    bytes: Option<i64>,
    modified_at: Option<i64>,
    project: Option<&str>,
    detail: &str,
) -> InventoryItem {
    let category = match kind {
        DiscoveryKind::Environment | DiscoveryKind::Dependencies => ItemCategory::Environment,
        DiscoveryKind::Cache => ItemCategory::Cache,
        DiscoveryKind::BuildOutput | DiscoveryKind::TestOutput | DiscoveryKind::TestData | DiscoveryKind::Database => {
            ItemCategory::Output
        }
        _ => ItemCategory::Manifest,
    };
    let name = rel.rsplit('/').find(|s| !s.is_empty()).unwrap_or(".");
    let mut item = InventoryItem::new(category, "discovery", name, rel).detail(detail);
    item.bytes = bytes;
    item.evidence.context = project.unwrap_or("").to_string();
    item.evidence.discovery = Some(DiscoveryEvidence {
        kind,
        is_dir,
        modified_at,
        has_project_context: project.is_some(),
        path_exact: crate::privacy::label(rel) == rel,
    });
    item
}

fn report(result: &mut GenerationResult, rel: &str, error: impl std::fmt::Display) {
    if result.errors.len() < 50 {
        result.errors.push(crate::privacy::label(&format!("{rel}: {error}")));
    }
}
