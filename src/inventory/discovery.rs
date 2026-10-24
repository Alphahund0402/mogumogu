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
    "scratches",
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
/// Data files that make a scratch-named folder a working folder.
const DATA_EXTENSIONS: &[&str] = &[
    ".csv", ".tsv", ".parquet", ".feather", ".arrow", ".jsonl", ".ndjson", ".h5", ".hdf5", ".npy", ".npz", ".pkl",
    ".pickle",
];
/// Default names of project templates: `ConsoleApp3`, `pythonProject`, `untitled1`.
const TEMPLATE_PREFIXES: &[&str] = &[
    "consoleapp",
    "consoleapplication",
    "windowsformsapp",
    "winformsapp",
    "wpfapp",
    "webapplication",
    "classlibrary",
    "blazorapp",
    "mauiapp",
    "pythonproject",
    "untitled",
    "myapplication",
    "newproject",
    "neuesprojekt",
];
/// Generic stems that count only with a number: `test2`, `tmp1`, `project4`.
const NUMBERED_STEMS: &[&str] = &["test", "tmp", "temp", "project", "projekt", "app", "demo", "new", "neu"];
const TEMPLATE_NAMES: &[&str] = &[
    "vite-project",
    "my-app",
    "my-project",
    "my-react-app",
    "my-vue-app",
    "new unity project",
    "my project",
    "new game project",
    "neuer ordner",
    "new folder",
];
const COPY_SUFFIXES: &[&str] = &[
    " - kopie", " - copy", "_old", "-old", ".old", "_alt", "-alt", "_backup", "-backup", ".bak", "_bak", "-bak",
    "_copy", "-copy", "_kopie", "-kopie",
];
const COPY_PREFIXES: &[&str] = &["copy of ", "kopie von ", "kopie - "];
/// First words of file names that are written to be thrown away.
const THROWAWAY_WORDS: &[&str] = &[
    "test",
    "tmp",
    "temp",
    "untitled",
    "scratch",
    "foo",
    "bar",
    "try",
    "versuch",
    "neu",
    "new",
    "spielwiese",
    "playground",
    "sandbox",
    "asdf",
];
/// Manifests that name a project by themselves.
const MANIFESTS: &[&str] = &[
    "cargo.toml",
    "package.json",
    "pyproject.toml",
    "setup.py",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "composer.json",
    "gemfile",
    "mix.exs",
    "pubspec.yaml",
    "deno.json",
    "deno.jsonc",
    "package.swift",
    "build.zig",
    "build.sbt",
    "stack.yaml",
    "meson.build",
    "project.godot",
    "flake.nix",
];
const MANIFEST_EXTENSIONS: &[&str] =
    &[".csproj", ".fsproj", ".vbproj", ".vcxproj", ".sln", ".slnx", ".cabal", ".uproject"];
/// Markers that also occur outside projects.
const WEAK_MARKERS: &[&str] = &[
    "requirements.txt",
    "setup.cfg",
    "pipfile",
    "uv.lock",
    "makefile",
    "cmakelists.txt",
    "packages.config",
    "environment.yml",
    "dockerfile",
    "compose.yaml",
    "docker-compose.yml",
];
const WEAK_EXTENSIONS: &[&str] = &[".code-workspace", ".rproj"];
const VCS: &[&str] = &[".git", ".hg", ".svn"];
const LOCKFILES: &[&str] = &[
    "cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "bun.lockb",
    "poetry.lock",
    "uv.lock",
    "pipfile.lock",
    "composer.lock",
    "gemfile.lock",
    "go.sum",
    "packages.lock.json",
    "pubspec.lock",
    "mix.lock",
    "deno.lock",
];
/// Instructions and settings of AI coding tools inside a project.
const AI_FILES: &[&str] = &["claude.md", "agents.md", "gemini.md", ".cursorrules", ".windsurfrules", ".clinerules"];
const AI_FOLDERS: &[&str] =
    &[".claude", ".cursor", ".codex", ".gemini", ".windsurf", ".roo", ".continue", ".clinerules"];
const CI: &[&str] =
    &[".github", ".gitlab-ci.yml", ".circleci", "azure-pipelines.yml", "jenkinsfile", ".woodpecker.yml"];
const SOURCE_FOLDERS: &[&str] = &["src", "lib", "app", "source", "pkg", "cmd"];
const TEST_FOLDERS: &[&str] = &["tests", "test", "__tests__", "spec", "specs"];
const EDITOR_FOLDERS: &[&str] = &[".vscode", ".idea", ".devcontainer"];
/// Suffixes of folders unpacked from a downloaded source archive.
const ARCHIVE_SUFFIXES: &[&str] = &["-main", "-master", "-develop", "-trunk"];

struct Pending {
    parent: Rc<DirHandle>,
    name: Option<String>,
    rel: String,
    depth: u32,
    project: Option<String>,
}

/// `drive` marks a search over a whole drive root. Below `Users` it only
/// enters `own_profile`, the folder name of the current user's profile:
/// other people's profiles stay out even when access would be granted.
pub(super) fn scan(root: Rc<DirHandle>, budget: &mut Budget, drive: bool, own_profile: &str) -> GenerationResult {
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
            // Also below a project marker at the drive or profile root (a
            // `C:\.git`, a dotfiles repository): Windows and AppData stay out.
            if drive && drive_excluded(&next.rel, name, own_profile) {
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
fn drive_excluded(parent: &str, name: &str, own_profile: &str) -> bool {
    let named = |list: &[&str]| list.iter().any(|s| s.eq_ignore_ascii_case(name));
    let parts: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
    let users = parts.first().is_some_and(|p| p.eq_ignore_ascii_case("Users"));
    let by_place = match parts.as_slice() {
        [] => named(DRIVE_SYSTEM),
        [_] if users => own_profile.is_empty() || !name.eq_ignore_ascii_case(own_profile),
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

/// Why a (lower-case) folder name looks like a quick try, if it does.
fn scratch_reason(base: &str) -> Option<&'static str> {
    if scratch_name(base) {
        return Some("Experimenttypischer Name");
    }
    let plain = without_copy_number(base);
    let digits = |rest: &str| rest.chars().all(|c| c.is_ascii_digit());
    let template = TEMPLATE_NAMES.contains(&plain)
        || TEMPLATE_PREFIXES.iter().any(|p| plain.strip_prefix(p).is_some_and(digits))
        || NUMBERED_STEMS.iter().any(|p| plain.strip_prefix(p).is_some_and(|rest| !rest.is_empty() && digits(rest)));
    if template {
        return Some("Standardname einer Projektvorlage");
    }
    let copy = COPY_SUFFIXES.iter().any(|s| plain.ends_with(s)) || COPY_PREFIXES.iter().any(|p| plain.starts_with(p));
    copy.then_some("Name einer Kopie")
}

/// `Neuer Ordner (2)` and `My project (1)` keep their template name.
fn without_copy_number(base: &str) -> &str {
    match base.strip_suffix(')').and_then(|rest| rest.rsplit_once(" (")) {
        Some((name, number)) if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) => name,
        _ => base,
    }
}

/// A file whose first word says it is meant to be thrown away.
fn throwaway_file(lower: &str) -> bool {
    lower.split(|c: char| !c.is_ascii_alphabetic()).next().is_some_and(|word| THROWAWAY_WORDS.contains(&word))
}

/// Where people drop single files on a drive: the profile itself and its
/// desktop, downloads and documents (also inside OneDrive).
fn filing_place(rel: &str) -> bool {
    let parts: Vec<String> = rel.split('/').map(str::to_ascii_lowercase).collect();
    let filing = |name: &str| matches!(name, "desktop" | "downloads" | "documents" | "dokumente");
    match parts.as_slice() {
        [users, _] => users == "users",
        [users, _, place] => users == "users" && filing(place),
        [users, _, onedrive, place] => users == "users" && onedrive.starts_with("onedrive") && filing(place),
        _ => false,
    }
}

/// What the names in a project folder say about it. This is filename
/// evidence only; no manifest is read.
struct Assessment {
    confidence: u8,
    signals: Vec<String>,
    notes: Vec<String>,
}

fn assess_project(rel: &str, base: &str, entries: &[&DirEntryInfo], inherited: Option<&str>) -> Assessment {
    let names: Vec<(&str, String, bool)> =
        entries.iter().filter_map(|e| e.name_str().map(|n| (n, n.to_ascii_lowercase(), e.is_dir()))).collect();
    let file = |list: &[&str]| names.iter().any(|(_, n, dir)| !dir && list.contains(&n.as_str()));
    let folder = |list: &[&str]| names.iter().any(|(_, n, dir)| *dir && list.contains(&n.as_str()));
    let any = |list: &[&str]| names.iter().any(|(_, n, _)| list.contains(&n.as_str()));
    let vcs = any(VCS);
    let ci = any(CI);
    let manifest =
        file(MANIFESTS) || names.iter().any(|(_, n, dir)| !dir && MANIFEST_EXTENSIONS.iter().any(|e| n.ends_with(e)));
    let lockfile = file(LOCKFILES);
    let sources = folder(SOURCE_FOLDERS) || names.iter().filter(|(_, n, dir)| !dir && is_source(n)).count() >= 3;
    let mut signals = Vec::new();
    for (present, label) in [
        (vcs, "Versionsverwaltung"),
        (lockfile, "Lockfile"),
        (sources, "Quellcode"),
        (folder(TEST_FOLDERS), "Tests"),
        (ci, "CI"),
        (folder(EDITOR_FOLDERS), "Editor-Einstellungen"),
    ] {
        if present {
            signals.push(label.to_string());
        }
    }
    let ai: Vec<&str> = names
        .iter()
        .filter(|(_, n, dir)| if *dir { AI_FOLDERS.contains(&n.as_str()) } else { AI_FILES.contains(&n.as_str()) })
        .map(|(original, ..)| *original)
        .take(4)
        .collect();
    if !ai.is_empty() {
        signals.push(format!("KI-Dateien ({})", ai.join(", ")));
    }
    let mut confidence = match (vcs, manifest, lockfile || sources) {
        (true, true, _) | (false, true, true) => 3,
        // A repository with its own CI, as many CMake and Make projects are.
        (true, false, _) if ci => 3,
        (true, false, _) | (false, true, false) => 2,
        _ => 1,
    };
    let mut notes = Vec::new();
    if let Some(parent) = inherited {
        // The outer folder's name: in free text a whole path can read as a
        // token and be redacted. The full path stays in the item's context.
        let outer = parent.rsplit('/').next().filter(|name| !name.is_empty());
        notes.push(format!("Teilprojekt von {}", outer.unwrap_or("der Suchwurzel")));
    }
    if !vcs && (ARCHIVE_SUFFIXES.iter().any(|s| base.ends_with(s)) || versioned_name(base)) {
        notes.push("Name eines heruntergeladenen Archivs, ohne Versionsverwaltung".into());
        confidence = 1;
    }
    if rel.split('/').any(|part| part.eq_ignore_ascii_case("downloads")) {
        notes.push("liegt unter Downloads".into());
        confidence = confidence.min(2);
    }
    Assessment { confidence, signals, notes }
}

/// `tool-1.4.2` or `lib-v2.0`: the folder name of an unpacked release.
fn versioned_name(base: &str) -> bool {
    base.rsplit_once('-').is_some_and(|(_, version)| {
        let version = version.strip_prefix('v').unwrap_or(version);
        version.contains('.')
            && version.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    })
}

fn confidence_label(confidence: u8) -> &'static str {
    match confidence {
        3 => "hoch",
        2 => "mittel",
        _ => "gering",
    }
}

/// The newest change below the folder's own entries, by day. A day keeps a
/// busy project from showing as changed after every save.
fn newest_day(entries: &[&DirEntryInfo]) -> Option<i64> {
    entries.iter().map(|e| e.modified).filter(|m| *m > 0).max().map(|m| m - m.rem_euclid(86_400))
}

/// Shared with the ordinary project scanner, without a second traversal or
/// content read. Adapter-claimed stores remain bounded leaves in discovery.
pub(super) struct DirectoryInspection {
    pub items: Vec<InventoryItem>,
    pub project: Option<String>,
    pub leaf: bool,
}

/// `drive` lists single loose scripts only where people drop files (desktop,
/// downloads, documents) or under a throwaway name: elsewhere on a drive they
/// are mostly tool and installer files. Two or more in one folder still
/// report that folder as a possible scratchpad.
pub(super) fn inspect_directory(
    rel: &str,
    entries: &[DirEntryInfo],
    inherited_project: Option<&str>,
    drive: bool,
) -> DirectoryInspection {
    let safe: Vec<&DirEntryInfo> = entries.iter().filter(|e| !e.is_reparse() && !e.is_placeholder()).collect();
    let markers = safe.iter().filter_map(|e| project_marker(e)).take(12).collect::<Vec<_>>();
    let mut project = if markers.is_empty() { inherited_project.map(str::to_string) } else { Some(rel.to_string()) };
    let mut items = Vec::new();
    let base = rel.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    let has_file =
        |name: &str| safe.iter().any(|e| !e.is_dir() && e.name_str().is_some_and(|n| n.eq_ignore_ascii_case(name)));
    let files =
        |test: &dyn Fn(&str) -> bool| safe.iter().filter(|e| !e.is_dir() && e.name_str().is_some_and(test)).count();
    let sources = files(&is_source);
    let notebooks = files(&|n: &str| n.to_ascii_lowercase().ends_with(".ipynb"));
    let data = files(&|n: &str| {
        let lower = n.to_ascii_lowercase();
        DATA_EXTENSIONS.iter().any(|e| lower.ends_with(e))
    });
    let dev_folder = safe
        .iter()
        .any(|e| e.is_dir() && e.name_str().is_some_and(|n| DEV_FOLDERS.iter().any(|d| d.eq_ignore_ascii_case(n))));
    let has_dev_context = project.is_some() || sources > 0 || dev_folder;
    let environment = has_file("pyvenv.cfg");
    let directory_kind: Option<(DiscoveryKind, String, bool)> = if environment {
        Some((DiscoveryKind::Environment, "Umgebungsmarker pyvenv.cfg".into(), true))
    } else if base == "node_modules" {
        Some((DiscoveryKind::Dependencies, "Ordnername node_modules; Installationszustand nicht geprüft".into(), true))
    } else if matches!(base.as_str(), ".npm" | "_cacache" | ".pnpm-store" | ".uv-cache") {
        Some((DiscoveryKind::Cache, "Werkzeugtypischer Cachename; gemeinsame Nutzung unbekannt".into(), true))
    } else if let Some(hint) = directory_hint(&base).filter(|_| has_dev_context) {
        Some((hint.0, hint.1.to_string(), hint.2))
    } else if has_file("CACHEDIR.TAG") {
        // uv, Hugging Face and others mark their caches this way, whatever
        // the folder is called; the packages inside are no user projects.
        Some((DiscoveryKind::Cache, "Cachemarker CACHEDIR.TAG; gemeinsame Nutzung unbekannt".into(), true))
    } else if let Some(reason) = scratch_reason(&base).filter(|_| has_dev_context || data > 0) {
        let context = if has_dev_context { "Entwicklungshinweisen" } else { "Datendateien" };
        Some((DiscoveryKind::Scratchpad, format!("{reason} mit {context}; Zweck unbekannt"), false))
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
        let modified = (kind == DiscoveryKind::Scratchpad).then(|| newest_day(&safe)).flatten();
        items.push(finding(kind, rel, true, None, modified, project.as_deref(), &evidence));
    } else if !markers.is_empty() {
        let assessment = assess_project(rel, &base, &safe, inherited_project);
        let mut detail = format!("Projektmarker: {}", markers.join(", "));
        if !assessment.signals.is_empty() {
            detail.push_str(&format!(" · Hinweise: {}", assessment.signals.join(", ")));
        }
        for note in &assessment.notes {
            detail.push_str(&format!(" · {note}"));
        }
        detail.push_str(&format!(
            " · Sicherheit: {}. Nur Namen; Inhalt und Nutzung unbekannt.",
            confidence_label(assessment.confidence)
        ));
        // A nested project keeps the outer one as its context.
        let context = inherited_project.unwrap_or(rel);
        let mut item = finding(DiscoveryKind::Project, rel, true, None, newest_day(&safe), Some(context), &detail);
        if let Some(evidence) = item.evidence.discovery.as_mut() {
            evidence.confidence = assessment.confidence;
            evidence.signals = assessment.signals;
        }
        items.push(item);
    }
    if !leaf {
        let scripts_here = !drive || filing_place(rel);
        for entry in safe.iter().filter(|e| !e.is_dir()) {
            let Some(name) = entry.name_str() else { continue };
            let lower = name.to_ascii_lowercase();
            let kind = if lower.ends_with(".ipynb") {
                Some((DiscoveryKind::Notebook, "Dateiendung .ipynb; Inhalt und Zweck unbekannt"))
            } else if project.is_none() && is_source(name) && scripts_here {
                Some((DiscoveryKind::Script, "Quellcode-/Skriptendung ohne gefundenen Projektmarker"))
            } else if project.is_none() && is_source(name) && throwaway_file(&lower) {
                Some((DiscoveryKind::Script, "Wegwerftypischer Dateiname ohne Projektmarker"))
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
        VCS.contains(&lower.as_str())
    } else {
        lower == ".git"
            || MANIFESTS.contains(&lower.as_str())
            || WEAK_MARKERS.contains(&lower.as_str())
            || MANIFEST_EXTENSIONS.iter().chain(WEAK_EXTENSIONS).any(|e| lower.ends_with(e))
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
        "vendor" | "third_party" | "third-party" | "bower_components" | "jspm_packages" | "pods" => Some((
            DiscoveryKind::Dependencies,
            "Typischer Ordner für mitgelieferten Fremdcode; Herkunft nicht geprüft",
            true,
        )),
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
        path_exact: crate::privacy::path(rel) == rel,
        confidence: 0,
        signals: Vec::new(),
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
        scan(handle, &mut Budget::new(ScanLimits::drive_search()), drive, "alice")
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
            "Tools/helper.ps1",
            "Tools/try_parser.py",
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
        let scripts: Vec<&str> =
            found.iter().filter(|(_, kind)| *kind == DiscoveryKind::Script).map(|(rel, _)| rel.as_str()).collect();
        assert_eq!(
            scripts.iter().copied().collect::<std::collections::BTreeSet<_>>(),
            ["Tools/try_parser.py", "Users/alice/Downloads/setup.bat"].into_iter().collect(),
            "single scripts only in filing places or with a throwaway name"
        );
        assert!(result.notes.iter().any(|n| n.starts_with("Laufwerkssuche:")));
    }

    #[test]
    fn markers_at_the_drive_or_profile_root_switch_no_exclusion_off() {
        let temp = tempfile::tempdir().unwrap();
        drive_layout(temp.path());
        put(temp.path(), ".git/HEAD");
        put(temp.path(), "Makefile");
        put(temp.path(), "Users/alice/package.json");
        put(temp.path(), "Users/bob/source/secret-project/Cargo.toml");
        let found = found(&search(temp.path(), true));
        for left_out in ["Windows", "Program Files", "ProgramData", "Default", "AppData", ".cargo", "bob", "Steam"] {
            assert!(found.iter().all(|(rel, _)| !rel.contains(left_out)), "{left_out}: {found:?}");
        }
        assert!(found.iter().any(|(rel, _)| rel == "Users/alice/source/repos/api"), "{found:?}");
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
        let excluded = |parent: &str, name: &str| drive_excluded(parent, name, "alice");
        assert!(excluded("", "Windows") && excluded("", "program files"));
        assert!(!excluded("dev", "Windows"), "only directly below the drive root");
        assert!(excluded("Users", "Default") && !excluded("Users", "alice"));
        assert!(excluded("Users", "bob") && excluded("Users", "Public"), "other people's profiles stay out");
        assert!(drive_excluded("Users", "alice", ""), "without a known profile no profile is entered");
        assert!(excluded("Users/alice", "AppData") && excluded("Users/alice", ".rustup"));
        assert!(!excluded("Users/alice/dev", ".config"), "dot folders only in the profile itself");
        assert!(excluded("Users/alice/go", "pkg") && !excluded("Users/alice/go", "src"));
        assert!(excluded("D/Games", "SteamLibrary"));
    }

    #[test]
    fn template_and_copy_names_mark_quick_tries() {
        for name in [
            "consoleapp3",
            "pythonproject",
            "untitled1",
            "my-app",
            "neuer ordner (2)",
            "my project (1)",
            "test2",
            "projekt-alt",
            "api - kopie",
            "copy of parser",
            "parser.bak",
        ] {
            assert!(scratch_reason(name).is_some(), "{name}");
        }
        for name in ["test", "project", "api", "tests", "application", "my-api", "parser"] {
            assert!(scratch_reason(name).is_none(), "{name}");
        }
        assert_eq!(scratch_reason("rust-playground"), Some("Experimenttypischer Name"));
        assert_eq!(scratch_reason("webapplication12"), Some("Standardname einer Projektvorlage"));
        assert_eq!(scratch_reason("api_old"), Some("Name einer Kopie"));
    }

    #[test]
    fn filing_places_and_throwaway_files() {
        for rel in
            ["Users/alice", "Users/alice/Desktop", "users/alice/downloads", "Users/alice/OneDrive - Firma/Documents"]
        {
            assert!(filing_place(rel), "{rel}");
        }
        for rel in ["", "Users", "Users/alice/Desktop/project", "dev/Downloads"] {
            assert!(!filing_place(rel), "{rel}");
        }
        assert!(throwaway_file("test_parser.py") && throwaway_file("untitled-1.ps1") && throwaway_file("tmp.js"));
        assert!(!throwaway_file("parser.py") && !throwaway_file("latest.py") && !throwaway_file("contest.py"));
    }

    #[test]
    fn project_confidence_follows_the_names() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "strong/Cargo.toml",
            "strong/Cargo.lock",
            "strong/.git/HEAD",
            "strong/CLAUDE.md",
            "strong/.github/ci.yml",
            "strong/tests/a.rs",
            "plain/package.json",
            "weak/Makefile",
            "cmake/CMakeLists.txt",
            "cmake/.git/HEAD",
            "cmake/.github/ci.yml",
            "cmake-local/CMakeLists.txt",
            "cmake-local/.git/HEAD",
            "parser-main/package.json",
            "parser-main/src/a.js",
            "lib-1.4.2/setup.py",
            "Downloads/tool/pyproject.toml",
            "Downloads/tool/uv.lock",
            "mono/package.json",
            "mono/packages/web/package.json",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        let project = |rel: &str| {
            let item = result.items.iter().find(|i| i.rel_path == rel).unwrap_or_else(|| panic!("{rel}"));
            let evidence = item.evidence.discovery.as_ref().unwrap();
            assert_eq!(evidence.kind, DiscoveryKind::Project, "{rel}");
            (evidence.confidence, evidence.signals.clone(), item.detail.clone(), item.evidence.context.clone())
        };
        let (confidence, signals, detail, _) = project("strong");
        assert_eq!(confidence, 3);
        for signal in ["Versionsverwaltung", "Lockfile", "Tests", "CI"] {
            assert!(signals.iter().any(|s| s == signal), "{signal}: {signals:?}");
        }
        assert!(signals.iter().any(|s| s.starts_with("KI-Dateien") && s.contains("CLAUDE.md")), "{signals:?}");
        assert!(detail.contains("Sicherheit: hoch"), "{detail}");
        assert_eq!(project("plain").0, 2);
        assert_eq!(project("weak").0, 1);
        assert_eq!(project("cmake").0, 3, "a repository with its own CI");
        assert_eq!(project("cmake-local").0, 2);
        let (confidence, _, detail, _) = project("parser-main");
        assert!(confidence == 1 && detail.contains("heruntergeladenen Archivs"), "{detail}");
        assert_eq!(project("lib-1.4.2").0, 1);
        let (confidence, _, detail, _) = project("Downloads/tool");
        assert!(confidence == 2 && detail.contains("Downloads"), "{detail}");
        let (_, _, detail, context) = project("mono/packages/web");
        assert!(detail.contains("Teilprojekt von mono"), "{detail}");
        assert_eq!(context, "mono", "a nested project keeps the outer one as context");
        assert_eq!(project("mono").3, "mono");
        let modified = result
            .items
            .iter()
            .find(|i| i.rel_path == "plain")
            .unwrap()
            .evidence
            .discovery
            .as_ref()
            .unwrap()
            .modified_at;
        assert!(modified.is_some_and(|day| day % 86_400 == 0), "activity by day: {modified:?}");
    }

    #[test]
    fn scratch_names_also_count_with_data_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in
            ["sandbox/measurements.csv", "ConsoleApp3/ConsoleApp3.csproj", "notes/readme.txt", "playground/notes.txt"]
        {
            put(root, rel);
        }
        let found = found(&search(root, false));
        assert!(found.contains(&("sandbox".into(), DiscoveryKind::Scratchpad)), "{found:?}");
        assert!(found.contains(&("ConsoleApp3".into(), DiscoveryKind::Scratchpad)), "{found:?}");
        assert!(
            found.iter().all(|(rel, _)| rel != "playground" && rel != "notes"),
            "names alone are not enough: {found:?}"
        );
    }

    #[test]
    fn vendored_code_is_a_leaf_inside_a_project() {
        let temp = tempfile::tempdir().unwrap();
        put(temp.path(), "app/go.mod");
        put(temp.path(), "app/vendor/github.com/lib/go.mod");
        let found = found(&search(temp.path(), false));
        assert!(found.contains(&("app/vendor".into(), DiscoveryKind::Dependencies)), "{found:?}");
        assert!(found.iter().all(|(rel, _)| !rel.starts_with("app/vendor/")), "{found:?}");
    }

    #[test]
    fn a_tagged_cache_is_a_leaf_whatever_its_name() {
        let temp = tempfile::tempdir().unwrap();
        put(temp.path(), "builds/tool-uv-cache/CACHEDIR.TAG");
        put(temp.path(), "builds/tool-uv-cache/archive-v0/Xq1/sklearn/meson.build");
        put(temp.path(), "app/Cargo.toml");
        put(temp.path(), "app/target/CACHEDIR.TAG");
        let found = found(&search(temp.path(), true));
        assert!(found.contains(&("builds/tool-uv-cache".into(), DiscoveryKind::Cache)), "{found:?}");
        assert!(found.iter().all(|(rel, _)| !rel.starts_with("builds/tool-uv-cache/")), "{found:?}");
        assert!(found.contains(&("app/target".into(), DiscoveryKind::BuildOutput)), "named outputs keep their kind");
    }
}
