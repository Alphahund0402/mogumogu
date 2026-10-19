//! Filename-only discovery within an approved root. Findings carry their
//! evidence and spatial context; no manifest or discovered instruction is read.
//!
//! A whole-drive search leaves out what belongs to Windows, installed
//! programs and tool homes, and counts folders without access as left out:
//! on a drive they are expected and would otherwise leave every pass partial.
use crate::domain::{DiscoveryEvidence, DiscoveryKind, InventoryItem, ItemCategory};
use crate::fsread::{self, Budget};
use crate::platform::{DirEntryInfo, DirHandle};
use crate::storage::GenerationResult;
use std::ffi::OsStr;
use std::io;
use std::rc::Rc;

const SKIP: &[&str] = &[".git", ".hg", ".svn", "$RECYCLE.BIN", "System Volume Information"];

/// Directly below a drive root: Windows itself, installed programs and what
/// the system keeps for updates and recovery.
const DRIVE_SYSTEM: &[&str] = &[
    "Windows",
    "Windows.old",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "Recovery",
    "PerfLogs",
    "Boot",
    "EFI",
    "Config.Msi",
    "MSOCache",
    "OneDriveTemp",
    "$WinREAgent",
    "$Windows.~BT",
    "$Windows.~WS",
    "$SysReset",
    "$GetCurrent",
];
/// Template and alias profiles directly below `Users`.
const DRIVE_PROFILES: &[&str] = &["Default", "Default User", "All Users"];
/// Game and app libraries anywhere on a drive.
const DRIVE_LIBRARIES: &[&str] = &[
    "SteamLibrary",
    "steamapps",
    "Epic Games",
    "GOG Games",
    "XboxGames",
    "WindowsApps",
    "ModifiableWindowsApps",
    "WpSystem",
];
/// Visited first on a drive, so a pass that runs out of budget has seen them.
const LIKELY_PLACES: &[&str] = &[
    "Users",
    "dev",
    "src",
    "source",
    "repos",
    "code",
    "projects",
    "projekte",
    "git",
    "github",
    "gitlab",
    "work",
    "workspace",
    "workspaces",
    "Desktop",
    "Documents",
    "Dokumente",
];
/// Words for a place to try things out. Matched as whole words of a folder
/// name, so `parser-spike` and `scratch_2024` count, `inspiker` does not.
const SCRATCH_WORDS: &[&str] = &[
    "scratch",
    "scratchpad",
    "scratchpads",
    "experiment",
    "experiments",
    "playground",
    "playgrounds",
    "sandbox",
    "sandboxes",
    "spike",
    "spikes",
    "poc",
    "pocs",
    "prototype",
    "prototypes",
    "tryout",
    "tryouts",
    "throwaway",
    "lab",
    "versuch",
    "versuche",
    "spielwiese",
];
/// Subfolders that show development work without a manifest.
const DEV_FOLDERS: &[&str] = &[".venv", "venv", "node_modules", "__pycache__", ".ipynb_checkpoints"];

struct Pending {
    parent: Rc<DirHandle>,
    name: Option<String>,
    rel: String,
    depth: u32,
    project: Option<String>,
}

/// `drive` marks a search over a whole drive root.
pub(super) fn scan(root: Rc<DirHandle>, budget: &mut Budget, drive: bool) -> GenerationResult {
    let mut result = GenerationResult::default();
    let mut pending = vec![Pending { parent: root, name: None, rel: String::new(), depth: 0, project: None }];
    let mut links = 0_u64;
    let mut placeholders = 0_u64;
    let mut excluded = 0_u64;
    let mut system = 0_u64;
    let mut denied = 0_u64;
    while let Some(next) = pending.pop() {
        if budget.exhausted() {
            break;
        }
        let dir = match next.name {
            Some(name) => match next.parent.open_dir(OsStr::new(&name)) {
                Ok(child) => Rc::new(child),
                Err(e) if drive && out_of_reach(&e) => {
                    denied += 1;
                    continue;
                }
                Err(e) => {
                    report(&mut result, &next.rel, e);
                    continue;
                }
            },
            None => next.parent,
        };
        let entries = match fsread::list(&dir, budget) {
            Ok((entries, _)) => entries,
            Err(e) if drive && out_of_reach(&e) => {
                denied += 1;
                continue;
            }
            Err(e) => {
                report(&mut result, &next.rel, e);
                continue;
            }
        };
        if drive && next.depth > 0 && next.project.is_none() && program_folder(&entries) {
            system += 1;
            continue;
        }
        let inspection = inspect_directory(&next.rel, &entries, next.project.as_deref(), drive);
        result.items.extend(inspection.items);
        let children = pending.len();
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
            if drive && inspection.project.is_none() && drive_excluded(&next.rel, name) {
                system += 1;
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
        // The stack takes the last entry first: likely places go on top.
        if drive {
            pending[children..].sort_by_key(|p| p.name.as_deref().is_some_and(likely_place));
        }
    }
    result.notes.push(format!(
        "Nur Namen/Metadaten; keine Nutzung oder Entbehrlichkeit belegt. {links} Links, {placeholders} Platzhalter und {excluded} Unterordner ausgelassen."
    ));
    if drive {
        result.notes.push(format!(
            "Laufwerkssuche: {system} System-, Programm- und Werkzeugordner sowie {denied} Ordner ohne Zugriffsrecht ausgelassen."
        ));
    }
    result
}

/// Other users' profiles and system folders deny access; a folder may also
/// vanish while a drive is walked. Neither is a reading error of the search.
fn out_of_reach(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::NotFound)
}

/// Program files next to their libraries, without any project marker: an
/// installation (Python, a game, a portable tool), not a user project.
fn program_folder(entries: &[DirEntryInfo]) -> bool {
    let files = || entries.iter().filter(|e| !e.is_dir() && !e.is_reparse()).filter_map(DirEntryInfo::name_str);
    let has = |ext: &str| files().any(|n| n.to_ascii_lowercase().ends_with(ext));
    has(".exe") && has(".dll") && !entries.iter().any(|e| project_marker(e).is_some())
}

/// What a drive search leaves out, by its place below the drive root.
fn drive_excluded(parent: &str, name: &str) -> bool {
    let named = |list: &[&str]| list.iter().any(|s| s.eq_ignore_ascii_case(name));
    let parts: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
    let users = parts.first().is_some_and(|p| p.eq_ignore_ascii_case("Users"));
    let by_place = match parts.as_slice() {
        [] => named(DRIVE_SYSTEM),
        [_] if users => named(DRIVE_PROFILES),
        // Inside a profile: application data, Scoop and the dot folders of
        // tools (.cargo, .rustup, .claude …). Stores and personal AI folders
        // have their own views.
        [_, _] if users => name.starts_with('.') || named(&["AppData", "scoop"]),
        [_, _, go] if users && go.eq_ignore_ascii_case("go") => name.eq_ignore_ascii_case("pkg"),
        _ => false,
    };
    by_place || named(DRIVE_LIBRARIES)
}

fn likely_place(name: &str) -> bool {
    LIKELY_PLACES.iter().any(|p| p.eq_ignore_ascii_case(name))
}

/// A folder name with a word for trying things out.
fn scratch_name(base: &str) -> bool {
    base.split(|c: char| !c.is_ascii_alphabetic()).any(|word| SCRATCH_WORDS.contains(&word))
}

/// Shared with the ordinary project scanner, without a second traversal or
/// content read. Adapter-claimed stores remain bounded leaves in discovery.
pub(super) struct DirectoryInspection {
    pub items: Vec<InventoryItem>,
    pub project: Option<String>,
    pub leaf: bool,
}

/// `drive` lists no single loose scripts: on a whole drive they are mostly
/// tool and installer files. Two or more in one folder still report that
/// folder as a possible scratchpad.
pub(super) fn inspect_directory(
    rel: &str,
    entries: &[DirEntryInfo],
    inherited_project: Option<&str>,
    drive: bool,
) -> DirectoryInspection {
    let safe = entries.iter().filter(|e| !e.is_reparse() && !e.is_placeholder());
    let markers = safe.clone().filter_map(project_marker).take(12).collect::<Vec<_>>();
    let mut project = if markers.is_empty() { inherited_project.map(str::to_string) } else { Some(rel.to_string()) };
    let mut items = Vec::new();
    let base = rel.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    let has_file =
        |name: &str| safe.clone().any(|e| !e.is_dir() && e.name_str().is_some_and(|n| n.eq_ignore_ascii_case(name)));
    let files = |test: fn(&str) -> bool| safe.clone().filter(|e| !e.is_dir() && e.name_str().is_some_and(test)).count();
    let sources = files(is_source);
    let notebooks = files(|n| n.to_ascii_lowercase().ends_with(".ipynb"));
    let dev_folder = safe
        .clone()
        .any(|e| e.is_dir() && e.name_str().is_some_and(|n| DEV_FOLDERS.iter().any(|d| d.eq_ignore_ascii_case(n))));
    let has_dev_context = project.is_some() || sources > 0 || dev_folder;
    let environment = has_file("pyvenv.cfg");
    let directory_kind: Option<(DiscoveryKind, String, bool)> = if environment {
        Some((DiscoveryKind::Environment, "Umgebungsmarker pyvenv.cfg".into(), true))
    } else if base == "node_modules" {
        Some((DiscoveryKind::Dependencies, "Ordnername node_modules; Installationszustand nicht geprüft".into(), true))
    } else if matches!(base.as_str(), ".npm" | "_cacache" | ".pnpm-store" | ".uv-cache") {
        Some((DiscoveryKind::Cache, "Werkzeugtypischer Cachename; gemeinsame Nutzung unbekannt".into(), true))
    } else if has_dev_context {
        directory_hint(&base).map(|(kind, evidence, leaf)| (kind, evidence.to_string(), leaf)).or_else(|| {
            scratch_name(&base).then(|| {
                (
                    DiscoveryKind::Scratchpad,
                    "Experimenttypischer Name mit Entwicklungshinweisen; Zweck unbekannt".into(),
                    false,
                )
            })
        })
    } else {
        None
    };
    // Loose scripts or a notebook outside every project: a working folder.
    let directory_kind = directory_kind.or_else(|| {
        (markers.is_empty() && inherited_project.is_none() && !rel.is_empty() && (sources >= 2 || notebooks > 0)).then(
            || {
                (
                    DiscoveryKind::Scratchpad,
                    format!("{sources} lose Skripte oder Notebooks ohne Projektmarker; Zweck unbekannt"),
                    false,
                )
            },
        )
    });
    if directory_kind.as_ref().is_some_and(|(kind, ..)| *kind != DiscoveryKind::Scratchpad) {
        project = inherited_project.map(str::to_string);
    }
    // Recognize environments, stores and outputs before checking manifest
    // markers: a dependency's package.json is not a new user project.
    let leaf = directory_kind.as_ref().is_some_and(|(_, _, leaf)| *leaf);
    if let Some((kind, evidence, _)) = directory_kind {
        items.push(finding(kind, rel, true, None, None, project.as_deref(), &evidence));
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
    if !leaf {
        for entry in safe.filter(|e| !e.is_dir()) {
            let Some(name) = entry.name_str() else { continue };
            let lower = name.to_ascii_lowercase();
            let kind = if lower.ends_with(".ipynb") {
                Some((DiscoveryKind::Notebook, "Dateiendung .ipynb; Inhalt und Zweck unbekannt"))
            } else if project.is_none() && !drive && is_source(name) {
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::limits::ScanLimits;
    use std::path::Path;

    fn put(root: &Path, rel: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "untrusted: never read").unwrap();
    }

    fn search(root: &Path, drive: bool) -> GenerationResult {
        let handle = Rc::new(DirHandle::open_root(root, false).unwrap());
        scan(handle, &mut Budget::new(ScanLimits::drive_search()), drive)
    }

    fn found(result: &GenerationResult) -> Vec<(String, DiscoveryKind)> {
        result.items.iter().map(|i| (i.rel_path.clone(), i.evidence.discovery.as_ref().unwrap().kind)).collect()
    }

    /// The temporary folder stands in for a drive root; the walk is the same.
    fn drive_layout(root: &Path) {
        for rel in [
            "Windows/System32/drivers/package.json",
            "Program Files/Tool/package.json",
            "ProgramData/vendor/Cargo.toml",
            "Users/Default/AppData/template/package.json",
            "Users/alice/AppData/Local/app/package.json",
            "Users/alice/.cargo/registry/src/crate/Cargo.toml",
            "Users/alice/go/pkg/mod/example.com/m/go.mod",
            "Users/alice/go/src/tool/go.mod",
            "Users/alice/source/repos/api/Cargo.toml",
            "Users/alice/Documents/parser-spike/main.py",
            "Users/alice/Downloads/tools/a.ps1",
            "Users/alice/Downloads/tools/b.ps1",
            "Users/alice/Downloads/setup.bat",
            "Games/Puzzle/puzzle.exe",
            "Games/Puzzle/engine.dll",
            "Games/Puzzle/mods/a.lua",
            "Games/Puzzle/mods/b.lua",
            "SteamLibrary/steamapps/common/x/package.json",
        ] {
            put(root, rel);
        }
    }

    #[test]
    fn a_drive_search_finds_projects_and_scratchpads_but_leaves_out_system_and_tools() {
        let temp = tempfile::tempdir().unwrap();
        drive_layout(temp.path());
        let result = search(temp.path(), true);
        assert!(result.errors.is_empty() && result.limit_reason.is_none(), "{:?}", result.errors);
        assert_eq!(result.files_read, 0);
        let found = found(&result);
        for (rel, kind) in [
            ("Users/alice/source/repos/api", DiscoveryKind::Project),
            ("Users/alice/go/src/tool", DiscoveryKind::Project),
            ("Users/alice/Documents/parser-spike", DiscoveryKind::Scratchpad),
            ("Users/alice/Downloads/tools", DiscoveryKind::Scratchpad),
        ] {
            assert!(found.contains(&(rel.to_string(), kind)), "{rel}: {found:?}");
        }
        for left_out in
            ["Windows", "Program Files", "ProgramData", "Default", "AppData", ".cargo", "go/pkg", "Games", "Steam"]
        {
            assert!(found.iter().all(|(rel, _)| !rel.contains(left_out)), "{left_out}: {found:?}");
        }
        assert!(found.iter().all(|(_, kind)| *kind != DiscoveryKind::Script), "no single loose scripts: {found:?}");
        assert!(result.notes.iter().any(|n| n.starts_with("Laufwerkssuche:")));
    }

    #[test]
    fn a_search_folder_keeps_every_place_and_its_loose_scripts() {
        let temp = tempfile::tempdir().unwrap();
        drive_layout(temp.path());
        let found = found(&search(temp.path(), false));
        assert!(found.contains(&("Program Files/Tool".into(), DiscoveryKind::Project)));
        assert!(found.contains(&("Users/alice/AppData/Local/app".into(), DiscoveryKind::Project)));
        assert!(found.contains(&("Users/alice/Downloads/setup.bat".into(), DiscoveryKind::Script)));
        assert!(found.contains(&("Users/alice/Downloads/tools/a.ps1".into(), DiscoveryKind::Script)));
    }

    #[test]
    fn scratch_words_match_whole_words_only() {
        for name in ["scratch", "parser-spike", "scratch_2024", "rust-playground", "poc-auth", "versuche", "ml.lab"] {
            assert!(scratch_name(name), "{name}");
        }
        for name in ["inspiker", "spikeball", "pocket", "laboratory", "api", ""] {
            assert!(!scratch_name(name), "{name}");
        }
    }

    #[test]
    fn drive_exclusions_depend_on_the_place() {
        assert!(drive_excluded("", "Windows") && drive_excluded("", "program files"));
        assert!(!drive_excluded("dev", "Windows"), "only directly below the drive root");
        assert!(drive_excluded("Users", "Default") && !drive_excluded("Users", "alice"));
        assert!(drive_excluded("Users/alice", "AppData") && drive_excluded("Users/alice", ".rustup"));
        assert!(!drive_excluded("Users/alice/dev", ".config"), "dot folders only in the profile itself");
        assert!(drive_excluded("Users/alice/go", "pkg") && !drive_excluded("Users/alice/go", "src"));
        assert!(drive_excluded("D/Games", "SteamLibrary"));
    }
}
