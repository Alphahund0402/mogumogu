//! Filename-only discovery within an approved root. Findings carry their
//! evidence and spatial context; no manifest or discovered instruction is read.
//!
//! A whole-drive search leaves out what belongs to Windows, installed
//! programs and tool homes, and counts folders without access as left out:
//! on a drive they are expected and would otherwise leave every pass partial.
//!
//! Tests are recognized by names too: test folders, test files and the
//! configuration of test tools. A walker records every listing in a
//! [`Census`] and finishes it once, which writes the counts into the project
//! and test folder findings. Names say nothing about whether tests run or pass.
use crate::domain::{DiscoveryEvidence, DiscoveryKind, InventoryItem, ItemCategory};
use crate::fsread::{self, Budget};
use crate::limits::LABEL_MAX_CHARS;
use crate::platform::{DirEntryInfo, DirHandle};
use crate::privacy;
use crate::storage::GenerationResult;
use std::collections::HashMap;
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
    "pixi.toml",
    "go.mod",
    "go.work",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "build.sbt",
    "deps.edn",
    "project.clj",
    "composer.json",
    "gemfile",
    "mix.exs",
    "rebar.config",
    "gleam.toml",
    "pubspec.yaml",
    "deno.json",
    "deno.jsonc",
    "package.swift",
    "podfile",
    "cartfile",
    "build.zig",
    "build.zig.zon",
    "stack.yaml",
    "cabal.project",
    "project.toml",
    "juliaproject.toml",
    "shard.yml",
    "elm.json",
    "dub.json",
    "dub.sdl",
    "dune-project",
    "cpanfile",
    "makefile.pl",
    "vcpkg.json",
    "conanfile.txt",
    "conanfile.py",
    "meson.build",
    "module.bazel",
    "workspace",
    "workspace.bazel",
    "pants.toml",
    "rush.json",
    "platformio.ini",
    "project.godot",
    "flake.nix",
];
const MANIFEST_EXTENSIONS: &[&str] = &[
    ".csproj",
    ".fsproj",
    ".vbproj",
    ".vcxproj",
    ".sln",
    ".slnx",
    ".sqlproj",
    ".pyproj",
    ".njsproj",
    ".esproj",
    ".cabal",
    ".uproject",
    ".gemspec",
    ".podspec",
    ".nimble",
    ".opam",
    ".rockspec",
    ".dproj",
    ".lpi",
];
/// Markers that also occur outside projects, or name a tool more than a project.
const WEAK_MARKERS: &[&str] = &[
    "requirements.txt",
    "setup.cfg",
    "pipfile",
    "uv.lock",
    "tox.ini",
    "noxfile.py",
    "manage.py",
    "environment.yml",
    "environment.yaml",
    "makefile",
    "justfile",
    "taskfile.yml",
    "taskfile.yaml",
    "cmakelists.txt",
    "cmakepresets.json",
    "configure.ac",
    "xmake.lua",
    "premake5.lua",
    "packages.config",
    "directory.build.props",
    "global.json",
    "tsconfig.json",
    "lerna.json",
    "nx.json",
    "turbo.json",
    "melos.yaml",
    ".tool-versions",
    "mise.toml",
    ".mise.toml",
    "devbox.json",
    "dockerfile",
    "compose.yaml",
    "compose.yml",
    "docker-compose.yml",
    "docker-compose.yaml",
    "chart.yaml",
    "pulumi.yaml",
    "wrangler.toml",
    "wrangler.json",
    "wrangler.jsonc",
    "mkdocs.yml",
];
const WEAK_EXTENSIONS: &[&str] = &[".code-workspace", ".rproj", ".tf"];
/// Folders that count like a manifest; nothing inside them is a project.
const XCODE_BUNDLES: &[&str] = &[".xcodeproj", ".xcworkspace"];
/// Configs of one part of a project (TypeScript `extends`, MSBuild directory
/// imports): inside a project they make no sub-project.
const PART_CONFIGS: &[&str] = &["tsconfig.json", "directory.build.props"];
/// Build files a project's test folder carries for its part of the build:
/// with them, as with weak markers only, it stays a test folder.
const TEST_BUILD_FILES: &[&str] = &["cmakelists.txt", "makefile", "meson.build"];
/// Tool version files that pin a user's global versions in the home folder.
const HOME_CONFIGS: &[&str] = &[".tool-versions", "mise.toml", ".mise.toml"];
const VCS: &[&str] = &[".git", ".hg", ".svn"];
const LOCKFILES: &[&str] = &[
    "cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "bun.lockb",
    "deno.lock",
    "poetry.lock",
    "uv.lock",
    "pdm.lock",
    "pipfile.lock",
    "pixi.lock",
    "conda-lock.yml",
    "composer.lock",
    "gemfile.lock",
    "go.sum",
    "go.work.sum",
    "packages.lock.json",
    "paket.lock",
    "gradle.lockfile",
    "pubspec.lock",
    "mix.lock",
    "rebar.lock",
    "package.resolved",
    "podfile.lock",
    "cartfile.resolved",
    "stack.yaml.lock",
    "cabal.project.freeze",
    "manifest.toml",
    "juliamanifest.toml",
    "renv.lock",
    "shard.lock",
    "cpanfile.snapshot",
    "dub.selections.json",
    "conan.lock",
    "module.bazel.lock",
    "flake.lock",
    "chart.lock",
    ".terraform.lock.hcl",
];
/// File names that only one package manager writes or reads, in display
/// order. One `*` stands for any run of characters. A bare `package.json`
/// names no manager.
const MANAGERS: &[(&str, &str)] = &[
    ("pnpm-lock.yaml", "pnpm"),
    ("pnpm-workspace.yaml", "pnpm"),
    ("yarn.lock", "Yarn"),
    (".yarnrc.yml", "Yarn"),
    ("bun.lock", "Bun"),
    ("bun.lockb", "Bun"),
    ("bunfig.toml", "Bun"),
    ("package-lock.json", "npm"),
    ("npm-shrinkwrap.json", "npm"),
    ("deno.json", "Deno"),
    ("deno.jsonc", "Deno"),
    ("uv.lock", "uv"),
    ("poetry.lock", "Poetry"),
    ("pdm.lock", "PDM"),
    ("pipfile", "Pipenv"),
    ("pipfile.lock", "Pipenv"),
    ("pixi.toml", "pixi"),
    ("pixi.lock", "pixi"),
    ("environment.yml", "conda"),
    ("environment.yaml", "conda"),
    ("conda-lock.yml", "conda"),
    ("requirements*.txt", "pip"),
    ("cargo.toml", "Cargo"),
    ("go.mod", "Go-Module"),
    ("composer.json", "Composer"),
    ("gemfile", "Bundler"),
    ("pubspec.yaml", "pub"),
    ("package.swift", "SwiftPM"),
    ("podfile", "CocoaPods"),
    ("cartfile", "Carthage"),
    ("mix.exs", "Mix"),
    ("rebar.config", "rebar3"),
    ("gleam.toml", "Gleam"),
    ("*.csproj", "NuGet"),
    ("*.fsproj", "NuGet"),
    ("*.vbproj", "NuGet"),
    ("packages.config", "NuGet"),
    ("directory.packages.props", "NuGet"),
    ("paket.dependencies", "Paket"),
    ("build.gradle", "Gradle"),
    ("build.gradle.kts", "Gradle"),
    ("settings.gradle", "Gradle"),
    ("settings.gradle.kts", "Gradle"),
    ("pom.xml", "Maven"),
    ("build.sbt", "sbt"),
    ("deps.edn", "Clojure CLI"),
    ("project.clj", "Leiningen"),
    ("stack.yaml", "Stack"),
    ("*.cabal", "Cabal"),
    ("cabal.project", "Cabal"),
    ("renv.lock", "renv"),
    ("project.toml", "Julia Pkg"),
    ("shard.yml", "Shards"),
    ("dub.*", "DUB"),
    ("*.nimble", "Nimble"),
    ("*.opam", "opam"),
    ("*.rockspec", "LuaRocks"),
    ("cpanfile", "cpanm"),
    ("vcpkg.json", "vcpkg"),
    ("conanfile.*", "Conan"),
    ("module.bazel", "Bazel"),
    ("workspace", "Bazel"),
    ("workspace.bazel", "Bazel"),
    ("build.zig.zon", "Zig"),
    ("elm.json", "Elm"),
    ("flake.nix", "Nix"),
    ("*.tf", "Terraform"),
    ("chart.yaml", "Helm"),
];
/// File names of workspace and monorepo tools, in display order.
const WORKSPACES: &[(&str, &str)] = &[
    ("pnpm-workspace.yaml", "pnpm"),
    ("lerna.json", "Lerna"),
    ("nx.json", "Nx"),
    ("turbo.json", "Turborepo"),
    ("rush.json", "Rush"),
    ("go.work", "Go"),
    ("settings.gradle", "Gradle"),
    ("settings.gradle.kts", "Gradle"),
    ("cabal.project", "Cabal"),
    ("melos.yaml", "Melos"),
    ("module.bazel", "Bazel"),
    ("workspace", "Bazel"),
    ("workspace.bazel", "Bazel"),
    ("pants.toml", "Pants"),
];
/// Instructions and settings of AI coding tools inside a project.
const AI_FILES: &[&str] = &["claude.md", "agents.md", "gemini.md", ".cursorrules", ".windsurfrules", ".clinerules"];
const AI_FOLDERS: &[&str] =
    &[".claude", ".cursor", ".codex", ".gemini", ".windsurf", ".roo", ".continue", ".clinerules"];
const CI: &[&str] =
    &[".github", ".gitlab-ci.yml", ".circleci", "azure-pipelines.yml", "jenkinsfile", ".woodpecker.yml"];
const SOURCE_FOLDERS: &[&str] = &["src", "lib", "app", "source", "pkg", "cmd"];
const EDITOR_FOLDERS: &[&str] = &[".vscode", ".idea", ".devcontainer"];
/// Suffixes of folders unpacked from a downloaded source archive.
const ARCHIVE_SUFFIXES: &[&str] = &["-main", "-master", "-develop", "-trunk"];
/// Names of test folders (Maven's `src/test` and SwiftPM's `Tests` among
/// them). `testing` is a common word: it counts only inside a project.
pub(super) const TEST_ROOTS: &[&str] = &[
    "tests",
    "test",
    "__tests__",
    "spec",
    "specs",
    "e2e",
    "e2e-tests",
    "integration-tests",
    "integration_tests",
    "integration_test",
    "unittests",
    "unit_tests",
    "unit-tests",
    "androidtest",
    "cypress",
    "testing",
];
/// Folders that hold data for tests wherever they are.
pub(super) const TEST_DATA_FOLDERS: &[&str] = &[
    "testdata",
    "test-data",
    "test_data",
    "testfiles",
    "test-files",
    "test-fixtures",
    "test_fixtures",
    "test-resources",
    "test_resources",
    "fixtures",
    "__fixtures__",
    "__snapshots__",
    "cassettes",
];
/// Data folders that count as test data directly inside a test folder.
pub(super) const DATA_IN_TESTS: &[&str] = &["data", "resources", "samples", "assets", "fixtures", "snapshots"];
/// Test data directly inside a test folder; deeper down or elsewhere the
/// word can name test code (`golden/GoldenMasterTest.java`).
pub(super) const GOLDEN: &[&str] = &["golden", "goldens"];
/// Outputs of test runs. `tidy` holds the trace that proves each of them.
pub(super) const TEST_OUTPUTS: &[&str] = &[
    "coverage",
    "htmlcov",
    "lcov-report",
    ".nyc_output",
    "test-results",
    "test-output",
    "test-reports",
    "testresults",
    "playwright-report",
    "blob-report",
    "allure-results",
    "allure-report",
    "surefire-reports",
    "failsafe-reports",
    "mutants.out",
    ".stryker-tmp",
];
/// Test frameworks in display order; a [`Tally`] keeps them as bits.
const FRAMEWORKS: &[&str] = &[
    "pytest",
    "tox",
    "nox",
    "behave",
    "coverage.py",
    "Jest",
    "Vitest",
    "Mocha",
    "AVA",
    "Karma",
    "Playwright",
    "Cypress",
    "WebdriverIO",
    "Nightwatch",
    "Stryker",
    "nyc",
    "c8",
    "Go test",
    "nextest",
    "TestNG",
    "VSTest",
    "xUnit",
    "PHPUnit",
    "RSpec",
    "ExUnit",
    "Common Test",
    "dart test",
    "Hspec",
    "testthat",
    "busted",
    "Pester",
    "Bats",
    "Robot Framework",
    "Gherkin",
    "Codecov",
];
/// Configuration files of test tools; one `*` stands for any characters.
const TEST_CONFIGS: &[(&str, &str)] = &[
    ("pytest.ini", "pytest"),
    ("conftest.py", "pytest"),
    ("tox.ini", "tox"),
    ("noxfile.py", "nox"),
    ("behave.ini", "behave"),
    (".coveragerc", "coverage.py"),
    ("jest.config.*", "Jest"),
    ("vitest.config.*", "Vitest"),
    ("vitest.workspace.*", "Vitest"),
    (".mocharc*", "Mocha"),
    ("ava.config.*", "AVA"),
    ("karma.conf.*", "Karma"),
    ("playwright.config.*", "Playwright"),
    ("cypress.config.*", "Cypress"),
    ("cypress.json", "Cypress"),
    ("wdio.conf.*", "WebdriverIO"),
    ("nightwatch.conf.*", "Nightwatch"),
    ("stryker.conf.*", "Stryker"),
    ("stryker.config.*", "Stryker"),
    (".nycrc*", "nyc"),
    (".c8rc*", "c8"),
    ("nextest.toml", "nextest"),
    ("testng.xml", "TestNG"),
    ("*.runsettings", "VSTest"),
    ("xunit.runner.json", "xUnit"),
    ("phpunit.xml", "PHPUnit"),
    ("phpunit.xml.dist", "PHPUnit"),
    (".rspec", "RSpec"),
    ("dart_test.yaml", "dart test"),
    ("codecov.yml", "Codecov"),
    ("codecov.yaml", "Codecov"),
    (".codecov.yml", "Codecov"),
    (".codecov.yaml", "Codecov"),
];
/// Source and script files that stand on their own.
const SCRIPT_EXTENSIONS: &[&str] = &[
    ".py", ".pyw", ".ipynb", ".ps1", ".psm1", ".sh", ".bash", ".bat", ".cmd", ".vbs", ".js", ".mjs", ".cjs", ".jsx",
    ".ts", ".mts", ".cts", ".tsx", ".vue", ".svelte", ".rb", ".pl", ".lua", ".sql", ".r", ".jl", ".rs", ".go", ".java",
    ".kt", ".kts", ".scala", ".groovy", ".clj", ".cljs", ".cljc", ".cs", ".fs", ".fsx", ".vb", ".c", ".cc", ".cpp",
    ".cxx", ".m", ".mm", ".swift", ".php", ".dart", ".ex", ".exs", ".erl", ".hs", ".elm", ".ml", ".zig", ".nim",
];
/// Headers are sources, but no loose scripts of their own: a folder of them
/// is an include directory, not a scratchpad.
const HEADER_EXTENSIONS: &[&str] = &[".h", ".hh", ".hpp", ".hxx"];
const MARKERS_SHOWN: usize = 4;
const MARKER_NAME_MAX: usize = 40;
const MANAGERS_SHOWN: usize = 5;
const FRAMEWORKS_SHOWN: usize = 4;

struct Pending {
    parent: Rc<DirHandle>,
    name: Option<String>,
    rel: String,
    depth: u32,
    lineage: Lineage,
}

/// `drive` marks a search over a whole drive root. Below `Users` it only
/// enters `own_profile`, the folder name of the current user's profile:
/// other people's profiles stay out even when access would be granted.
pub(super) fn scan(root: Rc<DirHandle>, budget: &mut Budget, drive: bool, own_profile: &str) -> GenerationResult {
    let mut result = GenerationResult::default();
    let mut census = Census::default();
    let mut pending =
        vec![Pending { parent: root, name: None, rel: String::new(), depth: 0, lineage: Lineage::default() }];
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
        if drive && next.depth > 0 && next.lineage.project.is_none() && program_folder(&entries) {
            system += 1;
            continue;
        }
        let mut inspection = inspect_directory(&next.rel, &entries, &next.lineage, drive);
        census.record(&next.rel, &mut inspection);
        result.items.append(&mut inspection.items);
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
                lineage: inspection.lineage.clone(),
            });
        }
        // The stack takes the last entry first: likely places go on top.
        if drive {
            pending[children..].sort_by_key(|p| p.name.as_deref().is_some_and(likely_place));
        }
    }
    census.finish(&mut result.items);
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
    let marker = entries.iter().any(|e| e.name_str().is_some_and(|n| is_marker(e.is_dir(), &n.to_ascii_lowercase())));
    has(".exe") && has(".dll") && !marker
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

/// The drive root or a profile folder on a drive search.
fn home_folder(rel: &str, drive: bool) -> bool {
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    drive
        && match parts.as_slice() {
            [] => true,
            [users, _] => users.eq_ignore_ascii_case("Users"),
            _ => false,
        }
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

/// One usable entry of a listing with its lower-case name.
struct Named<'a> {
    entry: &'a DirEntryInfo,
    name: &'a str,
    lower: String,
    dir: bool,
}

fn count_files(named: &[Named<'_>], test: impl Fn(&Named<'_>) -> bool) -> usize {
    named.iter().filter(|n| !n.dir && test(n)).count()
}

/// `name` against a lower-case pattern with at most one `*`.
fn matches_pattern(name: &str, pattern: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => {
            name.len() >= prefix.len() + suffix.len() && name.starts_with(prefix) && name.ends_with(suffix)
        }
        None => name == pattern,
    }
}

/// Labels of a name table with a matching file here, in table order.
fn matching_labels(table: &[(&str, &'static str)], named: &[Named<'_>]) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    for (pattern, label) in table {
        if !found.contains(label) && named.iter().any(|n| !n.dir && matches_pattern(&n.lower, pattern)) {
            found.push(label);
        }
    }
    found
}

/// `a, b, c +2`: a fixed number of labels and how many more there are.
fn bounded_list(labels: &[&str], shown: usize) -> String {
    let list = labels.iter().take(shown).copied().collect::<Vec<_>>().join(", ");
    match labels.len().saturating_sub(shown) {
        0 => list,
        more => format!("{list} +{more}"),
    }
}

/// What the names in a project folder say about it, before its tests are
/// counted. Filename evidence only; no manifest is read.
#[derive(Debug)]
struct Draft {
    markers: Vec<String>,
    notes: Vec<String>,
    confidence: u8,
    /// Signals before and after the tests signal, in display order.
    before: Vec<String>,
    after: Vec<String>,
    test_folder: bool,
}

impl Draft {
    /// Signals are stored unredacted: fixed labels and numbers only.
    fn signals(&self, tests: Tally) -> Vec<String> {
        let tests = match tests.label() {
            Some(label) => Some(format!("Tests ({label})")),
            None => self.test_folder.then(|| "Tests".to_string()),
        };
        self.before.iter().cloned().chain(tests).chain(self.after.iter().cloned()).collect()
    }

    /// Markers, hints and notes within the label limit. The marker list
    /// gives way first, down to one name, then the last hints; the
    /// confidence always stays.
    fn detail(&self, signals: &[String]) -> String {
        const SEPARATOR: &str = " · ";
        let separator = SEPARATOR.chars().count();
        let tail =
            format!("Sicherheit: {}. Nur Namen; Inhalt und Nutzung unbekannt.", confidence_label(self.confidence));
        let notes: Vec<String> = self.notes.iter().map(|n| privacy::label(n)).collect();
        let room = LABEL_MAX_CHARS.saturating_sub(
            tail.chars().count() + separator + notes.iter().map(|n| n.chars().count() + separator).sum::<usize>(),
        );
        let names: Vec<String> =
            self.markers.iter().map(|m| privacy::truncate(&privacy::label(m), MARKER_NAME_MAX)).collect();
        let shortest = fit("Projektmarker: ", &names, ", ", 0, 1).chars().count();
        let hints_room = room.saturating_sub(shortest + separator);
        let hints = Some(fit("Hinweise: ", signals, "; ", hints_room, signals.len()))
            .filter(|hints| !signals.is_empty() && hints.chars().count() <= hints_room);
        let head_room = room.saturating_sub(hints.as_ref().map_or(0, |h| h.chars().count() + separator));
        let head = fit("Projektmarker: ", &names, ", ", head_room, MARKERS_SHOWN);
        let body = std::iter::once(head).chain(hints).chain(notes).collect::<Vec<_>>().join(SEPARATOR);
        // Only very long folder names in the notes still need a cut here.
        let room = LABEL_MAX_CHARS.saturating_sub(separator + tail.chars().count());
        format!("{}{SEPARATOR}{tail}", privacy::truncate(&body, room))
    }
}

/// `prefix` with up to `limit` items that fit into `room` characters and the
/// rest as `+N`. At least the first item, which the caller may still cut.
fn fit(prefix: &str, items: &[String], joint: &str, room: usize, limit: usize) -> String {
    let mut shown = items.len().min(limit);
    loop {
        let mut text = format!("{prefix}{}", items[..shown].join(joint));
        if shown < items.len() {
            text.push_str(&format!(" +{}", items.len() - shown));
        }
        if shown <= 1 || text.chars().count() <= room {
            return text;
        }
        shown -= 1;
    }
}

fn assess_project(rel: &str, base: &str, named: &[Named<'_>], inherited: Option<&str>, markers: Vec<String>) -> Draft {
    let file = |list: &[&str]| named.iter().any(|n| !n.dir && list.contains(&n.lower.as_str()));
    let folder = |list: &[&str]| named.iter().any(|n| n.dir && list.contains(&n.lower.as_str()));
    let any = |list: &[&str]| named.iter().any(|n| list.contains(&n.lower.as_str()));
    let vcs = any(VCS);
    let ci = any(CI);
    let manifest = file(MANIFESTS)
        || named.iter().any(|n| !n.dir && MANIFEST_EXTENSIONS.iter().any(|e| n.lower.ends_with(e)))
        || named.iter().any(|n| n.dir && XCODE_BUNDLES.iter().any(|e| n.lower.ends_with(e)))
        || unity(named);
    // A manager-specific lockfile counts as one too.
    let lockfile = file(LOCKFILES);
    let sources = folder(SOURCE_FOLDERS) || count_files(named, |n| is_source(&n.lower)) >= 3;
    // The detail keeps the first hints when space runs out: the most telling first.
    let managers = matching_labels(MANAGERS, named);
    let workspaces = matching_labels(WORKSPACES, named);
    let mut before = Vec::new();
    if !managers.is_empty() {
        before.push(format!("Paketmanager: {}", bounded_list(&managers, MANAGERS_SHOWN)));
    }
    if !workspaces.is_empty() {
        before.push(format!("Workspace ({})", workspaces.join(", ")));
    }
    let mut after = Vec::new();
    for (present, label) in [
        (named.iter().any(|n| !n.dir && test_project_file(n.name)), "Testprojekt"),
        (vcs, "Versionsverwaltung"),
        (lockfile, "Lockfile"),
        (sources, "Quellcode"),
        (ci, "CI"),
    ] {
        if present {
            after.push(label.to_string());
        }
    }
    let ai: Vec<&str> = named
        .iter()
        .filter(|n| if n.dir { AI_FOLDERS.contains(&n.lower.as_str()) } else { AI_FILES.contains(&n.lower.as_str()) })
        .map(|n| n.name)
        .take(4)
        .collect();
    if !ai.is_empty() {
        after.push(format!("KI-Dateien ({})", ai.join(", ")));
    }
    if folder(EDITOR_FOLDERS) {
        after.push("Editor-Einstellungen".to_string());
    }
    let mut confidence = match (vcs, manifest, lockfile || sources) {
        (true, true, _) | (false, true, true) => 3,
        // A repository with its own CI, as many CMake and Make projects are.
        (true, false, _) if ci => 3,
        (true, false, _) | (false, true, false) => 2,
        _ => 1,
    };
    let mut notes = Vec::new();
    // The project scanner's root inherits itself: no sub-project of its own.
    if let Some(parent) = inherited.filter(|parent| *parent != rel) {
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
    Draft { markers, notes, confidence, before, after, test_folder: folder(TEST_ROOTS) }
}

/// A .NET project file whose name says it holds tests: `App.Tests.csproj`,
/// `AppUnitTests.fsproj`, `App.Specs.csproj`.
fn test_project_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(extension) = [".csproj", ".fsproj", ".vbproj"].into_iter().find(|e| lower.ends_with(e)) else {
        return false;
    };
    let stem = &name[..name.len() - extension.len()];
    ["Tests", "Test", "Specs"].iter().any(|s| stem.len() > s.len() && stem.ends_with(s))
        || stem.rsplit(['.', '_', '-']).next().is_some_and(|word| {
            ["tests", "test", "specs", "unittests", "integrationtests"].iter().any(|s| word.eq_ignore_ascii_case(s))
        })
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

/// What a walker hands down from the folders above a directory.
#[derive(Clone, Debug, Default)]
pub(super) struct Lineage {
    /// Nearest project, relative to the root; `""` is the root itself.
    pub project: Option<String>,
    /// Outermost test folder above: only that one is reported.
    pub tests: Option<String>,
    /// Inside test data, where nothing counts as a test file.
    pub data: bool,
}

/// Test files and test tools named in one listing, summed per project and
/// per test folder. Frameworks are bits of [`FRAMEWORKS`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    files: u64,
    frameworks: u64,
}

impl Tally {
    fn add(&mut self, other: Tally) {
        self.files = self.files.saturating_add(other.files);
        self.frameworks |= other.frameworks;
    }

    fn is_empty(self) -> bool {
        self.files == 0 && self.frameworks == 0
    }

    /// `37 Testdateien · pytest, Playwright`, or `None` when nothing was seen.
    fn label(self) -> Option<String> {
        let names: Vec<&str> = FRAMEWORKS
            .iter()
            .enumerate()
            .filter(|(bit, _)| self.frameworks & (1 << bit) != 0)
            .map(|(_, name)| *name)
            .collect();
        let frameworks = (!names.is_empty()).then(|| bounded_list(&names, FRAMEWORKS_SHOWN));
        let files = match self.files {
            0 => None,
            1 => Some("1 Testdatei".to_string()),
            n => Some(format!("{n} Testdateien")),
        };
        match (files, frameworks) {
            (Some(files), Some(frameworks)) => Some(format!("{files} · {frameworks}")),
            (files, frameworks) => files.or(frameworks),
        }
    }
}

fn framework(label: &str) -> u64 {
    FRAMEWORKS.iter().position(|f| *f == label).map_or(0, |bit| 1 << bit)
}

/// The test convention a file name follows, as framework bits (0 for one
/// that several frameworks share), or `None` for no test file. CamelCase
/// suffixes are compared as written: `Edit.java` is no `*IT` test and
/// `Latest.cs` no `*Test`.
fn test_file(name: &str, lower: &str) -> Option<u64> {
    let (stem, extension) = lower.rsplit_once('.')?;
    // ASCII lower-casing keeps byte offsets, so the original stem lines up.
    let written = &name[..stem.len()];
    let camel = |suffixes: &[&str]| {
        suffixes.iter().any(|s| {
            written.len() > s.len()
                && written.ends_with(s)
                && (*s != "IT"
                    || written[..written.len() - 2]
                        .chars()
                        .last()
                        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
        })
    };
    let any = |found: bool| found.then_some(0);
    let by = |found: bool, label: &str| found.then(|| framework(label));
    match extension {
        "py" if stem == "conftest" => Some(framework("pytest")),
        "py" => any(stem.starts_with("test_") || stem.ends_with("_test") || stem == "tests"),
        "js" | "jsx" | "ts" | "tsx" if stem.ends_with(".cy") => Some(framework("Cypress")),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts" => {
            any(stem.ends_with(".test") || stem.ends_with(".spec") || stem.ends_with(".e2e-spec"))
        }
        "go" => by(stem.ends_with("_test"), "Go test"),
        "rs" => any(stem.ends_with("_test") || stem.ends_with("_tests")),
        "java" | "kt" | "scala" | "groovy" => any(camel(&["Test", "Tests", "IT", "Spec"])),
        "cs" | "fs" | "vb" => any(camel(&["Test", "Tests"])),
        "rb" if stem.ends_with("_spec") => Some(framework("RSpec")),
        "rb" => any(stem.ends_with("_test")),
        "php" => any(camel(&["Test"])),
        "exs" => by(stem.ends_with("_test"), "ExUnit"),
        "erl" => by(stem.ends_with("_suite"), "Common Test"),
        "dart" => any(stem.ends_with("_test")),
        "swift" => any(camel(&["Tests"])),
        "c" | "cc" | "cpp" | "cxx" => any(stem.ends_with("_test")
            || (matches!(extension, "cc" | "cpp") && stem.ends_with("_unittest"))
            || (matches!(extension, "c" | "cpp") && stem.starts_with("test_"))),
        "hs" => by(camel(&["Spec"]), "Hspec"),
        "ps1" => by(stem.ends_with(".tests"), "Pester"),
        "lua" => by(stem.ends_with("_spec"), "busted"),
        "r" => by(stem.starts_with("test-") || stem.starts_with("test_"), "testthat"),
        "jl" => any(stem == "runtests"),
        "bats" => Some(framework("Bats")),
        "robot" => Some(framework("Robot Framework")),
        "feature" => Some(framework("Gherkin")),
        _ => None,
    }
}

/// Frameworks a test tool's configuration file names.
fn config_frameworks(lower: &str) -> u64 {
    TEST_CONFIGS
        .iter()
        .filter(|(pattern, _)| matches_pattern(lower, pattern))
        .fold(0, |bits, (_, label)| bits | framework(label))
}

/// The detail of a test folder: what its names say, never its content.
fn tests_detail(tally: Tally) -> String {
    let found = tally.label().unwrap_or_else(|| "keine Testdateien nach Namen erkannt".into());
    format!("Testordnertypischer Name · {found}; Inhalt nicht gelesen")
}

/// Shared with the ordinary project scanner, without a second traversal or
/// content read. Adapter-claimed stores remain bounded leaves in discovery.
pub(super) struct DirectoryInspection {
    pub items: Vec<InventoryItem>,
    /// What the folders below inherit.
    pub lineage: Lineage,
    pub leaf: bool,
    tally: Tally,
    draft: Option<Draft>,
}

/// Test files and frameworks of one walk, per nearest project and per
/// outermost test folder. A walker records every folder it inspects and
/// finishes once; claimed folders and leaves add nothing.
#[derive(Default)]
pub(super) struct Census {
    projects: HashMap<String, Tally>,
    roots: HashMap<String, Tally>,
    drafts: HashMap<String, Draft>,
}

impl Census {
    pub fn record(&mut self, rel: &str, inspection: &mut DirectoryInspection) {
        if let Some(draft) = inspection.draft.take() {
            self.drafts.insert(rel.to_string(), draft);
        }
        let tally = inspection.tally;
        if tally.is_empty() {
            return;
        }
        for (key, sums) in
            [(&inspection.lineage.project, &mut self.projects), (&inspection.lineage.tests, &mut self.roots)]
        {
            let Some(key) = key else { continue };
            match sums.get_mut(key.as_str()) {
                Some(sum) => sum.add(tally),
                None => {
                    sums.insert(key.clone(), tally);
                }
            }
        }
    }

    /// Writes the counts into the project and test folder findings.
    pub fn finish(mut self, items: &mut [InventoryItem]) {
        for item in items.iter_mut().filter(|i| i.ecosystem == "discovery") {
            let Some(evidence) = item.evidence.discovery.as_mut() else { continue };
            match evidence.kind {
                DiscoveryKind::Project => {
                    let Some(draft) = self.drafts.remove(&item.rel_path) else { continue };
                    let tally = self.projects.get(&item.rel_path).copied().unwrap_or_default();
                    evidence.signals = draft.signals(tally);
                    item.detail = privacy::label(&draft.detail(&evidence.signals));
                }
                DiscoveryKind::Tests => {
                    let tally = self.roots.get(&item.rel_path).copied().unwrap_or_default();
                    item.detail = privacy::label(&tests_detail(tally));
                }
                _ => {}
            }
        }
    }
}

/// `drive` lists single loose scripts only where people drop files (desktop,
/// downloads, documents) or under a throwaway name: elsewhere on a drive they
/// are mostly tool and installer files. Two or more in one folder still
/// report that folder as a possible scratchpad.
pub(super) fn inspect_directory(
    rel: &str,
    entries: &[DirEntryInfo],
    above: &Lineage,
    drive: bool,
) -> DirectoryInspection {
    let safe: Vec<&DirEntryInfo> = entries.iter().filter(|e| !e.is_reparse() && !e.is_placeholder()).collect();
    let named: Vec<Named<'_>> = safe
        .iter()
        .filter_map(|e| {
            e.name_str().map(|name| Named { entry: e, name, lower: name.to_ascii_lowercase(), dir: e.is_dir() })
        })
        .collect();
    let base = rel.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    let inherited_project = above.project.as_deref();
    let test_name = TEST_ROOTS.contains(&base.as_str());
    let markers = project_markers(&base, &named, inherited_project.is_some(), test_name, home_folder(rel, drive));
    let mut project = if markers.is_empty() { inherited_project.map(str::to_string) } else { Some(rel.to_string()) };
    let mut items = Vec::new();
    let has_file = |wanted: &str| named.iter().any(|n| !n.dir && n.lower == wanted);
    let has_folder = |wanted: &str| named.iter().any(|n| n.dir && n.lower == wanted);
    let scripts = count_files(&named, |n| is_script(&n.lower));
    let notebooks = count_files(&named, |n| n.lower.ends_with(".ipynb"));
    let data = count_files(&named, |n| DATA_EXTENSIONS.iter().any(|e| n.lower.ends_with(e)));
    let named_tests = count_files(&named, |n| test_file(n.name, &n.lower).is_some());
    let dev_folder = named.iter().any(|n| n.dir && DEV_FOLDERS.contains(&n.lower.as_str()));
    let in_tests = above.tests.is_some();
    let has_dev_context = project.is_some() || scripts > 0 || dev_folder || in_tests;
    let test_root = test_name
        && !in_tests
        && !above.data
        && markers.is_empty()
        && (has_dev_context || named_tests > 0)
        && (base != "testing" || inherited_project.is_some());
    let directory_kind: Option<(DiscoveryKind, String, bool)> = if has_file("pyvenv.cfg") {
        Some((DiscoveryKind::Environment, "Umgebungsmarker pyvenv.cfg".into(), true))
    } else if base == "node_modules" {
        Some((DiscoveryKind::Dependencies, "Ordnername node_modules; Installationszustand nicht geprüft".into(), true))
    } else if matches!(base.as_str(), ".npm" | "_cacache" | ".pnpm-store" | ".uv-cache") {
        Some((DiscoveryKind::Cache, "Werkzeugtypischer Cachename; gemeinsame Nutzung unbekannt".into(), true))
    } else if let Some(hint) = directory_hint(&base).filter(|_| has_dev_context) {
        Some((hint.0, hint.1.to_string(), hint.2))
    } else if has_file("cachedir.tag") {
        // uv, Hugging Face and others mark their caches this way, whatever
        // the folder is called; the packages inside are no user projects.
        Some((DiscoveryKind::Cache, "Cachemarker CACHEDIR.TAG; gemeinsame Nutzung unbekannt".into(), true))
    } else if in_tests
        && (GOLDEN.contains(&base.as_str()) || DATA_IN_TESTS.contains(&base.as_str()))
        && in_test_folder(rel)
    {
        // Directly in a test root only: deeper down, `golden/` or
        // `resources/` can hold test code (`GoldenMasterTest.java`).
        Some((
            DiscoveryKind::TestData,
            "Datenordner in einem Testordner; kann selbst gepflegte Daten enthalten".into(),
            false,
        ))
    } else if base == "library" && (has_folder("packagecache") || has_folder("scriptassemblies")) {
        // Unity's cache: its packages would otherwise read as sub-projects.
        Some((
            DiscoveryKind::Cache,
            "Unity-Bibliothek mit PackageCache oder ScriptAssemblies; Nutzung unbekannt".into(),
            true,
        ))
    } else if test_root {
        Some((DiscoveryKind::Tests, String::new(), false))
    } else if let Some(reason) = scratch_reason(&base).filter(|_| has_dev_context || data > 0) {
        let context = if has_dev_context { "Entwicklungshinweisen" } else { "Datendateien" };
        Some((DiscoveryKind::Scratchpad, format!("{reason} mit {context}; Zweck unbekannt"), false))
    } else {
        None
    };
    // Loose scripts or a notebook outside every project: a working folder.
    let directory_kind = directory_kind.or_else(|| {
        (markers.is_empty() && inherited_project.is_none() && !rel.is_empty() && (scripts >= 2 || notebooks > 0)).then(
            || {
                (
                    DiscoveryKind::Scratchpad,
                    format!("{scripts} lose Skripte oder Notebooks ohne Projektmarker; Zweck unbekannt"),
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
    let kind_here = |wanted: DiscoveryKind| directory_kind.as_ref().is_some_and(|(kind, ..)| *kind == wanted);
    let lineage = Lineage {
        tests: if kind_here(DiscoveryKind::Tests) { Some(rel.to_string()) } else { above.tests.clone() },
        data: above.data || kind_here(DiscoveryKind::TestData),
        project,
    };
    // Test files by their names; inside a test folder every source file.
    let mut tally = Tally::default();
    if !leaf && !lineage.data {
        let in_root = lineage.tests.is_some();
        for n in named.iter().filter(|n| !n.dir) {
            if let Some(bits) = test_file(n.name, &n.lower).or_else(|| (in_root && is_source(&n.lower)).then_some(0)) {
                tally.add(Tally { files: 1, frameworks: bits });
            }
            tally.frameworks |= config_frameworks(&n.lower);
        }
    }
    let mut draft = None;
    if let Some((kind, evidence, _)) = directory_kind {
        let modified = (kind == DiscoveryKind::Scratchpad).then(|| newest_day(&safe)).flatten();
        let evidence = if kind == DiscoveryKind::Tests { tests_detail(tally) } else { evidence };
        items.push(finding(kind, rel, true, None, modified, lineage.project.as_deref(), &evidence));
    } else if !markers.is_empty() {
        let assessed = assess_project(rel, &base, &named, inherited_project, markers);
        let signals = assessed.signals(tally);
        // A nested project keeps the outer one as its context.
        let context = inherited_project.unwrap_or(rel);
        let detail = assessed.detail(&signals);
        let mut item = finding(DiscoveryKind::Project, rel, true, None, newest_day(&safe), Some(context), &detail);
        if let Some(evidence) = item.evidence.discovery.as_mut() {
            evidence.confidence = assessed.confidence;
            evidence.signals = signals;
        }
        items.push(item);
        draft = Some(assessed);
    }
    if !leaf {
        let scripts_here = !drive || filing_place(rel);
        let loose = lineage.project.is_none();
        for n in named.iter().filter(|n| !n.dir) {
            let lower = n.lower.as_str();
            let kind = if lower.ends_with(".ipynb") {
                Some((DiscoveryKind::Notebook, "Dateiendung .ipynb; Inhalt und Zweck unbekannt"))
            } else if loose && is_script(lower) && scripts_here {
                Some((DiscoveryKind::Script, "Quellcode-/Skriptendung ohne gefundenen Projektmarker"))
            } else if loose && is_script(lower) && throwaway_file(lower) {
                Some((DiscoveryKind::Script, "Wegwerftypischer Dateiname ohne Projektmarker"))
            } else if has_dev_context && [".sqlite", ".sqlite3", ".db", ".duckdb"].iter().any(|e| lower.ends_with(e)) {
                Some((DiscoveryKind::Database, "Datenbankendung im Entwicklungskontext; Testzweck unbekannt"))
            } else {
                None
            };
            if let Some((kind, evidence)) = kind {
                items.push(finding(
                    kind,
                    &fsread::join(rel, n.name),
                    false,
                    i64::try_from(n.entry.size).ok(),
                    Some(n.entry.modified),
                    lineage.project.as_deref(),
                    evidence,
                ));
            }
        }
    }
    DirectoryInspection { items, lineage, leaf, tally, draft }
}

/// Names that make this folder a project. Inside a project, configs of a
/// part and a test folder's build files make none; on a drive, tool version
/// files in the home folder pin global versions.
fn project_markers(base: &str, named: &[Named<'_>], inside: bool, test_name: bool, home: bool) -> Vec<String> {
    // An Xcode bundle belongs to the project around it.
    if XCODE_BUNDLES.iter().any(|e| base.ends_with(e)) {
        return Vec::new();
    }
    let counts = |n: &&Named<'_>| {
        let lower = n.lower.as_str();
        let test_part = test_name && (TEST_BUILD_FILES.contains(&lower) || marker_rank(n.dir, lower) == 1);
        is_marker(n.dir, lower)
            && !(inside && (PART_CONFIGS.contains(&lower) || test_part))
            && !(home && HOME_CONFIGS.contains(&lower))
    };
    let mut markers: Vec<(u8, String)> =
        named.iter().filter(counts).map(|n| (marker_rank(n.dir, &n.lower), n.name.to_string())).collect();
    if unity(named) {
        markers.push((0, "Assets + ProjectSettings".into()));
    }
    // Manifests first: a shortened detail still names one.
    markers.sort_by_key(|(rank, _)| *rank);
    markers.into_iter().map(|(_, name)| name).take(12).collect()
}

/// Manifests and bundles before weak markers, version control last.
fn marker_rank(dir: bool, lower: &str) -> u8 {
    if lower == ".git" || (dir && VCS.contains(&lower)) {
        2
    } else if !dir && (WEAK_MARKERS.contains(&lower) || WEAK_EXTENSIONS.iter().any(|e| lower.ends_with(e))) {
        1
    } else {
        0
    }
}

fn is_marker(dir: bool, lower: &str) -> bool {
    if dir {
        VCS.contains(&lower) || XCODE_BUNDLES.iter().any(|e| lower.ends_with(e))
    } else {
        lower == ".git"
            || MANIFESTS.contains(&lower)
            || WEAK_MARKERS.contains(&lower)
            || MANIFEST_EXTENSIONS.iter().chain(WEAK_EXTENSIONS).any(|e| lower.ends_with(e))
    }
}

/// `Assets` and `ProjectSettings` side by side: a Unity project.
fn unity(named: &[Named<'_>]) -> bool {
    let folder = |wanted: &str| named.iter().any(|n| n.dir && n.lower == wanted);
    folder("assets") && folder("projectsettings")
}

/// Whether the folder at `rel` lies directly inside a test folder.
fn in_test_folder(rel: &str) -> bool {
    let mut parts = rel.rsplit('/');
    parts.next();
    parts.next().is_some_and(|parent| TEST_ROOTS.iter().any(|t| t.eq_ignore_ascii_case(parent)))
}

/// A source file of any language, headers included (lower-case name).
fn is_source(lower: &str) -> bool {
    is_script(lower) || HEADER_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

/// A source or script file that can stand alone (lower-case name).
fn is_script(lower: &str) -> bool {
    SCRIPT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

fn directory_hint(base: &str) -> Option<(DiscoveryKind, &'static str, bool)> {
    match base {
        ".venv" | "venv" | ".tox" | ".nox" | "conda-meta" | ".pixi" => Some((
            DiscoveryKind::Environment,
            "Umgebungstypischer Name; kein Installations- oder Nutzungsnachweis",
            true,
        )),
        "target" | "dist" | "build" | "bin" | "obj" | "out" | ".next" | ".nuxt" | ".build" => {
            Some((DiscoveryKind::BuildOutput, "Ausgabetypischer Name im Entwicklungskontext; Erzeuger unbekannt", true))
        }
        "vendor" | "third_party" | "third-party" | "bower_components" | "jspm_packages" | "pods" | "deps"
        | "vcpkg_installed" | ".terraform" => Some((
            DiscoveryKind::Dependencies,
            "Typischer Ordner für mitgelieferten Fremdcode; Herkunft nicht geprüft",
            true,
        )),
        ".cache" | "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" | ".gradle" | ".turbo"
        | ".parcel-cache" | ".hypothesis" | ".phpunit.cache" => {
            Some((DiscoveryKind::Cache, "Cachetypischer Name im Entwicklungskontext; Nutzung unbekannt", true))
        }
        _ if TEST_OUTPUTS.contains(&base) => {
            Some((DiscoveryKind::TestOutput, "Testausgabetypischer Name; Wiederherstellbarkeit unbekannt", true))
        }
        _ if TEST_DATA_FOLDERS.contains(&base) => Some((
            DiscoveryKind::TestData,
            "Testdatentypischer Name; kann selbst gepflegte Quelldaten enthalten",
            false,
        )),
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
        // Test code is the project's own: never an output, so no tidy item
        // and no deny rule of the context filter.
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
    use std::collections::BTreeSet;
    use std::path::Path;

    fn put(root: &Path, rel: &str) {
        write(root, rel, "untrusted: never read");
    }

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn search(root: &Path, drive: bool) -> GenerationResult {
        let handle = Rc::new(DirHandle::open_root(root, false).unwrap());
        scan(handle, &mut Budget::new(ScanLimits::drive_search()), drive, "alice")
    }

    fn found(result: &GenerationResult) -> Vec<(String, DiscoveryKind)> {
        result
            .items
            .iter()
            .filter_map(|i| i.evidence.discovery.as_ref().map(|d| (i.rel_path.clone(), d.kind)))
            .collect()
    }

    fn finding_at<'a>(result: &'a GenerationResult, rel: &str) -> &'a InventoryItem {
        result
            .items
            .iter()
            .find(|i| i.ecosystem == "discovery" && i.rel_path == rel)
            .unwrap_or_else(|| panic!("{rel}: {:?}", found(result)))
    }

    fn kind_at(result: &GenerationResult, rel: &str) -> DiscoveryKind {
        finding_at(result, rel).evidence.discovery.as_ref().unwrap().kind
    }

    fn signals(result: &GenerationResult, rel: &str) -> Vec<String> {
        let evidence = finding_at(result, rel).evidence.discovery.as_ref().unwrap();
        assert_eq!(evidence.kind, DiscoveryKind::Project, "{rel}");
        evidence.signals.clone()
    }

    fn tests_signal(result: &GenerationResult, rel: &str) -> Option<String> {
        signals(result, rel).into_iter().find(|s| s.starts_with("Tests"))
    }

    fn rels(result: &GenerationResult, kind: DiscoveryKind) -> BTreeSet<String> {
        found(result).into_iter().filter(|(_, k)| *k == kind).map(|(rel, _)| rel).collect()
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
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
    fn more_languages_keep_the_drive_rules_for_loose_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        drive_layout(root);
        for rel in [
            "SDK/include/api.h",
            "SDK/include/types.hpp",
            "SDK/include/detail/impl.hh",
            "Tools/widget.kt",
            "Users/alice/Desktop/notes.php",
            "Users/alice/Desktop/sketch.svelte",
            "Shared/lib/Helpers.swift",
            "Users/alice/Downloads/test_upload.dart",
        ] {
            put(root, rel);
        }
        let result = search(root, true);
        assert_eq!((result.files_read, result.bytes_read), (0, 0), "names only");
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            rels(&result, DiscoveryKind::Script),
            set(&[
                "Tools/try_parser.py",
                "Users/alice/Downloads/setup.bat",
                "Users/alice/Desktop/notes.php",
                "Users/alice/Desktop/sketch.svelte",
                "Users/alice/Downloads/test_upload.dart",
            ]),
            "single files only in filing places or with a throwaway name"
        );
        let found = found(&result);
        assert!(found.iter().all(|(rel, _)| !rel.starts_with("SDK") && !rel.starts_with("Shared")), "{found:?}");
        assert_eq!(kind_at(&result, "Tools"), DiscoveryKind::Scratchpad, "Kotlin counts like any script");
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
    fn tool_versions_in_the_home_folder_make_no_project() {
        let temp = tempfile::tempdir().unwrap();
        drive_layout(temp.path());
        put(temp.path(), "Users/alice/.tool-versions");
        put(temp.path(), "Users/alice/mise.toml");
        put(temp.path(), ".tool-versions");
        let result = search(temp.path(), true);
        let found = found(&result);
        assert!(!found.contains(&("Users/alice".into(), DiscoveryKind::Project)), "{found:?}");
        assert!(!found.contains(&(String::new(), DiscoveryKind::Project)), "{found:?}");
        let api = finding_at(&result, "Users/alice/source/repos/api");
        assert_eq!(api.evidence.context, "Users/alice/source/repos/api", "an outer project, not a member");

        let elsewhere = tempfile::tempdir().unwrap();
        put(elsewhere.path(), "tools/.tool-versions");
        let result = search(elsewhere.path(), false);
        assert_eq!(kind_at(&result, "tools"), DiscoveryKind::Project, "elsewhere it marks a project");
        assert_eq!(finding_at(&result, "tools").evidence.discovery.as_ref().unwrap().confidence, 1);
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
        for signal in ["Versionsverwaltung", "Paketmanager: Cargo", "Lockfile", "Tests (1 Testdatei)", "CI"] {
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
    fn manifests_and_lockfiles_of_more_ecosystems() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "swift/Package.swift",
            "swift/Package.resolved",
            "ios/Podfile",
            "ios/Podfile.lock",
            "crystal/shard.yml",
            "crystal/shard.lock",
            "julia/Project.toml",
            "julia/Manifest.toml",
            "haskell/cabal.project",
            "haskell/cabal.project.freeze",
            "bazel/MODULE.bazel",
            "bazel/MODULE.bazel.lock",
            "conan/conanfile.py",
            "conan/conan.lock",
            "perl/cpanfile",
            "perl/cpanfile.snapshot",
            "pixi/pixi.toml",
            "pixi/pixi.lock",
            "gowork/go.work",
            "gowork/go.work.sum",
            "jvm/settings.gradle.kts",
            "jvm/gradle.lockfile",
            "clojure/deps.edn",
            "gem/tool.gemspec",
            "nim/tool.nimble",
            "embedded/platformio.ini",
            "warehouse/Warehouse.sqlproj",
            "zig/build.zig.zon",
            "ocaml/dune-project",
            "terraform/main.tf",
            "terraform/.terraform.lock.hcl",
            "chart/Chart.yaml",
            "site/mkdocs.yml",
            "ts/tsconfig.json",
            "dotnet/global.json",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        let confidence = |rel: &str| finding_at(&result, rel).evidence.discovery.as_ref().unwrap().confidence;
        for rel in ["swift", "ios", "crystal", "julia", "haskell", "bazel", "conan", "perl", "pixi", "gowork", "jvm"] {
            assert_eq!(confidence(rel), 3, "{rel}: manifest and lockfile");
            assert!(signals(&result, rel).iter().any(|s| s == "Lockfile"), "{rel}");
        }
        for rel in ["clojure", "gem", "nim", "embedded", "warehouse", "zig", "ocaml"] {
            assert_eq!(confidence(rel), 2, "{rel}: a manifest of its own");
        }
        for rel in ["terraform", "chart", "site", "ts", "dotnet"] {
            assert_eq!(confidence(rel), 1, "{rel}: a weak marker");
        }
    }

    #[test]
    fn xcode_bundles_and_unity_folders_mark_projects() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "mac/App.xcodeproj/project.pbxproj",
            "mac/App.xcodeproj/project.xcworkspace/contents.xcworkspacedata",
            "mac/App.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved",
            "mac/App/main.swift",
            "game/Assets/Scenes/Main.unity",
            "game/ProjectSettings/ProjectVersion.txt",
            "game/Packages/manifest.json",
            "game/Library/PackageCache/com.unity.test-framework@1.4.5/package.json",
            "game/Library/ScriptAssemblies/Assembly-CSharp.dll",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        assert_eq!(rels(&result, DiscoveryKind::Project), set(&["game", "mac"]), "{:?}", found(&result));
        let mac = finding_at(&result, "mac");
        assert!(mac.detail.contains("App.xcodeproj"), "{}", mac.detail);
        assert_eq!(mac.evidence.discovery.as_ref().unwrap().confidence, 3, "a bundle counts like a manifest");
        let game = finding_at(&result, "game");
        assert!(game.detail.contains("Assets + ProjectSettings"), "{}", game.detail);
        assert_eq!(kind_at(&result, "game/Library"), DiscoveryKind::Cache);
        assert!(found(&result).iter().all(|(rel, _)| !rel.starts_with("game/Library/")), "{:?}", found(&result));
    }

    #[test]
    fn package_managers_and_workspaces_come_from_their_own_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "mono/package.json",
            "mono/pnpm-lock.yaml",
            "mono/pnpm-workspace.yaml",
            "mono/turbo.json",
            "mono/Cargo.toml",
            "bare/package.json",
            "py/pyproject.toml",
            "py/poetry.lock",
            "py/requirements-dev.txt",
            "jvm/settings.gradle.kts",
            "jvm/build.gradle.kts",
            "net/App.csproj",
            "net/Directory.Packages.props",
            "gw/go.work",
            "gw/go.mod",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        let mono = signals(&result, "mono");
        assert!(mono.contains(&"Paketmanager: pnpm, Cargo".to_string()), "{mono:?}");
        assert!(mono.contains(&"Workspace (pnpm, Turborepo)".to_string()), "{mono:?}");
        assert!(signals(&result, "bare").iter().all(|s| !s.starts_with("Paketmanager")), "package.json alone");
        assert!(signals(&result, "py").contains(&"Paketmanager: Poetry, pip".to_string()));
        let jvm = signals(&result, "jvm");
        assert!(jvm.contains(&"Paketmanager: Gradle".to_string()) && jvm.contains(&"Workspace (Gradle)".to_string()));
        assert!(signals(&result, "net").contains(&"Paketmanager: NuGet".to_string()));
        let gw = signals(&result, "gw");
        assert!(gw.contains(&"Paketmanager: Go-Module".to_string()) && gw.contains(&"Workspace (Go)".to_string()));
        let detail = &finding_at(&result, "mono").detail;
        assert!(detail.contains("Paketmanager: pnpm, Cargo") && detail.contains("Sicherheit: hoch"), "{detail}");
    }

    #[test]
    fn long_names_never_push_out_the_confidence() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let outer = format!("outer-{}", "o".repeat(120));
        put(root, &format!("{outer}/package.json"));
        for index in 0..14 {
            put(root, &format!("{outer}/Company.Product.Feature{index:02}.Implementation.Tests.csproj"));
        }
        for rel in ["pnpm-lock.yaml", "turbo.json", "nx.json", "lerna.json", ".git/HEAD", ".github/ci.yml", "CLAUDE.md"]
        {
            put(root, &format!("{outer}/{rel}"));
        }
        put(root, &format!("{outer}/inner-1.2.3/package.json"));
        let result = search(root, false);
        for rel in [outer.clone(), format!("{outer}/inner-1.2.3")] {
            let item = finding_at(&result, &rel);
            assert!(item.detail.chars().count() <= LABEL_MAX_CHARS, "{}", item.detail);
            assert!(item.detail.contains("Sicherheit: "), "{}", item.detail);
            assert!(item.detail.ends_with("Nur Namen; Inhalt und Nutzung unbekannt."), "{}", item.detail);
        }
        let detail = &finding_at(&result, &outer).detail;
        assert!(detail.contains(" +"), "markers and hints are shortened: {detail}");
        let signals = signals(&result, &outer);
        assert!(signals.iter().all(|s| !s.contains("outer-") && !s.contains("Company")), "{signals:?}");
    }

    #[test]
    fn go_tests_beside_the_sources_count_for_the_project() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "svc/go.mod",
            "svc/go.sum",
            "svc/main.go",
            "svc/main_test.go",
            "svc/internal/store/store.go",
            "svc/internal/store/store_test.go",
            "web/package.json",
            "web/src/app.ts",
            "web/src/app.test.ts",
            "web/src/view.spec.tsx",
            "web/vitest.config.ts",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        assert_eq!(tests_signal(&result, "svc").as_deref(), Some("Tests (2 Testdateien · Go test)"));
        assert_eq!(tests_signal(&result, "web").as_deref(), Some("Tests (2 Testdateien · Vitest)"));
        assert!(rels(&result, DiscoveryKind::Tests).is_empty(), "no test folder: {:?}", found(&result));
    }

    #[test]
    fn a_python_project_reports_its_outermost_test_folder_once() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "app/pyproject.toml",
            "app/uv.lock",
            "app/src/app/__init__.py",
            "app/tests/conftest.py",
            "app/tests/test_api.py",
            "app/tests/unit/test_model.py",
            "app/tests/unit/helpers.py",
            "app/tests/integration/test/test_db.py",
            "app/tests/data/sample.csv",
            "app/tests/fixtures/payload.json",
            "app/tests/fixtures/fake_module.py",
            "app/tests/golden/expected.txt",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        assert_eq!(rels(&result, DiscoveryKind::Tests), set(&["app/tests"]));
        let tests = finding_at(&result, "app/tests");
        assert!(tests.detail.contains("5 Testdateien · pytest"), "{}", tests.detail);
        assert_eq!(tests.category, ItemCategory::Manifest, "test code is no output");
        assert_eq!(tests.evidence.context, "app");
        assert_eq!(tests_signal(&result, "app").as_deref(), Some("Tests (5 Testdateien · pytest)"));
        let detail = &finding_at(&result, "app").detail;
        assert!(detail.contains("Paketmanager: uv; Tests (5 Testdateien · pytest)"), "{detail}");
        for rel in ["app/tests/data", "app/tests/fixtures", "app/tests/golden"] {
            assert_eq!(kind_at(&result, rel), DiscoveryKind::TestData, "{rel}");
        }
        assert!(found(&result).iter().all(|(rel, _)| !rel.starts_with("app/tests/integration")), "nested root");
    }

    #[test]
    fn tests_in_dependencies_outputs_and_test_data_are_never_counted() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "web/package.json",
            "web/package-lock.json",
            "web/src/app.test.ts",
            "web/node_modules/lib/index.test.js",
            "web/node_modules/lib/test/a.js",
            "web/dist/app.test.js",
            "web/coverage/lcov-report/x.test.js",
            "web/vendor/legacy/test/b_test.go",
            "web/testdata/case/c_test.go",
            "web/__fixtures__/project/test/d.test.js",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        assert_eq!(tests_signal(&result, "web").as_deref(), Some("Tests (1 Testdatei)"));
        assert!(rels(&result, DiscoveryKind::Tests).is_empty(), "{:?}", found(&result));
    }

    #[test]
    fn a_dotnet_test_project_is_named_as_such() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "App/App.csproj",
            "App/Calculator.cs",
            "App/Latest.cs",
            "App.Tests/App.Tests.csproj",
            "App.Tests/CalculatorTests.cs",
            "App.Tests/Edit.cs",
            "App.Tests/xunit.runner.json",
            "Billing/BillingUnitTests.fsproj",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        let tests = signals(&result, "App.Tests");
        assert!(tests.contains(&"Testprojekt".to_string()), "{tests:?}");
        assert!(tests.contains(&"Tests (1 Testdatei · xUnit)".to_string()), "{tests:?}");
        assert!(signals(&result, "Billing").contains(&"Testprojekt".to_string()));
        let app = signals(&result, "App");
        assert!(!app.contains(&"Testprojekt".to_string()) && tests_signal(&result, "App").is_none(), "{app:?}");
    }

    #[test]
    fn a_test_folder_with_its_own_build_file_stays_a_test_folder() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "lib/CMakeLists.txt",
            "lib/.git/HEAD",
            "lib/src/parser.c",
            "lib/tests/CMakeLists.txt",
            "lib/tests/test_parser.c",
            "lib/tests/helpers.h",
            "lib/integration-tests/docker-compose.yml",
            "lib/integration-tests/requirements.txt",
            "lib/integration-tests/test_flow.py",
            "lib/web/tsconfig.json",
            "lib/web/main.ts",
            "lib/e2e/package.json",
            "lib/e2e/specs/login.spec.ts",
            "standalone/tsconfig.json",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        assert_eq!(kind_at(&result, "lib/tests"), DiscoveryKind::Tests);
        assert!(finding_at(&result, "lib/tests").detail.contains("2 Testdateien"));
        assert_eq!(kind_at(&result, "lib/integration-tests"), DiscoveryKind::Tests, "weak markers only");
        assert!(found(&result).iter().all(|(rel, _)| rel != "lib/web"), "a part's tsconfig.json is no project");
        assert_eq!(kind_at(&result, "lib/e2e"), DiscoveryKind::Project, "a test package with its own manifest");
        assert_eq!(tests_signal(&result, "lib/e2e").as_deref(), Some("Tests (1 Testdatei)"));
        assert_eq!(kind_at(&result, "standalone"), DiscoveryKind::Project);
        assert_eq!(tests_signal(&result, "lib").as_deref(), Some("Tests (3 Testdateien)"), "two test folders");
    }

    #[test]
    fn test_roots_need_a_development_context() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "photos/test/holiday.jpg",
            "docs/specs/requirements.pdf",
            "qa/tests/test_login.py",
            "qa/tests/test_logout.py",
            "notes/testing/plan.txt",
            "notes/testing/check.py",
            "app/package.json",
            "app/testing/smoke.js",
            "suite/spec/login.feature",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        assert_eq!(
            rels(&result, DiscoveryKind::Tests),
            set(&["app/testing", "qa/tests", "suite/spec"]),
            "{:?}",
            found(&result)
        );
        assert!(finding_at(&result, "suite/spec").detail.contains("1 Testdatei · Gherkin"));
        assert!(finding_at(&result, "qa/tests").detail.contains("2 Testdateien"));
    }

    #[test]
    fn test_data_and_test_outputs_by_name() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in [
            "app/package.json",
            "app/__snapshots__/a.snap",
            "app/cassettes/login.yml",
            "app/test-fixtures/a.json",
            "app/test_resources/b.bin",
            "app/tests/golden/a.png",
            "app/tests/snapshots/c.txt",
            "app/tests/unit/data/d.json",
            "app/tests/unit/golden/GoldenMasterTest.java",
            "app/tests/data2/CACHEDIR.TAG",
            "app/tests/resources/CACHEDIR.TAG",
            "app/tests/resources/pkg/package.json",
            "app/golden/b.png",
            "app/TestResults/run.trx",
            "app/allure-results/1-result.json",
            "app/blob-report/report-1.zip",
            "app/mutants.out/outcomes.json",
            "app/.stryker-tmp/sandbox-1/package.json",
            "app/test-reports/junit.xml",
            "app/surefire-reports/TEST-demo.xml",
            "app/.hypothesis/examples/x",
            "app/.phpunit.cache/test-results",
        ] {
            put(root, rel);
        }
        let result = search(root, false);
        assert_eq!(
            rels(&result, DiscoveryKind::TestData),
            set(&[
                "app/__snapshots__",
                "app/cassettes",
                "app/test-fixtures",
                "app/test_resources",
                "app/tests/golden",
                "app/tests/snapshots",
            ])
        );
        assert_eq!(
            rels(&result, DiscoveryKind::TestOutput),
            set(&[
                "app/.stryker-tmp",
                "app/TestResults",
                "app/allure-results",
                "app/blob-report",
                "app/mutants.out",
                "app/surefire-reports",
                "app/test-reports",
            ])
        );
        assert_eq!(
            rels(&result, DiscoveryKind::Cache),
            set(&["app/.hypothesis", "app/.phpunit.cache", "app/tests/data2", "app/tests/resources"]),
            "a cache marker wins over a data folder name and stays a leaf"
        );
        let unexpected = ["app/golden", "app/tests/unit/data", "app/tests/unit/golden", "app/tests/resources/pkg"];
        assert!(found(&result).iter().all(|(rel, _)| !unexpected.contains(&rel.as_str())));
        assert_eq!(rels(&result, DiscoveryKind::Project), set(&["app"]), "nothing inside an output is a project");
    }

    #[test]
    fn test_file_conventions() {
        let test = |name: &str| test_file(name, &name.to_ascii_lowercase());
        for name in [
            "test_parser.py",
            "parser_test.py",
            "tests.py",
            "app.test.ts",
            "view.spec.tsx",
            "lib.test.mjs",
            "app.e2e-spec.ts",
            "lib_test.rs",
            "lib_tests.rs",
            "FooTest.java",
            "FooTests.kt",
            "UserServiceIT.java",
            "CartSpec.scala",
            "RouteSpec.groovy",
            "CalculatorTests.cs",
            "ParserTest.fs",
            "ModelTests.vb",
            "model_test.rb",
            "UserTest.php",
            "parser_test.dart",
            "AppTests.swift",
            "parser_test.cpp",
            "parser_unittest.cc",
            "test_main.c",
            "runtests.jl",
            "TEST_PARSER.PY",
        ] {
            assert_eq!(test(name), Some(0), "{name}");
        }
        for (name, label) in [
            ("conftest.py", "pytest"),
            ("store_test.go", "Go test"),
            ("login.cy.ts", "Cypress"),
            ("model_spec.rb", "RSpec"),
            ("parser_test.exs", "ExUnit"),
            ("http_SUITE.erl", "Common Test"),
            ("ParserSpec.hs", "Hspec"),
            ("Module.Tests.ps1", "Pester"),
            ("install.bats", "Bats"),
            ("parser_spec.lua", "busted"),
            ("test-api.R", "testthat"),
            ("test_utils.r", "testthat"),
            ("login.feature", "Gherkin"),
            ("smoke.robot", "Robot Framework"),
        ] {
            assert_ne!(framework(label), 0, "{label}");
            assert_eq!(test(name), Some(framework(label)), "{name}");
        }
        for name in [
            "test.py",
            "contest.py",
            "latest.py",
            "Edit.java",
            "SPLIT.java",
            "Latest.cs",
            "Contest.java",
            "Test.java",
            "IT.java",
            "testing.ts",
            "latest.ts",
            "attest.go",
            "main.go",
            "spec.rb",
            "test_main.cc",
            "build.gradle.kts",
            "README.md",
            "noextension",
        ] {
            assert_eq!(test(name), None, "{name}");
        }
    }

    #[test]
    fn name_tables_stay_consistent() {
        assert!(FRAMEWORKS.len() <= 64, "frameworks are bits of one u64");
        for (pattern, label) in TEST_CONFIGS {
            assert_ne!(framework(label), 0, "{label}");
            assert_eq!(pattern.to_ascii_lowercase(), *pattern);
            assert!(pattern.matches('*').count() <= 1, "{pattern}");
        }
        for (pattern, _) in MANAGERS.iter().chain(WORKSPACES) {
            assert_eq!(pattern.to_ascii_lowercase(), *pattern);
            assert!(pattern.matches('*').count() <= 1, "{pattern}");
        }
        for list in [MANIFESTS, WEAK_MARKERS, LOCKFILES, TEST_ROOTS, TEST_DATA_FOLDERS, DATA_IN_TESTS, TEST_OUTPUTS] {
            assert!(list.iter().all(|name| name.to_ascii_lowercase() == *name), "{list:?}");
        }
        assert_eq!(config_frameworks("jest.config.ts"), framework("Jest"));
        assert_eq!(config_frameworks("vitest.workspace.mjs"), framework("Vitest"));
        assert_eq!(config_frameworks(".mocharc.yml"), framework("Mocha"));
        assert_eq!(config_frameworks("phpunit.xml.dist"), framework("PHPUnit"));
        assert_eq!(config_frameworks("local.runsettings"), framework("VSTest"));
        assert_eq!(config_frameworks("conftest.py"), framework("pytest"));
        assert_eq!(config_frameworks("jest.setup.ts"), 0);
        assert!(matches_pattern("requirements-dev.txt", "requirements*.txt"));
        assert!(!matches_pattern("requirements.in", "requirements*.txt"));
        assert!(test_project_file("App.IntegrationTests.csproj") && test_project_file("AppTests.vbproj"));
        assert!(!test_project_file("Latest.csproj") && !test_project_file("Contest.Web.csproj"));
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
        put(temp.path(), "app/.terraform/modules/vpc/main.tf");
        put(temp.path(), "app/deps/plug/mix.exs");
        let found = found(&search(temp.path(), false));
        for rel in ["app/vendor", "app/.terraform", "app/deps"] {
            assert!(found.contains(&(rel.into(), DiscoveryKind::Dependencies)), "{rel}: {found:?}");
            assert!(found.iter().all(|(inside, _)| !inside.starts_with(&format!("{rel}/"))), "{found:?}");
        }
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

    fn project_scan(root: &Path) -> GenerationResult {
        let handle = Rc::new(DirHandle::open_root(root, false).unwrap());
        super::super::scanner::scan_project(handle, &mut Budget::new(ScanLimits::default()))
    }

    #[test]
    fn the_project_scanner_stops_at_leaves_but_walks_its_root() {
        let temp = tempfile::tempdir().unwrap();
        // The approved root itself may carry an output's name.
        let root = temp.path().join("build");
        write(&root, "Cargo.toml", "[package]\nname = 'demo'\nversion = '0.1.0'\n\n[dependencies]\nserde = '1'\n");
        write(&root, "web/package.json", r#"{"dependencies":{"react":"^18.2.0"}}"#);
        write(&root, "web/src/app.test.ts", "never read");
        write(&root, "src/test/java/demo/AppTest.java", "never read");
        for (rel, name) in [
            ("dist/package.json", "never-dist"),
            ("vendor/lib/package.json", "never-vendor"),
            ("third_party/x/package.json", "never-third-party"),
            (".venv/lib/package.json", "never-venv"),
            ("out/package.json", "never-out"),
            ("coverage/package.json", "never-coverage"),
        ] {
            write(&root, rel, &format!(r#"{{"dependencies":{{"{name}":"1.0.0"}}}}"#));
        }
        let result = project_scan(&root);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(result.items.iter().any(|i| i.ecosystem == "cargo" && i.name == "serde"), "the root is walked");
        assert!(result.items.iter().any(|i| i.ecosystem == "npm" && i.name == "react"));
        let never: Vec<&str> =
            result.items.iter().filter(|i| i.name.starts_with("never")).map(|i| i.name.as_str()).collect();
        assert!(never.is_empty(), "nothing inside a leaf is parsed: {never:?}");
        for (rel, kind) in [
            ("dist", DiscoveryKind::BuildOutput),
            ("vendor", DiscoveryKind::Dependencies),
            ("third_party", DiscoveryKind::Dependencies),
            (".venv", DiscoveryKind::Environment),
            ("out", DiscoveryKind::BuildOutput),
            ("coverage", DiscoveryKind::TestOutput),
        ] {
            assert_eq!(kind_at(&result, rel), kind, "{rel}");
        }
        assert!(result.items.iter().all(|i| !i.rel_path.starts_with("dist/") && !i.rel_path.starts_with("vendor/")));
        // The census runs in the project scanner too.
        assert_eq!(kind_at(&result, "src/test"), DiscoveryKind::Tests);
        assert!(finding_at(&result, "src/test").detail.contains("1 Testdatei"));
        assert_eq!(tests_signal(&result, "").as_deref(), Some("Tests (1 Testdatei)"));
        assert!(!finding_at(&result, "").detail.contains("Teilprojekt"), "{}", finding_at(&result, "").detail);
        assert_eq!(tests_signal(&result, "web").as_deref(), Some("Tests (1 Testdatei)"));
        assert_eq!(finding_at(&result, "web").evidence.context, "", "a member points to the root project");
        assert!(finding_at(&result, "web").detail.contains("Teilprojekt von der Suchwurzel"));
    }

    #[test]
    fn adapter_claims_come_before_leaves() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("svc");
        write(&root, "go.mod", "module example.com/svc\n\ngo 1.22\n\nrequire github.com/foo/bar v1.2.3\n");
        write(&root, "vendor/modules.txt", "# github.com/foo/bar v1.2.3\n## explicit\n");
        write(&root, "vendor/github.com/foo/bar/bar_test.go", "never read");
        let result = project_scan(&root);
        let installed = result.items.iter().any(|i| {
            i.ecosystem == "go"
                && i.name == "github.com/foo/bar"
                && i.install_state == Some(crate::domain::InstallState::Installed)
        });
        assert!(installed, "{:?}", result.items);
        assert_eq!(kind_at(&result, "vendor"), DiscoveryKind::Dependencies);
        assert!(tests_signal(&result, "").is_none(), "nothing in a claimed folder counts");
    }
}
