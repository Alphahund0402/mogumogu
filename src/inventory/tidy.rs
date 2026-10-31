//! Bounded tidy-up walk over one approved project (see `domain::tidy`). Sizes
//! come from directory listings below the identity-checked root; links and
//! cloud placeholders are never followed, and an incomplete walk leaves the
//! total unknown. The only files opened are a work tree's `.git/config` and
//! `.git/index`: a folder with versioned files belongs to the repository and
//! is no longer recoverable. Test code is the project's own: test folders
//! and test files never become items, only test data and outputs. Leftovers
//! of AI agents are recognised by name and always need a look.
use super::discovery::{DATA_IN_TESTS, GOLDEN, TEST_DATA_FOLDERS, TEST_ROOTS};
use super::git::{self, GitState};
use crate::domain::{Hold, HoldKind, TidyClass, TidyItem, holding, same_location};
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
/// Files from this size on are counted once for allocated space (hard links).
const LINKED_MIN: u64 = 64 << 10;
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
const LCOV_REPORT: &[&str] = &["sorter.js", "prettify.js", "block-navigation.js"];
const VSTEST_RESULTS: &[&str] = &["*.trx", "*.coverage"];
const BLOB_REPORT: &[&str] = &["report-*.zip", "report.zip"];
const ALLURE_RESULTS: &[&str] = &["*-result.json", "*-container.json"];
const SUREFIRE_REPORTS: &[&str] = &["test-*.xml"];
const FAILSAFE_REPORTS: &[&str] = &["failsafe-summary.xml", "test-*.xml"];
const MUTANTS_OUT: &[&str] = &["outcomes.json", "mutants.json"];
const PHPUNIT_CACHE: &[&str] = &["test-results", "code-coverage"];
const DOTNET_PROJECTS: &[&str] = &[".csproj", ".fsproj", ".vbproj", ".sln", ".slnx"];

#[derive(Debug)]
pub struct TidyWalk {
    pub total: Option<u64>,
    pub regenerable: u64,
    /// Allocated space of the recoverable items.
    pub regenerable_allocated: u64,
    pub review: u64,
    pub vcs: u64,
    /// Bytes in test data folders, part of `review`.
    pub test_data: u64,
    /// Bytes in leftovers of AI agents, part of `review`.
    pub agent: u64,
    pub items: Vec<TidyItem>,
    /// What each item holds, by index during the walk.
    kinds: Vec<Kind>,
    /// Ids of larger files already counted for allocated space.
    seen: std::collections::HashSet<[u8; 16]>,
    pub last_change: Option<i64>,
    pub reason: Option<String>,
}

use crate::domain::{AGENT_LABEL as AGENT, TEST_DATA_LABEL as TEST_DATA};

/// Evidence suffix of a recoverable folder in a work tree without versioned files.
pub const NOT_IN_GIT: &str = "nicht in Git";
/// Evidence suffix of a folder the person keeps.
#[cfg(test)]
const KEPT: &str = "von dir behalten";
/// Provenance of an agent leftover known by its name only.
const BY_NAME: &str = "nur am Namen erkannt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Other,
    TestData,
    Agent,
}

/// A folder with `.git` met outside any item. `tree` is the folder itself,
/// or why its index cannot be read (a `.git` file or link).
struct Repo {
    rel: String,
    tree: Result<Rc<DirHandle>, String>,
}

/// Whether `name` inside the folder `parent` (relative path) holds test
/// data: a data or golden folder directly in a test folder. Deeper down
/// such names can hold test code. The names are shared with the discovery.
fn test_data_folder(parent: &str, name: &str) -> bool {
    let parent = parent.rsplit('/').next().unwrap_or_default().to_ascii_lowercase();
    TEST_ROOTS.contains(&parent.as_str()) && (DATA_IN_TESTS.contains(&name) || GOLDEN.contains(&name))
}

/// Leftovers of AI agents and their tools, known by name only: always a
/// look, never recoverable. A worktree can hold unpublished commits, a
/// recording or a failed run the only trace of what happened.
fn agent_folder(parent: &str, name: &str) -> Option<&'static str> {
    let parent = parent.rsplit('/').next().unwrap_or_default().to_ascii_lowercase();
    Some(match (parent.as_str(), name) {
        (".claude", "worktrees") => {
            "Git-Arbeitskopien von Claude Code; können unveröffentlichte Commits und Änderungen enthalten"
        }
        (".serena", "cache") => "Symbolcache von Serena",
        (_, ".playwright-mcp") => "Screenshots, Mitschnitte und Konsolenprotokolle von Playwright MCP",
        (_, ".aider.tags.cache.v3" | ".aider.tags.cache.v4") => "Repo-Karten-Cache von Aider",
        (_, ".specstory") => "Gespeicherte KI-Chatverläufe (SpecStory)",
        (_, ".crush") => "Sitzungsdatenbank und Logs von Crush",
        (_, ".swarm" | ".hive-mind") => "Agentengedächtnis von claude-flow",
        _ => return None,
    })
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

/// The first trace of `signature` among the names of a folder. One `*`
/// stands for any run of characters: `lib.*`, `*.trx`, `test-*.xml`.
fn trace(entries: &[DirEntryInfo], signature: &[&'static str]) -> Option<&'static str> {
    signature.iter().copied().find(|wanted| {
        entries.iter().filter(|e| !e.is_reparse()).filter_map(|e| e.name_str()).any(|name| {
            let name = name.to_ascii_lowercase();
            match wanted.split_once('*') {
                Some((prefix, suffix)) => {
                    name.len() >= prefix.len() + suffix.len() && name.starts_with(prefix) && name.ends_with(suffix)
                }
                None => name == *wanted,
            }
        })
    })
}

fn signed_evidence(evidence: &str, found: &str) -> String {
    format!("{evidence} · {} darin", found.trim_end_matches('*'))
}

/// Walks one project. `keeps` holds the real locations of every kept folder,
/// of any project; a kept folder, one in it and one holding it are never
/// recoverable.
pub fn tidy(
    path: &Path,
    identity: Option<FileIdentity>,
    control: Option<Arc<ScanControl>>,
    holds: &[Hold],
) -> TidyWalk {
    let limits =
        ScanLimits { max_depth: 64, max_entries: 2_000_000, max_total_bytes: 0, time_budget: Duration::from_secs(120) };
    let mut walk = TidyWalk {
        total: None,
        regenerable: 0,
        regenerable_allocated: 0,
        review: 0,
        vcs: 0,
        test_data: 0,
        agent: 0,
        items: Vec::new(),
        kinds: Vec::new(),
        seen: Default::default(),
        last_change: None,
        reason: None,
    };
    let mut repos = Vec::new();
    let root = match ScopeRoot::open(path, identity) {
        Ok(root) => Rc::new(root.into_handle()),
        Err(e) => {
            walk.reason = Some(e.to_string());
            return walk;
        }
    };
    // The root's real location: long names, the drive's own spelling.
    let place = root.final_path().ok();
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
        // A work tree outside any item; its index is read after the walk.
        if frame.bucket.is_none()
            && let Some(git) = entries.iter().find(|e| e.name_str().is_some_and(|n| n.eq_ignore_ascii_case(".git")))
        {
            let tree = if git.is_reparse() {
                Err(".git ist eine Verknüpfung".to_string())
            } else if git.is_dir() {
                Ok(Rc::clone(&dir))
            } else {
                Err(git::OUTSIDE.to_string())
            };
            repos.push(Repo { rel: frame.rel.clone(), tree });
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
                    Some(bucket) => walk.add(bucket, entry),
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
            } else if let Some(evidence) = agent_folder(&frame.rel, &lower) {
                Some(Verdict::Item(TidyClass::Review, AGENT, format!("{evidence} · {BY_NAME}")))
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
    walk.settle(place.as_deref(), path, &repos, holds);
    walk.items.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.rel_path.cmp(&b.rel_path)));
    walk.items.truncate(ITEMS_KEPT);
    walk
}

/// The evidence of a recoverable folder, given what every Git work tree
/// around it says, or why it no longer counts: versioned files belong to a
/// repository, and an index that cannot be read leaves that open. All of
/// them must list nothing. The cleanup gate asks the same.
pub fn git_evidence(evidence: &str, states: &[GitState]) -> Result<String, String> {
    for state in states {
        match state {
            GitState::Tracked(0) => {}
            GitState::Tracked(1) => return Err("1 Datei darin ist in Git versioniert".into()),
            GitState::Tracked(n) => return Err(format!("{n} Dateien darin sind in Git versioniert")),
            GitState::Unknown(reason) => return Err(format!("Git-Stand unbekannt ({reason})")),
        }
    }
    Ok(if states.is_empty() { evidence.to_string() } else { format!("{evidence} · {NOT_IN_GIT}") })
}

/// Why a folder at `location` no longer counts because of a hold, if it is
/// held, lies in a held folder or holds one.
pub fn held_evidence(holds: &[Hold], location: &str) -> Option<String> {
    let hold = holding(holds, location)?;
    Some(if same_location(&hold.location, location) {
        hold.reason.clone()
    } else {
        format!("{}: {}", hold.reason, hold.location)
    })
}

/// The work trees holding `rel_path`, outermost first, with the path inside
/// each. Both come from the same walk, so the prefixes are exact.
fn work_trees<'a>(repos: &'a [Repo], rel_path: &'a str) -> impl Iterator<Item = (usize, String)> + 'a {
    repos.iter().enumerate().filter_map(move |(index, repo)| {
        let inside =
            if repo.rel.is_empty() { Some(rel_path) } else { rel_path.strip_prefix(&format!("{}/", repo.rel)) };
        inside.map(|inside| (index, inside.to_string()))
    })
}

impl TidyWalk {
    fn open(&mut self, rel_path: String, class: TidyClass, label: &str, evidence: String) -> Bucket {
        let item =
            TidyItem { rel_path, class, label: label.into(), bytes: 0, allocated: 0, evidence, modified_at: None };
        self.items.push(item);
        self.kinds.push(match label {
            TEST_DATA => Kind::TestData,
            AGENT => Kind::Agent,
            _ => Kind::Other,
        });
        Bucket::Item(self.items.len() - 1)
    }

    fn add(&mut self, bucket: Bucket, entry: &DirEntryInfo) {
        let (bytes, mut allocated, modified) = (entry.size, entry.allocated, entry.modified);
        // Hard links share a file id: their space is reserved once. Larger
        // files are where it matters (Cargo links its executables), and the
        // set stays small. Links to files outside the walk stay unknown.
        if bytes >= LINKED_MIN && entry.file_id != [0; 16] && !self.seen.insert(entry.file_id) {
            allocated = 0;
        }
        match bucket {
            Bucket::Vcs => self.vcs = self.vcs.saturating_add(bytes),
            Bucket::Item(index) => {
                match self.kinds[index] {
                    Kind::TestData => self.test_data = self.test_data.saturating_add(bytes),
                    Kind::Agent => self.agent = self.agent.saturating_add(bytes),
                    Kind::Other => {}
                }
                let item = &mut self.items[index];
                item.bytes = item.bytes.saturating_add(bytes);
                item.allocated = item.allocated.saturating_add(allocated);
                item.modified_at = item.modified_at.max(Some(modified));
                match item.class {
                    TidyClass::Regenerable => {
                        self.regenerable = self.regenerable.saturating_add(bytes);
                        self.regenerable_allocated = self.regenerable_allocated.saturating_add(allocated);
                    }
                    TidyClass::Review => {
                        self.review = self.review.saturating_add(bytes);
                        self.touch(modified);
                    }
                }
            }
        }
    }

    /// Classes after the walk: a folder that is held, or that any work tree
    /// around it versions files in, is no longer recoverable. `place` is the
    /// root's real location, `registered` its registered spelling; a hold is
    /// compared with both, so it counts from any project that reaches the
    /// folder under any spelling. A leftover of an agent that a registered
    /// run claims names that run instead of a guess from its name. Each work
    /// tree's index is read once.
    fn settle(&mut self, place: Option<&str>, registered: &Path, repos: &[Repo], holds: &[Hold]) {
        let registered = registered.to_string_lossy();
        let join = |root: &str, rel: &str| format!("{}\\{}", root.trim_end_matches('\\'), rel.replace('/', "\\"));
        let spellings = |rel: &str| {
            let mut found = vec![join(&registered, rel)];
            found.extend(place.map(|root| join(root, rel)));
            found
        };
        for index in 0..self.items.len() {
            if self.kinds[index] == Kind::Agent
                && let Some(run) = spellings(&self.items[index].rel_path)
                    .iter()
                    .find_map(|here| holding(holds, here).filter(|h| h.kind == HoldKind::Run))
            {
                let item = &mut self.items[index];
                item.evidence = item.evidence.replace(BY_NAME, &format!("registriert, {}", run.reason));
            }
        }
        let recoverable: Vec<usize> =
            (0..self.items.len()).filter(|i| self.items[*i].class == TidyClass::Regenerable).collect();
        for &index in &recoverable {
            let rel = &self.items[index].rel_path;
            let why = match spellings(rel).iter().find_map(|here| held_evidence(holds, here)) {
                Some(why) => Some(why),
                None if place.is_none() && !holds.is_empty() => {
                    Some("Ort nicht bestimmbar, gehaltene Ordner nicht prüfbar".into())
                }
                None => None,
            };
            if let Some(why) = why {
                self.demote(index, why);
            }
        }
        // A repository around the root may version files here; its index is not approved.
        let around = match place {
            Some(root) => git::above(Path::new(root)),
            None => Some("Ort des Projekts nicht bestimmbar".into()),
        };
        let recoverable: Vec<usize> =
            recoverable.into_iter().filter(|i| self.items[*i].class == TidyClass::Regenerable).collect();
        let mut states: Vec<Vec<GitState>> = vec![Vec::new(); self.items.len()];
        let mut groups: Vec<Vec<(usize, String)>> = repos.iter().map(|_| Vec::new()).collect();
        for &index in &recoverable {
            states[index].extend(around.iter().map(|reason| GitState::Unknown(reason.clone())));
            for (repo, inside) in work_trees(repos, &self.items[index].rel_path) {
                groups[repo].push((index, inside));
            }
        }
        for (repo, group) in repos.iter().zip(groups).filter(|(_, group)| !group.is_empty()) {
            let inside: Vec<String> = group.iter().map(|(_, rel)| rel.clone()).collect();
            let answers = match &repo.tree {
                Ok(tree) => match git::open(tree) {
                    Some(Ok(dir)) => git::tracked(&dir, &inside),
                    Some(Err(reason)) => vec![GitState::Unknown(reason); inside.len()],
                    None => vec![GitState::Unknown(".git nicht mehr vorhanden".into()); inside.len()],
                },
                Err(reason) => vec![GitState::Unknown(reason.clone()); inside.len()],
            };
            for ((index, _), answer) in group.into_iter().zip(answers) {
                states[index].push(answer);
            }
        }
        for index in recoverable {
            match git_evidence(&self.items[index].evidence, &states[index]) {
                Ok(evidence) => self.items[index].evidence = evidence,
                Err(why) => self.demote(index, why),
            }
        }
    }

    /// A recoverable item that turned out to need a look.
    fn demote(&mut self, index: usize, why: String) {
        let item = &mut self.items[index];
        item.class = TidyClass::Review;
        item.evidence = format!("{} · {why}", item.evidence);
        let (bytes, allocated, modified) = (item.bytes, item.allocated, item.modified_at);
        self.regenerable = self.regenerable.saturating_sub(bytes);
        self.regenerable_allocated = self.regenerable_allocated.saturating_sub(allocated);
        self.review = self.review.saturating_add(bytes);
        if let Some(modified) = modified {
            self.touch(modified);
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
                self.add(bucket, entry);
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
        // Alone, outside coverage/, these are often copies or published reports.
        "lcov-report" => match here.file(BUILD_MANIFESTS) {
            Some(file) => {
                Verdict::Signed("Test-Ausgabe", format!("Abdeckungsbericht (Istanbul) · {}", beside(file)), LCOV_REPORT)
            }
            None => Verdict::Item(Review, "Test-Ausgabe", "Abdeckungsbericht ohne Build-Datei daneben".into()),
        },
        "testresults" => Verdict::Signed("Test-Ausgabe", "VSTest-Ergebnisse".into(), VSTEST_RESULTS),
        "blob-report" => Verdict::Signed("Test-Ausgabe", "Playwright-Blob-Bericht".into(), BLOB_REPORT),
        "allure-results" => Verdict::Signed("Test-Ausgabe", "Allure-Rohdaten".into(), ALLURE_RESULTS),
        // Maven writes them into target/; elsewhere only with pom.xml beside
        // (a custom reportsDirectory), otherwise they are copies.
        "surefire-reports" | "failsafe-reports" => {
            let (label, traces) = if name == "surefire-reports" {
                ("Surefire-Berichte", SUREFIRE_REPORTS)
            } else {
                ("Failsafe-Berichte", FAILSAFE_REPORTS)
            };
            match here.file(&["pom.xml"]) {
                Some(file) => Verdict::Signed("Test-Ausgabe", format!("{label} · {}", beside(file)), traces),
                None => Verdict::Item(Review, "Test-Ausgabe", format!("{label} ohne pom.xml daneben")),
            }
        }
        "mutants.out" => Verdict::Signed("Test-Ausgabe", "cargo-mutants-Ergebnisse".into(), MUTANTS_OUT),
        // After an aborted in-place run backup-* holds the only original sources.
        ".stryker-tmp" => {
            Verdict::Item(Review, "Test-Ausgabe", "Stryker-Arbeitsordner; kann Originaldateien enthalten".into())
        }
        // The report keeps the trend of earlier runs, a new run does not.
        "allure-report" => Verdict::Item(Review, "Test-Ausgabe", "Allure-Bericht mit Verlauf früherer Läufe".into()),
        "test-reports" => Verdict::Item(Review, "Test-Ausgabe", "Testberichte; Erzeuger unbekannt".into()),
        ".hypothesis" => {
            // Found failing examples are not reliably found again; teams commit them.
            Verdict::Item(
                Review,
                "Cache",
                "Hypothesis-Beispieldatenbank; gefundene Beispiele entstehen nicht sicher neu".into(),
            )
        }
        ".phpunit.cache" => {
            Verdict::Signed("Cache", "PHPUnit-Cache; entsteht beim nächsten Lauf neu".into(), PHPUNIT_CACHE)
        }
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

    fn write(root: &Path, rel: &str, contents: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
    }

    fn item<'a>(walk: &'a TidyWalk, rel: &str) -> &'a TidyItem {
        walk.items.iter().find(|i| i.rel_path == rel).unwrap_or_else(|| panic!("{rel}: {:#?}", walk.items))
    }

    /// A keep of a folder below a test root, at its real location.
    fn keep(root: &Path, rel: &str) -> Hold {
        let root = DirHandle::open_root(root, false).unwrap().final_path().unwrap();
        let location = format!("{root}\\{}", rel.replace('/', "\\"));
        Hold { kind: HoldKind::Keep, location, reason: KEPT.into() }
    }

    #[test]
    fn hard_links_reserve_their_space_once() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "Cargo.toml", 1);
        put(root, "target/CACHEDIR.TAG", 1);
        put(root, "target/debug/deps/app-1f2e.exe", 1 << 20);
        std::fs::hard_link(root.join("target/debug/deps/app-1f2e.exe"), root.join("target/debug/app.exe")).unwrap();
        let walk = tidy(root, None, None, &[]);
        let target = item(&walk, "target");
        assert_eq!(target.bytes, (2 << 20) + 1, "logical size counts each name");
        assert!(target.allocated < (2 << 20), "the shared file is reserved once: {}", target.allocated);
        assert_eq!(walk.regenerable_allocated, target.allocated);
    }

    #[test]
    fn versioned_or_kept_folders_are_no_longer_recoverable() {
        use crate::parsers::git_index::tests::index;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for manifest in ["package.json", "package-lock.json", "web/package.json", "web/package-lock.json"] {
            put(root, manifest, 1);
        }
        // A bundled GitHub Action commits its build output.
        put(root, "dist/index.js", 100);
        put(root, "node_modules/dep/index.js", 200);
        put(root, "web/node_modules/x/index.js", 300);
        put(root, "web/dist/app.js", 400);
        put(root, "sub/package.json", 1);
        put(root, "sub/yarn.lock", 1);
        put(root, "sub/node_modules/y/index.js", 50);
        write(root, "sub/.git", b"gitdir: ../.git/modules/sub\n");
        write(root, ".git/config", b"[core]\n\tbare = false\n");
        write(root, ".git/index", &index(2, 20, &["DIST/index.js", "package.json", "web/package.json"], &[]));
        let walk = tidy(root, None, None, &[keep(root, "web/dist")]);
        let dist = item(&walk, "dist");
        assert_eq!(dist.class, TidyClass::Review);
        assert!(dist.evidence.ends_with("1 Datei darin ist in Git versioniert"), "{}", dist.evidence);
        for free in ["node_modules", "web/node_modules"] {
            let item = item(&walk, free);
            assert_eq!(item.class, TidyClass::Regenerable, "{free}");
            assert!(item.evidence.ends_with(NOT_IN_GIT), "{}", item.evidence);
        }
        assert!(item(&walk, "web/dist").evidence.ends_with(KEPT));
        assert_eq!(item(&walk, "web/dist").class, TidyClass::Review);
        let submodule = item(&walk, "sub/node_modules");
        assert_eq!(submodule.class, TidyClass::Review, "the index of a submodule lies elsewhere");
        assert!(submodule.evidence.contains("Git-Stand unbekannt"), "{}", submodule.evidence);
        assert_eq!(walk.regenerable, 200 + 300);
        assert!(walk.review >= 100 + 400 + 50);
        assert_eq!(
            walk.regenerable_allocated,
            item(&walk, "node_modules").allocated + item(&walk, "web/node_modules").allocated
        );

        let broken = tempfile::tempdir().unwrap();
        put(broken.path(), "package.json", 1);
        put(broken.path(), "package-lock.json", 1);
        put(broken.path(), "node_modules/dep/index.js", 10);
        write(broken.path(), ".git/config", b"[core]\n");
        write(broken.path(), ".git/index", b"DIRC\0\0\0\x09");
        let walk = tidy(broken.path(), None, None, &[]);
        assert_eq!(item(&walk, "node_modules").class, TidyClass::Review, "an unreadable index decides nothing");
        assert_eq!(walk.regenerable, 0);
    }

    #[test]
    fn a_kept_folder_inside_keeps_the_folder_around_it() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "package.json", 1);
        put(root, "package-lock.json", 1);
        put(root, "node_modules/patched/index.js", 10);
        let keep = keep(root, "Node_Modules/patched");
        let walk = tidy(root, None, None, std::slice::from_ref(&keep));
        let modules = item(&walk, "node_modules");
        assert_eq!(modules.class, TidyClass::Review);
        assert!(modules.evidence.ends_with(&format!("von dir behalten: {}", keep.location)), "{}", modules.evidence);
    }

    #[test]
    fn every_work_tree_around_a_folder_is_asked() {
        use crate::parsers::git_index::tests::index;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "vendor/lib/package.json", 1);
        put(root, "vendor/lib/package-lock.json", 1);
        put(root, "vendor/lib/node_modules/patched/index.js", 10);
        // The outer repository committed the patch before the inner one existed.
        write(root, ".git/index", &index(2, 20, &["vendor/lib/node_modules/patched/index.js"], &[]));
        write(root, "vendor/lib/.git/index", &index(2, 20, &["package.json"], &[]));
        let walk = tidy(root, None, None, &[]);
        let modules = item(&walk, "vendor/lib/node_modules");
        assert_eq!(modules.class, TidyClass::Review, "{}", modules.evidence);
        assert!(modules.evidence.contains("in Git versioniert"), "{}", modules.evidence);
    }

    #[test]
    fn a_repository_above_the_approved_root_leaves_the_state_unknown() {
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), ".git/HEAD", b"ref: refs/heads/main\n");
        let root = temp.path().join("packages").join("web");
        put(&root, "package.json", 1);
        put(&root, "package-lock.json", 1);
        put(&root, "dist/app.js", 10);
        let walk = tidy(&root, None, None, &[]);
        let dist = item(&walk, "dist");
        assert_eq!(dist.class, TidyClass::Review);
        assert!(dist.evidence.contains("oberhalb der Freigabe"), "{}", dist.evidence);
    }

    #[test]
    fn agent_leftovers_need_a_look_and_are_summed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "Cargo.toml", 1);
        put(root, ".claude/settings.json", 10);
        put(root, ".claude/worktrees/fix-bug/src/main.rs", 100);
        put(root, ".claude/worktrees/fix-bug/target/debug/app.exe", 1_000);
        put(root, ".claude/worktrees/fix-bug/.git", 30);
        put(root, ".playwright-mcp/page-2026-10-31.png", 200);
        put(root, ".aider.tags.cache.v4/cache.db", 50);
        put(root, ".serena/cache/python/document_symbols.pkl", 20);
        put(root, ".serena/memories/notes.md", 5);
        put(root, "worktrees/notes.txt", 1);
        let walk = tidy(root, None, None, &[]);
        for rel in [".claude/worktrees", ".playwright-mcp", ".aider.tags.cache.v4", ".serena/cache"] {
            let found = item(&walk, rel);
            assert_eq!((found.class, found.label.as_str()), (TidyClass::Review, AGENT), "{rel}");
            assert!(found.evidence.ends_with("nur am Namen erkannt"), "{}", found.evidence);
        }
        for own in [".claude/worktrees/fix-bug/target", ".serena/memories", "worktrees", ".claude"] {
            assert!(walk.items.iter().all(|i| i.rel_path != own), "{own}");
        }
        assert_eq!(walk.agent, 100 + 1_000 + 30 + 200 + 50 + 20);
        assert_eq!(walk.regenerable, 0, "a worktree's build output is part of the worktree");
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
        // A fresh repository: no branch yet, nothing staged, so no index.
        write(root, ".git/HEAD", b"ref: refs/heads/main\n");
        std::fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
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
        let walk = tidy(root, None, None, &[]);
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
        assert_eq!(walk.vcs, 700 + 21);
        assert_eq!(walk.regenerable, 5_000 + 3_000 + 400 + 30);
        assert_eq!(walk.review, 200 + 900 + 600 + 800 + (120 << 20));
        let all =
            10 + 10 + 100 + 5_000 + 700 + 21 + 10 + 10 + 3_000 + 400 + 200 + 20 + 900 + 30 + 600 + 800 + (120 << 20);
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
        let walk = tidy(root, None, None, &[]);
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
        put(root, "tests/snapshots/e.txt", 70);
        put(root, "e2e/assets/f.png", 80);
        put(root, "test/goldens/g.png", 90);
        put(root, "golden/h.png", 5);
        put(root, "src/test/java/demo/golden/GoldenMasterTest.java", 7);
        put(root, "api/cassettes/login.yml", 40);
        put(root, "api/test_resources/i.bin", 20);
        let walk = tidy(root, None, None, &[]);
        assert_eq!(walk.test_data, 3_300 + 70 + 80 + 90 + 40 + 20, "{:#?}", walk.items);
        assert!(walk.items.iter().all(|i| i.rel_path != "data"), "data outside a test folder is the project's own");
        assert!(walk.items.iter().all(|i| i.rel_path != "golden"), "golden files only inside a test folder");
        assert!(walk.items.iter().any(|i| i.rel_path == "test/goldens" && i.label == TEST_DATA));
        assert!(
            walk.items.iter().all(|i| !i.rel_path.ends_with("demo/golden")),
            "deeper down a golden folder can hold test code"
        );
        assert_eq!(walk.review, walk.test_data);
    }

    #[test]
    fn test_code_never_becomes_an_item() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "pom.xml", 1);
        put(root, "src/test/java/demo/AppTest.java", 100);
        put(root, "tests/test_api.py", 100);
        put(root, "spec/model_spec.rb", 100);
        put(root, "__tests__/view.test.tsx", 100);
        put(root, "cypress/e2e/login.cy.ts", 100);
        put(root, "pkg/store_test.go", 100);
        let walk = tidy(root, None, None, &[]);
        assert!(walk.items.is_empty(), "{:#?}", walk.items);
        assert_eq!((walk.regenerable, walk.review, walk.test_data), (0, 0, 0));
    }

    #[test]
    fn new_test_outputs_and_caches_need_their_tool_trace() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        // Reports outside target/ or coverage/ need their build file beside.
        for rel in ["f/pom.xml", "g/pom.xml", "m/package.json"] {
            put(root, rel, 1);
        }
        for (rel, regenerable) in [
            ("a/TestResults/run.trx", true),
            ("b/TestResults/notes.txt", false),
            ("c/allure-results/0f1e-result.json", true),
            ("d/allure-report/widgets/summary.json", false),
            ("e/blob-report/report-3f2a.zip", true),
            ("f/surefire-reports/TEST-demo.AppTest.xml", true),
            ("g/failsafe-reports/failsafe-summary.xml", true),
            ("h/mutants.out/outcomes.json", true),
            ("i/.stryker-tmp/sandbox-Ab12/package.json", false),
            ("j/test-reports/junit.xml", false),
            ("k/.hypothesis/examples/1a2b/3c4d", false),
            ("l/.phpunit.cache/test-results", true),
            ("m/lcov-report/sorter.js", true),
            ("n/lcov-report/index.html", false),
            ("o/surefire-reports/TEST-copied.xml", false),
            ("p/lcov-report/sorter.js", false),
            ("q/.stryker-tmp/backup-Zz9/src/cart.ts", false),
        ] {
            put(root, rel, 10);
            let folder = rel.split('/').take(2).collect::<Vec<_>>().join("/");
            let walk = tidy(root, None, None, &[]);
            let item = walk
                .items
                .iter()
                .find(|i| i.rel_path == folder)
                .unwrap_or_else(|| panic!("{folder}: {:#?}", walk.items));
            let expected = if regenerable { TidyClass::Regenerable } else { TidyClass::Review };
            assert_eq!(item.class, expected, "{folder}: {}", item.evidence);
        }
        let parent = crate::platform::DirHandle::open_root(&root.join("a"), false).unwrap();
        let (label, evidence) = recoverable(&parent, "TestResults").unwrap();
        assert!(label == "Test-Ausgabe" && evidence.contains("*.trx darin"), "{evidence}");
        let parent = crate::platform::DirHandle::open_root(&root.join("b"), false).unwrap();
        assert!(recoverable(&parent, "TestResults").is_none(), "no trace, no cleanup");
        let parent = crate::platform::DirHandle::open_root(&root.join("d"), false).unwrap();
        assert!(recoverable(&parent, "allure-report").is_none(), "the report keeps a history");
    }

    #[test]
    fn every_test_output_and_test_data_name_of_the_discovery_has_a_rule() {
        let none = Names(Vec::new());
        let label = |name: &str| match verdict(name, &none) {
            Some(Verdict::Item(_, label, _) | Verdict::Signed(label, ..)) => Some(label),
            _ => None,
        };
        for name in super::super::discovery::TEST_OUTPUTS {
            assert_eq!(label(name), Some("Test-Ausgabe"), "{name}");
        }
        for name in TEST_DATA_FOLDERS {
            assert!(matches!(verdict(name, &none), Some(Verdict::Item(TidyClass::Review, TEST_DATA, _))), "{name}");
        }
    }

    #[test]
    fn a_replaced_root_is_not_measured() {
        let temp = tempfile::tempdir().unwrap();
        let identity = crate::platform::path_identity(temp.path()).unwrap();
        let other = tempfile::tempdir().unwrap();
        let walk = tidy(other.path(), Some(identity), None, &[]);
        assert!(walk.total.is_none() && walk.reason.is_some());
    }
}
