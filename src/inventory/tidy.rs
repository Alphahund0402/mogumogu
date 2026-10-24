//! Bounded tidy-up walk over one approved project (see `domain::tidy`). Sizes
//! come from directory listings below the identity-checked root; no file is
//! opened, links and cloud placeholders are never followed, and an incomplete
//! walk leaves the total unknown.
use crate::domain::{TidyClass, TidyItem};
use crate::fsread::{self, Budget, ScanControl, ScopeRoot};
use crate::limits::ScanLimits;
use crate::platform::{DirEntryInfo, DirHandle, FileIdentity};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

/// Entries listed per project; the class totals include all of them.
const ITEMS_KEPT: usize = 60;
/// A single file this large is worth a look wherever it lies.
const LARGE_FILE: u64 = 100 << 20;
/// Log files below this size are not listed on their own.
const LARGE_LOG: u64 = 10 << 20;

const NODE_LOCKS: &[&str] =
    &["package-lock.json", "npm-shrinkwrap.json", "yarn.lock", "pnpm-lock.yaml", "bun.lock", "bun.lockb"];
const PYTHON_LOCKS: &[&str] = &["uv.lock", "poetry.lock", "pipfile.lock", "pdm.lock"];
const BUILD_MANIFESTS: &[&str] = &[
    "package.json",
    "pyproject.toml",
    "setup.py",
    "build.gradle",
    "build.gradle.kts",
    "cmakelists.txt",
    "meson.build",
    "deno.json",
];
/// Traces a tool leaves in the folder it made. A name alone can also be a
/// hand-made folder (`coverage/` as a Python package, `build/` with icons):
/// without such a trace the folder needs a look. `*` ends a prefix.
const PYTHON_ENV: &[&str] = &["pyvenv.cfg"];
const CARGO_TARGET: &[&str] = &["cachedir.tag", ".rustc_info.json", "debug", "release"];
const MAVEN_TARGET: &[&str] = &["maven-status", "maven-archiver", "classes"];
const DOTNET_BIN: &[&str] = &["debug", "release"];
const DOTNET_OBJ: &[&str] = &["project.assets.json"];
const GRADLE_BUILD: &[&str] = &["tmp", "classes", "libs", "intermediates", "generated", "reports"];
const CMAKE_BUILD: &[&str] = &["cmakecache.txt"];
const PYTHON_BUILD: &[&str] = &["lib.*", "bdist.*", "temp.*"];
const CACHE_TAG: &[&str] = &["cachedir.tag"];
const GRADLE_CACHE: &[&str] = &["buildoutputcleanup", "file-system.probe", "vcs-1"];
const TURBO_CACHE: &[&str] = &["cache", "daemon", "cookies"];
const PARCEL_CACHE: &[&str] = &["data.mdb", "lock.mdb"];
const ANGULAR_CACHE: &[&str] = &["cache"];
const COVERAGE: &[&str] = &[
    "lcov.info",
    "lcov-report",
    "coverage-final.json",
    "coverage-summary.json",
    "clover.xml",
    "cobertura-coverage.xml",
    "coverage.xml",
];
const HTML_COVERAGE: &[&str] = &["status.json", "coverage_html*"];
const PLAYWRIGHT_RESULTS: &[&str] = &[".last-run.json"];
const TESTNG_OUTPUT: &[&str] = &["testng-results.xml"];
const PLAYWRIGHT_REPORT: &[&str] = &["data"];
const NYC_OUTPUT: &[&str] = &["processinfo"];
const DOTNET_PROJECTS: &[&str] = &[".csproj", ".fsproj", ".vbproj", ".sln", ".slnx"];

#[derive(Debug)]
pub struct TidyWalk {
    pub total: Option<u64>,
    pub regenerable: u64,
    pub review: u64,
    pub vcs: u64,
    /// Bytes in test data folders, part of `review`.
    pub test_data: u64,
    pub items: Vec<TidyItem>,
    /// Whether each item, by index during the walk, holds test data.
    test_items: Vec<bool>,
    pub last_change: Option<i64>,
    pub reason: Option<String>,
}

use crate::domain::TEST_DATA_LABEL as TEST_DATA;
/// Folders that hold data for tests wherever they are.
const TEST_DATA_FOLDERS: &[&str] =
    &["testdata", "test-data", "test_data", "testfiles", "test-files", "fixtures", "__fixtures__", "__snapshots__"];
/// Data folders that count as test data directly inside a test folder.
const TEST_FOLDERS: &[&str] = &["test", "tests", "spec", "__tests__"];
const DATA_IN_TESTS: &[&str] = &["data", "resources", "samples", "assets"];

/// Whether `name` inside the folder `parent` (relative path) holds test data.
fn test_data_folder(parent: &str, name: &str) -> bool {
    let parent = parent.rsplit('/').next().unwrap_or_default().to_ascii_lowercase();
    TEST_FOLDERS.contains(&parent.as_str()) && DATA_IN_TESTS.contains(&name)
}

#[derive(Clone, Copy)]
enum Bucket {
    Item(usize),
    Vcs,
}

/// What a folder name, read together with its siblings, says.
enum Verdict {
    Item(TidyClass, &'static str, String),
    /// Recoverable only with one of these traces inside; otherwise "Prüfen".
    Signed(&'static str, String, &'static [&'static str]),
    Vcs,
}

struct Frame {
    parent: Rc<DirHandle>,
    name: Option<OsString>,
    rel: String,
    depth: u32,
    bucket: Option<Bucket>,
    /// Traces this item's own listing must show to count as recoverable.
    signature: Option<&'static [&'static str]>,
}

/// The first trace of `signature` among the names of a folder.
fn trace(entries: &[DirEntryInfo], signature: &[&'static str]) -> Option<&'static str> {
    signature.iter().copied().find(|wanted| {
        entries.iter().filter(|e| !e.is_reparse()).filter_map(|e| e.name_str()).any(|name| {
            let name = name.to_ascii_lowercase();
            match wanted.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => name == *wanted,
            }
        })
    })
}

fn signed_evidence(evidence: &str, found: &str) -> String {
    format!("{evidence} · {} darin", found.trim_end_matches('*'))
}

pub fn tidy(path: &Path, identity: Option<FileIdentity>, control: Option<Arc<ScanControl>>) -> TidyWalk {
    let limits =
        ScanLimits { max_depth: 64, max_entries: 2_000_000, max_total_bytes: 0, time_budget: Duration::from_secs(120) };
    let mut walk = TidyWalk {
        total: None,
        regenerable: 0,
        review: 0,
        vcs: 0,
        test_data: 0,
        items: Vec::new(),
        test_items: Vec::new(),
        last_change: None,
        reason: None,
    };
    let root = match ScopeRoot::open(path, identity) {
        Ok(root) => Rc::new(root.into_handle()),
        Err(e) => {
            walk.reason = Some(e.to_string());
            return walk;
        }
    };
    let mut budget = Budget::with_control(limits, control);
    let mut total = 0_u64;
    let mut complete = true;
    let mut stack =
        vec![Frame { parent: root, name: None, rel: String::new(), depth: 0, bucket: None, signature: None }];
    while let Some(frame) = stack.pop() {
        if budget.exhausted() {
            break;
        }
        let dir = match frame.name {
            None => frame.parent,
            Some(name) => match frame.parent.open_dir(OsStr::new(&name)) {
                Ok(child) => Rc::new(child),
                Err(e) => {
                    walk.reason = Some(format!("Unterordner nicht lesbar: {e}"));
                    complete = false;
                    continue;
                }
            },
        };
        let entries = match fsread::list(&dir, &mut budget) {
            Ok((entries, _)) => entries,
            Err(e) => {
                walk.reason = Some(format!("Ordner nicht lesbar: {e}"));
                complete = false;
                continue;
            }
        };
        // A signed item starts as "Prüfen"; its own listing may prove it.
        if let (Some(signature), Some(Bucket::Item(index))) = (frame.signature, frame.bucket)
            && let Some(found) = trace(&entries, signature)
        {
            let item = &mut walk.items[index];
            item.class = TidyClass::Regenerable;
            item.evidence = signed_evidence(&item.evidence, found);
        }
        let here = Names::of(&entries);
        for entry in &entries {
            if !budget.entry() {
                break;
            }
            if entry.is_reparse() {
                continue;
            }
            if entry.is_placeholder() {
                walk.reason = Some("Cloud-Platzhalter enthalten – lokale Größe unbekannt".into());
                complete = false;
                continue;
            }
            let Some(name) = entry.name_str() else {
                complete = false;
                continue;
            };
            let rel = fsread::join(&frame.rel, name);
            if !entry.is_dir() {
                total = total.saturating_add(entry.size);
                match frame.bucket {
                    Some(bucket) => walk.add(bucket, entry.size, entry.modified),
                    None => walk.file(&rel, name, entry),
                }
                continue;
            }
            if frame.depth + 1 > limits.max_depth {
                complete = false;
                continue;
            }
            let mut signature = None;
            let lower = name.to_ascii_lowercase();
            let found = if test_data_folder(&frame.rel, &lower) {
                Some(Verdict::Item(
                    TidyClass::Review,
                    TEST_DATA,
                    "Daten in einem Testordner; kann selbst gepflegt sein".into(),
                ))
            } else {
                verdict(&lower, &here)
            };
            let bucket = frame.bucket.or_else(|| match found {
                Some(Verdict::Vcs) => Some(Bucket::Vcs),
                Some(Verdict::Item(class, label, evidence)) => Some(walk.open(rel.clone(), class, label, evidence)),
                Some(Verdict::Signed(label, evidence, traces)) => {
                    signature = Some(traces);
                    Some(walk.open(rel.clone(), TidyClass::Review, label, evidence))
                }
                None => None,
            });
            stack.push(Frame {
                parent: Rc::clone(&dir),
                name: Some(entry.name.clone()),
                rel,
                depth: frame.depth + 1,
                bucket,
                signature,
            });
        }
    }
    if let Some(reason) = budget.limit_reason.take() {
        walk.reason = Some(reason);
        complete = false;
    }
    walk.total = complete.then_some(total);
    walk.items.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.rel_path.cmp(&b.rel_path)));
    walk.items.truncate(ITEMS_KEPT);
    walk
}

impl TidyWalk {
    fn open(&mut self, rel_path: String, class: TidyClass, label: &str, evidence: String) -> Bucket {
        self.items.push(TidyItem { rel_path, class, label: label.into(), bytes: 0, evidence, modified_at: None });
        self.test_items.push(label == TEST_DATA);
        Bucket::Item(self.items.len() - 1)
    }

    fn add(&mut self, bucket: Bucket, bytes: u64, modified: i64) {
        match bucket {
            Bucket::Vcs => self.vcs = self.vcs.saturating_add(bytes),
            Bucket::Item(index) => {
                if self.test_items[index] {
                    self.test_data = self.test_data.saturating_add(bytes);
                }
                let item = &mut self.items[index];
                item.bytes = item.bytes.saturating_add(bytes);
                item.modified_at = item.modified_at.max(Some(modified));
                match item.class {
                    TidyClass::Regenerable => self.regenerable = self.regenerable.saturating_add(bytes),
                    TidyClass::Review => {
                        self.review = self.review.saturating_add(bytes);
                        self.touch(modified);
                    }
                }
            }
        }
    }

    /// A file in a kept folder: listed if it deserves a look, otherwise it
    /// only counts towards the project's last change.
    fn file(&mut self, rel: &str, name: &str, entry: &DirEntryInfo) {
        let lower = name.to_ascii_lowercase();
        let review = if [".sqlite", ".sqlite3", ".db", ".duckdb"].iter().any(|e| lower.ends_with(e)) {
            Some(("Datenbank", "Datenbankendung; Inhalt und Zweck unbekannt"))
        } else if lower.ends_with(".log") && entry.size >= LARGE_LOG {
            Some(("Logdatei", "Große Logdatei"))
        } else if entry.size >= LARGE_FILE {
            Some(("Große Datei", "Über 100 MB; Zweck unbekannt"))
        } else {
            None
        };
        match review {
            Some((label, evidence)) => {
                let bucket = self.open(rel.to_string(), TidyClass::Review, label, evidence.into());
                self.add(bucket, entry.size, entry.modified);
            }
            None => self.touch(entry.modified),
        }
    }

    fn touch(&mut self, modified: i64) {
        if modified > 0 {
            self.last_change = self.last_change.max(Some(modified));
        }
    }
}

/// Lower-case names of one directory, to read a folder with its siblings.
struct Names(Vec<(String, bool)>);

impl Names {
    fn of(entries: &[DirEntryInfo]) -> Self {
        Self(
            entries
                .iter()
                .filter(|e| !e.is_reparse())
                .filter_map(|e| e.name_str().map(|n| (n.to_ascii_lowercase(), e.is_dir())))
                .collect(),
        )
    }

    /// The first of these file names present here.
    fn file<'a>(&self, names: &[&'a str]) -> Option<&'a str> {
        names.iter().copied().find(|n| self.0.iter().any(|(name, dir)| !dir && name == n))
    }

    fn file_ending(&self, endings: &[&str]) -> bool {
        self.0.iter().any(|(name, dir)| !dir && endings.iter().any(|e| name.ends_with(e)))
    }

    fn folder(&self, wanted: &str) -> bool {
        self.0.iter().any(|(name, dir)| *dir && name == wanted)
    }
}

/// Reads the siblings of one folder, and for a signed rule the folder
/// itself, again and returns its label and evidence while it still counts as
/// recoverable. A cleanup asks this right before it acts; an unreadable or
/// truncated listing never adds evidence.
pub fn recoverable(parent: &DirHandle, name: &str) -> Option<(&'static str, String)> {
    let limits =
        ScanLimits { max_depth: 1, max_entries: 100_000, max_total_bytes: 0, time_budget: Duration::from_secs(10) };
    let listed = |dir: &DirHandle| match fsread::list(dir, &mut Budget::new(limits)) {
        Ok((entries, false)) => Some(entries),
        _ => None,
    };
    match verdict(&name.to_ascii_lowercase(), &Names::of(&listed(parent)?))? {
        Verdict::Item(TidyClass::Regenerable, label, evidence) => Some((label, evidence)),
        Verdict::Signed(label, evidence, signature) => {
            let inside = listed(&parent.open_dir(OsStr::new(name)).ok()?)?;
            trace(&inside, signature).map(|found| (label, signed_evidence(&evidence, found)))
        }
        _ => None,
    }
}

/// Outputs and environments need the file that rebuilds them next to them;
/// where a name alone could also mean a hand-made folder, the tool's trace
/// inside is required too (see [`Verdict::Signed`]).
fn verdict(name: &str, here: &Names) -> Option<Verdict> {
    use TidyClass::Review;
    let beside = |file: &str| format!("{file} daneben");
    let regenerable = |label, evidence| Verdict::Item(TidyClass::Regenerable, label, evidence);
    Some(match name {
        ".git" | ".hg" | ".svn" => Verdict::Vcs,
        "node_modules" => match here.file(NODE_LOCKS) {
            Some(lock) => regenerable("Abhängigkeiten (node_modules)", beside(lock)),
            None => Verdict::Item(Review, "Abhängigkeiten (node_modules)", "Kein Lockfile daneben".into()),
        },
        ".venv" | "venv" => match here.file(PYTHON_LOCKS) {
            Some(lock) => Verdict::Signed("Python-Umgebung", beside(lock), PYTHON_ENV),
            None => Verdict::Item(Review, "Python-Umgebung", "Kein Lockfile daneben".into()),
        },
        ".tox" | ".nox" => match here.file(&["tox.ini", "noxfile.py"]) {
            Some(file) => regenerable("Testumgebungen", beside(file)),
            None => Verdict::Item(Review, "Testumgebungen", "Keine tox-/nox-Konfiguration daneben".into()),
        },
        "target" => match (here.file(&["cargo.toml"]), here.file(&["pom.xml"])) {
            (Some(file), _) => Verdict::Signed("Build-Ausgabe", beside(file), CARGO_TARGET),
            (None, Some(file)) => Verdict::Signed("Build-Ausgabe", beside(file), MAVEN_TARGET),
            _ => Verdict::Item(Review, "Ordner target", "Kein Build-Manifest daneben".into()),
        },
        // `bin` alone is a common name for scripts: it needs `obj` beside it.
        "bin" if here.file_ending(DOTNET_PROJECTS) && here.folder("obj") => {
            Verdict::Signed("Build-Ausgabe (.NET)", ".NET-Projekt und obj daneben".into(), DOTNET_BIN)
        }
        "obj" if here.file_ending(DOTNET_PROJECTS) => {
            Verdict::Signed("Build-Ausgabe (.NET)", ".NET-Projekt daneben".into(), DOTNET_OBJ)
        }
        // `build/` beside package.json often holds hand-made resources
        // (electron-builder icons, old Vue build scripts): never on its own.
        "build" => match (
            here.file(&["build.gradle", "build.gradle.kts"]),
            here.file(&["cmakelists.txt"]),
            here.file(&["setup.py", "pyproject.toml"]),
        ) {
            (Some(file), ..) => Verdict::Signed("Build-Ausgabe", beside(file), GRADLE_BUILD),
            (None, Some(file), _) => Verdict::Signed("Build-Ausgabe", beside(file), CMAKE_BUILD),
            (None, None, Some(file)) => Verdict::Signed("Build-Ausgabe", beside(file), PYTHON_BUILD),
            _ => Verdict::Item(Review, "Mögliche Build-Ausgabe", "Kann selbst gepflegte Dateien enthalten".into()),
        },
        "dist" | "out" | ".next" | ".nuxt" | ".svelte-kit" | ".output" => match here.file(BUILD_MANIFESTS) {
            Some(file) => regenerable("Build-Ausgabe", beside(file)),
            None => Verdict::Item(Review, "Mögliche Build-Ausgabe", "Kein Build-Manifest daneben".into()),
        },
        "__pycache__" => regenerable("Cache", "Python-Bytecode; entsteht beim nächsten Lauf neu".into()),
        ".pytest_cache" | ".mypy_cache" | ".ruff_cache" => {
            Verdict::Signed("Cache", "Werkzeugcache; entsteht beim nächsten Lauf neu".into(), CACHE_TAG)
        }
        ".gradle" => Verdict::Signed("Cache", "Gradle-Cache; entsteht beim nächsten Lauf neu".into(), GRADLE_CACHE),
        ".turbo" => Verdict::Signed("Cache", "Turborepo-Cache; entsteht beim nächsten Lauf neu".into(), TURBO_CACHE),
        ".parcel-cache" => {
            Verdict::Signed("Cache", "Parcel-Cache; entsteht beim nächsten Lauf neu".into(), PARCEL_CACHE)
        }
        ".angular" => Verdict::Signed("Cache", "Angular-Cache; entsteht beim nächsten Lauf neu".into(), ANGULAR_CACHE),
        ".cache" => Verdict::Item(Review, "Cache-Ordner", "Allgemeiner Name; Werkzeug und Inhalt unbekannt".into()),
        "coverage" => Verdict::Signed("Test-Ausgabe", "Abdeckungsbericht".into(), COVERAGE),
        "htmlcov" => Verdict::Signed("Test-Ausgabe", "Abdeckungsbericht (coverage.py)".into(), HTML_COVERAGE),
        "test-results" => Verdict::Signed("Test-Ausgabe", "Playwright-Ergebnisse".into(), PLAYWRIGHT_RESULTS),
        "test-output" => Verdict::Signed("Test-Ausgabe", "TestNG-Ausgabe".into(), TESTNG_OUTPUT),
        "playwright-report" => Verdict::Signed("Test-Ausgabe", "Playwright-Bericht".into(), PLAYWRIGHT_REPORT),
        ".nyc_output" => Verdict::Signed("Test-Ausgabe", "nyc-Rohdaten".into(), NYC_OUTPUT),
        _ if TEST_DATA_FOLDERS.contains(&name) => {
            Verdict::Item(Review, TEST_DATA, "Kann selbst gepflegte Daten enthalten".into())
        }
        "logs" | "log" => Verdict::Item(Review, "Logs", "Protokolle; Aufbewahrung unbekannt".into()),
        _ => return None,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, bytes: u64) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(&path).unwrap().set_len(bytes).unwrap();
    }

    #[test]
    fn classes_follow_names_and_the_files_beside_them() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "Cargo.toml", 10);
        put(root, "Cargo.lock", 10);
        put(root, "src/main.rs", 100);
        put(root, "target/debug/app.exe", 5_000);
        put(root, ".git/objects/pack/a.pack", 700);
        put(root, "web/package.json", 10);
        put(root, "web/pnpm-lock.yaml", 10);
        put(root, "web/node_modules/dep/index.js", 3_000);
        put(root, "web/dist/app.js", 400);
        put(root, "tools/node_modules/x/index.js", 200);
        put(root, "tools/bin/run.sh", 20);
        put(root, "py/.venv/lib/site.py", 900);
        put(root, "py/__pycache__/m.pyc", 30);
        put(root, "tests/fixtures/sample.json", 600);
        put(root, "data/local.sqlite", 800);
        put(root, "media/video.mp4", 120 << 20);
        let walk = tidy(root, None, None);
        assert!(walk.reason.is_none(), "{:?}", walk.reason);
        let item = |rel: &str| {
            walk.items.iter().find(|i| i.rel_path == rel).unwrap_or_else(|| panic!("{rel}: {:#?}", walk.items))
        };
        assert_eq!((item("target").class, item("target").bytes), (TidyClass::Regenerable, 5_000));
        assert!(item("target").evidence.contains("cargo.toml"));
        assert_eq!(item("web/node_modules").class, TidyClass::Regenerable);
        assert!(item("web/node_modules").evidence.contains("pnpm-lock.yaml"));
        assert_eq!(item("web/dist").class, TidyClass::Regenerable);
        assert_eq!(item("tools/node_modules").class, TidyClass::Review, "no lockfile beside");
        assert!(walk.items.iter().all(|i| i.rel_path != "tools/bin"), "bin without a .NET project is kept");
        assert_eq!(item("py/.venv").class, TidyClass::Review, "an environment without lockfile needs a look");
        assert_eq!(item("py/__pycache__").class, TidyClass::Regenerable);
        assert_eq!(item("tests/fixtures").class, TidyClass::Review);
        assert_eq!(item("data/local.sqlite").label, "Datenbank");
        assert_eq!(item("media/video.mp4").label, "Große Datei");
        assert_eq!(walk.vcs, 700);
        assert_eq!(walk.regenerable, 5_000 + 3_000 + 400 + 30);
        assert_eq!(walk.review, 200 + 900 + 600 + 800 + (120 << 20));
        let all = 10 + 10 + 100 + 5_000 + 700 + 10 + 10 + 3_000 + 400 + 200 + 20 + 900 + 30 + 600 + 800 + (120 << 20);
        assert_eq!(walk.total, Some(all));
        assert!(walk.last_change.is_some());
        assert_eq!(walk.items[0].rel_path, "media/video.mp4", "largest first");
    }

    #[test]
    fn names_alone_do_not_make_hand_made_folders_recoverable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "pyproject.toml", 1);
        put(root, "coverage/__init__.py", 10);
        put(root, "app/package.json", 1);
        put(root, "app/build/icon.ico", 10);
        put(root, ".cache/state.sqlite", 10);
        put(root, "rs/Cargo.toml", 1);
        put(root, "rs/target/notes.txt", 10);
        put(root, "rs2/Cargo.toml", 1);
        put(root, "rs2/target/CACHEDIR.TAG", 10);
        put(root, "js/package.json", 1);
        put(root, "js/coverage/lcov.info", 10);
        put(root, "py/uv.lock", 1);
        put(root, "py/.venv/lib.txt", 10);
        put(root, "py2/uv.lock", 1);
        put(root, "py2/.venv/pyvenv.cfg", 10);
        put(root, "gr/build.gradle", 1);
        put(root, "gr/build/tmp/x", 10);
        put(root, "pyb/pyproject.toml", 1);
        put(root, "pyb/build/lib/handmade_helper.py", 10);
        put(root, "net/app.csproj", 1);
        put(root, "net/bin/release/deploy.ps1", 10);
        let walk = tidy(root, None, None);
        let class = |rel: &str| walk.items.iter().find(|i| i.rel_path == rel).unwrap_or_else(|| panic!("{rel}")).class;
        for review in ["coverage", "app/build", ".cache", "rs/target", "py/.venv", "pyb/build"] {
            assert_eq!(class(review), TidyClass::Review, "{review}");
        }
        for regenerable in ["rs2/target", "js/coverage", "py2/.venv", "gr/build"] {
            assert_eq!(class(regenerable), TidyClass::Regenerable, "{regenerable}");
        }
        let target = walk.items.iter().find(|i| i.rel_path == "rs2/target").unwrap();
        assert!(target.evidence.contains("cachedir.tag darin"), "{}", target.evidence);
        assert!(walk.items.iter().all(|i| i.rel_path != "net/bin"), "bin without obj stays part of the project");
        assert_eq!(walk.regenerable, 40, "only proven folders count");
    }

    #[test]
    fn test_data_are_summed_wherever_they_lie() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "src/lib.rs", 100);
        put(root, "tests/fixtures/a.json", 1_000);
        put(root, "tests/data/b.csv", 2_000);
        put(root, "web/__snapshots__/c.snap", 300);
        put(root, "data/d.csv", 50);
        put(root, "tests/unit.rs", 10);
        let walk = tidy(root, None, None);
        assert_eq!(walk.test_data, 3_300, "{:#?}", walk.items);
        assert!(walk.items.iter().all(|i| i.rel_path != "data"), "data outside a test folder is the project's own");
        assert_eq!(walk.review, 3_300);
    }

    #[test]
    fn a_replaced_root_is_not_measured() {
        let temp = tempfile::tempdir().unwrap();
        let identity = crate::platform::path_identity(temp.path()).unwrap();
        let other = tempfile::tempdir().unwrap();
        let walk = tidy(other.path(), Some(identity), None);
        assert!(walk.total.is_none() && walk.reason.is_some());
    }
}
