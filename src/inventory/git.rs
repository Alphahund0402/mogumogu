//! What a Git work tree says, read from the repository's own files below the
//! approved root. Git is never started. Read are `config`, `index`, `HEAD`,
//! `packed-refs`, loose refs and reflogs, through a handle to a `.git` folder
//! that is no link; object contents are not. Anything these cannot answer
//! stays unknown, and an answer can only ever add a reason to keep something.
//! Above the approved root only the existence of `.git` is checked
//! ([`above`]): such an index is not approved, so the state stays unknown.
use crate::domain::{CHECK_NAMES, GitRefs, GitTree, fold_case};
use crate::fsread::{self, Budget, ReadIssue};
use crate::limits::ScanLimits;
use crate::parsers::{git_index, git_refs};
use crate::platform::{DirHandle, FileIdentity, HandleStat};
use crate::privacy;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::io::Read;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

/// Large monorepos reach tens of megabytes; beyond this the state is unknown.
const INDEX_MAX: u64 = 128 << 20;
const CONFIG_MAX: u64 = 1 << 20;
const REF_MAX: u64 = 4096;
const PACKED_REFS_MAX: u64 = 32 << 20;
const REFLOG_MAX: u64 = 16 << 20;
/// Folders below `refs/heads` or `refs/remotes`, each part of a branch name.
const REF_DEPTH: usize = 24;
/// Reflogs read to tell "behind" from "ahead"; further branches count as ahead.
const REFLOGS_READ: usize = 32;
/// Index entries compared with the work tree; more leaves the state unknown.
const COMPARED_MAX: usize = 500_000;
const INDEX_MISSING: &str = "Git-Index fehlt, obwohl Commits vorhanden sind";
const REFTABLE: &str = "Das Repository nutzt das Reftable-Format, das mogumogu nicht liest";

/// What a work tree says about one folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitState {
    /// Index entries at or below the folder.
    Tracked(u32),
    Unknown(String),
}

/// The `.git` of a work tree: a folder to read, or why it cannot be read.
/// A `.git` file (worktrees, submodules) points to an index elsewhere; a
/// link is never followed.
pub fn open(work_tree: &DirHandle) -> Option<Result<DirHandle, String>> {
    let error = match work_tree.open_dir(OsStr::new(".git")) {
        Ok(git) => return Some(Ok(git)),
        Err(error) => error,
    };
    // Opening a file as a folder fails with "not a directory", not "not found".
    match work_tree.child_identity(OsStr::new(".git"), false) {
        Ok(None) if error.kind() == io::ErrorKind::NotFound => None,
        Ok(Some(_)) => Some(Err(OUTSIDE.into())),
        _ => Some(Err(format!(".git nicht sicher zu öffnen: {error}"))),
    }
}

/// Reason for a work tree whose index lies elsewhere.
pub const OUTSIDE: &str = ".git-Datei: der Index liegt außerhalb";

/// A work tree around the approved root, found by the existence of `.git`
/// in a folder above it; nothing in it is opened. `root` is the root's real
/// place (`DirHandle::final_path`). This only ever blocks: a project inside
/// a repository can hold versioned files the approval cannot see.
pub fn above(root: &Path) -> Option<String> {
    for folder in root.ancestors().skip(1) {
        match std::fs::symlink_metadata(folder.join(".git")) {
            Ok(_) => {
                return Some(format!(
                    "Git-Repository oberhalb der Freigabe in {}; sein Index ist nicht freigegeben",
                    folder.display()
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Some(format!(".git oberhalb der Freigabe nicht prüfbar ({}): {e}", folder.display())),
        }
    }
    None
}

/// Index files already read and checked during one plan, by the file's
/// identity, size and last write. Git writes a new file on every change, so
/// a changed index is a new key. Lives as long as one draft or execution.
#[derive(Debug, Default)]
pub struct IndexCache {
    entries: std::cell::RefCell<Vec<Cached>>,
}

/// Identity, size, last write and hash length of a checked index, and its bytes.
type Cached = (FileIdentity, u64, i64, usize, Rc<Vec<u8>>);

/// Larger indexes are read anew each time instead of kept.
const CACHED_MAX: u64 = 32 << 20;

impl IndexCache {
    fn find(&self, stat: &HandleStat, hash_len: usize) -> Option<Rc<Vec<u8>>> {
        self.entries.borrow().iter().find_map(|(identity, size, modified, hash, bytes)| {
            (*identity == stat.identity && *size == stat.size && *modified == stat.modified && *hash == hash_len)
                .then(|| Rc::clone(bytes))
        })
    }

    fn keep(&self, stat: &HandleStat, hash_len: usize, bytes: Vec<u8>) {
        if bytes.len() as u64 <= CACHED_MAX {
            let mut entries = self.entries.borrow_mut();
            entries.truncate(3);
            entries.insert(0, (stat.identity, stat.size, stat.modified, hash_len, Rc::new(bytes)));
        }
    }
}

/// For each of `rel_paths` (`/`-separated, relative to the work tree) how
/// many versioned entries lie at or below it.
pub fn tracked(git: &DirHandle, rel_paths: &[String]) -> Vec<GitState> {
    tracked_with(git, rel_paths, &IndexCache::default())
}

/// As [`tracked`], reusing an index already read in `cache`.
pub fn tracked_with(git: &DirHandle, rel_paths: &[String], cache: &IndexCache) -> Vec<GitState> {
    match counts(git, rel_paths, cache) {
        Ok(counts) => counts.into_iter().map(GitState::Tracked).collect(),
        Err(reason) => vec![GitState::Unknown(reason); rel_paths.len()],
    }
}

/// The index of a repository, opened after the checks every reader needs,
/// with the length of its object names. `None`: there is no index yet.
fn index_file(git: &DirHandle) -> Result<Option<(File, HandleStat, usize)>, String> {
    let limits = ScanLimits {
        max_depth: 1,
        max_entries: 1_000,
        max_total_bytes: INDEX_MAX + CONFIG_MAX,
        time_budget: Duration::from_secs(60),
    };
    let mut budget = Budget::new(limits);
    // Git is changing the index right now; what it lists is about to change.
    if !matches!(git.child_identity(OsStr::new("index.lock"), false), Ok(None)) {
        return Err("Git arbeitet gerade (index.lock)".into());
    }
    // Without a config Git uses SHA-1; a wrong guess fails the index layout.
    let config = match fsread::read_text(git, OsStr::new("config"), CONFIG_MAX, &mut budget) {
        Ok(config) => config,
        Err(ReadIssue::Io(e)) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Git-Konfiguration {e}")),
    };
    let hash_len = git_index::hash_len(&config);
    match git.open_file(OsStr::new("index")) {
        Ok((file, stat)) => Ok(Some((file, stat, hash_len))),
        // A new repository has no index until something is staged.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Git-Index nicht lesbar: {e}")),
    }
}

fn read_index(file: File, stat: &HandleStat) -> Result<Vec<u8>, String> {
    if stat.size > INDEX_MAX {
        return Err(format!("Git-Index zu groß ({} MB)", stat.size >> 20));
    }
    let mut bytes = Vec::with_capacity(stat.size as usize);
    file.take(INDEX_MAX + 1).read_to_end(&mut bytes).map_err(|e| format!("Git-Index nicht lesbar: {e}"))?;
    if bytes.len() as u64 > INDEX_MAX {
        return Err("Git-Index zu groß".into());
    }
    Ok(bytes)
}

fn counts(git: &DirHandle, rel_paths: &[String], cache: &IndexCache) -> Result<Vec<u32>, String> {
    let Some((file, stat, hash_len)) = index_file(git)? else {
        return if has_commits(git) { Err(INDEX_MISSING.into()) } else { Ok(vec![0; rel_paths.len()]) };
    };
    if let Some(bytes) = cache.find(&stat, hash_len) {
        return git_index::tracked_below_checked(&bytes, hash_len, rel_paths, false).map_err(|e| e.to_string());
    }
    let bytes = read_index(file, &stat)?;
    let counts = git_index::tracked_below(&bytes, hash_len, rel_paths).map_err(|e| e.to_string())?;
    cache.keep(&stat, hash_len, bytes);
    Ok(counts)
}

/// A file `rel` (`/`-separated, safe) below `git`; every folder on the way is
/// opened through its parent, so no link is followed.
fn nested_bytes(git: &DirHandle, rel: &str, max: u64, budget: &mut Budget) -> Result<Vec<u8>, ReadIssue> {
    if !git_refs::safe_ref(rel) {
        return Err(ReadIssue::Unsafe("Unzulässiger Ref-Name".into()));
    }
    let mut parts: Vec<&str> = rel.split('/').collect();
    let file = parts.pop().unwrap_or_default();
    let mut folder: Option<DirHandle> = None;
    for part in parts {
        let next = folder.as_ref().unwrap_or(git).open_dir(OsStr::new(part)).map_err(ReadIssue::Io)?;
        folder = Some(next);
    }
    fsread::read_bytes(folder.as_ref().unwrap_or(git), OsStr::new(file), max, budget)
}

/// As [`nested_bytes`] as lossy text, for files with names in any encoding;
/// `None` if the file is not there.
fn optional_text(git: &DirHandle, rel: &str, max: u64, budget: &mut Budget) -> Result<Option<String>, ReadIssue> {
    match nested_bytes(git, rel, max, budget) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(ReadIssue::Io(e)) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Loose refs below `top` (`refs/heads`, …), added over the packed ones. A
/// symbolic ref such as `origin/HEAD` names no commit and is left out.
fn loose_refs(
    git: &DirHandle,
    top: &str,
    found: &mut BTreeMap<String, String>,
    budget: &mut Budget,
) -> Result<(), String> {
    let mut opened: Option<DirHandle> = None;
    for part in top.split('/') {
        match opened.as_ref().unwrap_or(git).open_dir(OsStr::new(part)) {
            Ok(next) => opened = Some(next),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("{top} nicht lesbar: {e}")),
        }
    }
    let Some(start) = opened else { return Ok(()) };
    let mut stack = vec![(start, top.to_string())];
    while let Some((folder, prefix)) = stack.pop() {
        let (entries, truncated) = fsread::list(&folder, budget).map_err(|e| format!("{top} nicht lesbar: {e}"))?;
        if truncated {
            return Err("Zu viele Refs".into());
        }
        for entry in entries {
            if !budget.entry() {
                return Err("Zu viele Refs".into());
            }
            if entry.is_reparse() {
                return Err("Verknüpfung unter refs".into());
            }
            let name = entry.name_str().ok_or("Ref-Name nicht lesbar")?;
            let rel = format!("{prefix}/{name}");
            if entry.is_dir() {
                if rel.split('/').count() > REF_DEPTH {
                    return Err("Refs zu tief verschachtelt".into());
                }
                stack.push((folder.open_dir(&entry.name).map_err(|e| format!("{top} nicht lesbar: {e}"))?, rel));
            } else if !name.ends_with(".lock") && git_refs::safe_ref(&rel) {
                match fsread::read_bytes(&folder, &entry.name, REF_MAX, budget) {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        match git_refs::object_id(&text) {
                            Some(id) => {
                                found.insert(rel, id);
                            }
                            None if text.trim_start().starts_with("ref:") => {}
                            // A crash can leave an empty file; a packed value
                            // would then silently stand in for the branch.
                            None => return Err(format!("Ref beschädigt: {}", privacy::path(&rel))),
                        }
                    }
                    // Git may have just packed or removed it.
                    Err(ReadIssue::Io(e)) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("{top} {e}")),
                }
            }
        }
    }
    Ok(())
}

/// Branches, remotes and stashes of a repository from its own files. Whether
/// a commit is on a remote means: as this repository last saw it.
pub fn refs(git: &DirHandle) -> Result<GitRefs, String> {
    let limits = ScanLimits {
        max_depth: REF_DEPTH as u32,
        max_entries: 100_000,
        max_total_bytes: 512 << 20,
        time_budget: Duration::from_secs(20),
    };
    let mut budget = Budget::new(limits);
    if !matches!(git.child_identity(OsStr::new("reftable"), true), Ok(None)) {
        return Err(REFTABLE.into());
    }
    let config = optional_text(git, "config", CONFIG_MAX, &mut budget)
        .map_err(|e| format!("Git-Konfiguration {e}"))?
        .map_or_else(Default::default, |text| git_refs::config(&text));
    if config.reftable {
        return Err(REFTABLE.into());
    }
    let head = optional_text(git, "HEAD", REF_MAX, &mut budget)
        .map_err(|e| format!("HEAD {e}"))?
        .map_or(git_refs::Head::Unknown, |text| git_refs::head(&text));
    if head == git_refs::Head::Unknown {
        return Err("HEAD nicht lesbar".into());
    }
    let mut all = optional_text(git, "packed-refs", PACKED_REFS_MAX, &mut budget)
        .map_err(|e| format!("packed-refs {e}"))?
        .map_or_else(BTreeMap::new, |text| git_refs::packed_refs(&text));
    loose_refs(git, "refs/heads", &mut all, &mut budget)?;
    loose_refs(git, "refs/remotes", &mut all, &mut budget)?;
    if let Some(text) = optional_text(git, "refs/stash", REF_MAX, &mut budget).map_err(|e| format!("refs/stash {e}"))? {
        let id = git_refs::object_id(&text).ok_or("Ref beschädigt: refs/stash")?;
        all.insert("refs/stash".into(), id);
    }

    let heads: Vec<(&str, &String)> =
        all.iter().filter_map(|(name, id)| Some((name.strip_prefix("refs/heads/")?, id))).collect();
    let current = match &head {
        git_refs::Head::Branch(name) => Some(name.clone()),
        _ => None,
    };
    // Damage, or a branch without a first commit: its work is not in the refs.
    if current.as_ref().is_some_and(|name| !heads.is_empty() && !heads.iter().any(|(branch, _)| branch == name)) {
        return Err("Der aktive Branch hat noch keinen Commit oder ist nicht lesbar".into());
    }
    let (mut unpushed, mut local_only, mut read, mut on_local_remote) = (Vec::new(), Vec::new(), 0, 0_u32);
    for (branch, id) in &heads {
        if !budget.entry() {
            return Err("Zu viele Branches".into());
        }
        let tracking = config
            .upstream(branch)
            .filter(|t| all.contains_key(t))
            .or_else(|| config.remotes.iter().filter_map(|r| config.tracking(r, branch)).find(|t| all.contains_key(t)));
        let Some(tracking) = tracking else {
            local_only.push(branch.to_string());
            continue;
        };
        // The remote may simply be ahead: then it still has this very commit.
        let saved = all[&tracking] == **id || {
            read += 1;
            read <= REFLOGS_READ
                && matches!(optional_text(git, &format!("logs/{tracking}"), REFLOG_MAX, &mut budget),
                    Ok(Some(log)) if git_refs::reflog_keeps(&log, id))
        };
        if !saved {
            unpushed.push(branch.to_string());
        } else if !config.on_network(&tracking) {
            on_local_remote += 1;
        }
    }
    let shown = |mut names: Vec<String>| -> Vec<String> {
        names.sort_by_key(|name| Some(name) != current.as_ref());
        names.truncate(CHECK_NAMES);
        names.iter().map(|name| privacy::path(name)).collect()
    };
    let stashes = if all.contains_key("refs/stash") {
        optional_text(git, "logs/refs/stash", REFLOG_MAX, &mut budget)
            .ok()
            .flatten()
            .map_or(1, |log| git_refs::reflog_entries(&log).max(1))
    } else {
        0
    };
    let last_commit = optional_text(git, "logs/HEAD", REFLOG_MAX, &mut budget)
        .ok()
        .flatten()
        .and_then(|log| git_refs::last_commit(&log));
    // Each linked work tree has a folder here; the work tree itself may be anywhere.
    let worktrees = match git.open_dir(OsStr::new("worktrees")) {
        Ok(folder) => fsread::list(&folder, &mut budget)
            .map_err(|e| format!("worktrees nicht lesbar: {e}"))?
            .0
            .iter()
            .filter(|entry| entry.is_dir())
            .count() as u32,
        Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
        Err(e) => return Err(format!("worktrees nicht lesbar: {e}")),
    };
    // One-way: what copies of this repository have in common.
    let mut keys: Vec<String> = config.remote_keys.iter().cloned().collect();
    keys.extend(
        current.as_ref().and_then(|name| all.get(&format!("refs/heads/{name}"))).map(|id| format!("commit:{id}")),
    );
    Ok(GitRefs {
        keys,
        branch: current.as_deref().map(privacy::path),
        detached: head == git_refs::Head::Detached,
        remotes: config.remotes.len() as u32,
        local_remotes: config.local_remotes() as u32,
        on_local_remote,
        branches: heads.len() as u32,
        unpushed: unpushed.len() as u32,
        unpushed_names: shown(unpushed),
        local_only: local_only.len() as u32,
        local_only_names: shown(local_only),
        stashes,
        worktrees,
        last_commit,
    })
}

/// One index entry during the comparison with the work tree.
#[derive(Debug)]
struct Tracked {
    path: String,
    size: u32,
    mtime: u32,
    /// Size and time say something about this entry.
    compare: bool,
    /// A file of this name should exist in the work tree.
    expected: bool,
    seen: bool,
}

/// The work tree compared with the index, one listed file at a time. Only
/// metadata: as `git status` does first, without looking into any file.
#[derive(Debug, Default)]
pub struct TreeCheck {
    tracked: HashMap<String, Tracked>,
    /// Case-folded directory prefixes that contain at least one tracked file.
    tracked_dirs: HashSet<String>,
    staged: bool,
    conflicts: BTreeSet<String>,
    changed: u32,
    samples: Vec<String>,
}

impl TreeCheck {
    pub fn open(git: &DirHandle) -> Result<Self, String> {
        Self::open_with(git, &IndexCache::default())
    }

    /// Opens and verifies `.git/index`, reusing or populating `cache` so a
    /// subsequent `tracked_with` call in the same pass skips re-reading and
    /// re-hashing the index.
    pub fn open_with(git: &DirHandle, cache: &IndexCache) -> Result<Self, String> {
        let Some((file, stat, hash_len)) = index_file(git)? else {
            return if has_commits(git) { Err(INDEX_MISSING.into()) } else { Ok(Self::default()) };
        };
        let (bytes, cached) = match cache.find(&stat, hash_len) {
            Some(bytes) => (bytes, true),
            None => (Rc::new(read_index(file, &stat)?), false),
        };
        let mut check = Self::default();
        let (mut too_many, mut intent) = (false, false);
        let summary = git_index::entries(&bytes, hash_len, |entry| {
            if check.tracked.len() >= COMPARED_MAX {
                too_many = true;
                return;
            }
            let path = String::from_utf8_lossy(entry.path).into_owned();
            let key = fold_case(&path);
            for (slash, _) in key.match_indices('/') {
                if !check.tracked_dirs.contains(&key[..slash]) {
                    check.tracked_dirs.insert(key[..slash].to_string());
                }
            }
            intent |= entry.intent_to_add;
            let kind = entry.mode & 0o170000;
            if entry.stage != 0 {
                check.conflicts.insert(key.clone());
            }
            let tracked = Tracked {
                path,
                size: entry.size,
                mtime: entry.mtime,
                // Also with assume-unchanged and skip-worktree: `git status`
                // hides such edits, and they exist only on this disk.
                compare: entry.stage == 0 && kind == 0o100000,
                // Such a file may rightly be absent. A submodule is a folder,
                // and the walk meets only its files.
                expected: entry.stage == 0 && !entry.assume_valid && !entry.skip_worktree && kind != 0o160000,
                seen: false,
            };
            check.tracked.insert(key, tracked);
        })
        .map_err(|e| e.to_string())?;
        if too_many {
            return Err("Git-Index zu groß für den Abgleich".into());
        }
        if !cached {
            cache.keep(&stat, hash_len, Rc::unwrap_or_clone(bytes));
        }
        check.staged = intent || summary.staged;
        Ok(check)
    }

    /// True if the index contains any file directly or transitively below `rel_dir`.
    pub fn has_tracked_under(&self, rel_dir: &str) -> bool {
        if rel_dir.is_empty() { !self.tracked.is_empty() } else { self.tracked_dirs.contains(&fold_case(rel_dir)) }
    }

    /// Compares one listed file with its entry. False if Git does not list it.
    pub fn file(&mut self, rel: &str, size: u64, modified: i64) -> bool {
        let Some(tracked) = self.tracked.get_mut(&fold_case(rel)) else { return false };
        tracked.seen = true;
        if tracked.compare && (tracked.size != size as u32 || i64::from(tracked.mtime) != modified) {
            self.changed = self.changed.saturating_add(1);
            if self.samples.len() < CHECK_NAMES {
                self.samples.push(privacy::path(rel));
            }
        }
        true
    }

    /// A name the walk does not compare, a link for example, is still there.
    pub fn present(&mut self, rel: &str) {
        if let Some(tracked) = self.tracked.get_mut(&fold_case(rel)) {
            tracked.seen = true;
        }
    }

    /// The result of a complete walk; files outside Git are the caller's.
    pub fn finish(mut self) -> GitTree {
        let mut missing: Vec<&str> =
            self.tracked.values().filter(|t| t.expected && !t.seen).map(|t| t.path.as_str()).collect();
        missing.sort_unstable();
        let deleted = missing.len() as u32;
        let room = CHECK_NAMES.saturating_sub(self.samples.len());
        self.samples.extend(missing.into_iter().take(room).map(privacy::path));
        GitTree {
            staged: self.staged,
            changed: self.changed,
            deleted,
            conflicts: self.conflicts.len() as u32,
            samples: self.samples,
            ..GitTree::default()
        }
    }
}

/// Whether the repository may have commits: a branch, tag or packed ref, a
/// detached HEAD, or the reftable format. Anything unreadable counts as yes;
/// only a new repository with a symbolic HEAD and no refs says no.
fn has_commits(git: &DirHandle) -> bool {
    let absent = |name: &str| matches!(git.child_identity(OsStr::new(name), false), Ok(None));
    let no_folder = |name: &str| matches!(git.child_identity(OsStr::new(name), true), Ok(None));
    if !absent("packed-refs") || !no_folder("reftable") {
        return true;
    }
    let mut budget = Budget::new(ScanLimits { max_total_bytes: 4096, ..ScanLimits::default() });
    let symbolic = fsread::read_text(git, OsStr::new("HEAD"), 4096, &mut budget)
        .is_ok_and(|head| head.trim_start().starts_with("ref: refs/heads/"));
    let refs = |kind: &str| {
        let folder = git.open_dir(OsStr::new("refs")).and_then(|refs| refs.open_dir(OsStr::new(kind)));
        match folder {
            Err(e) if e.kind() == io::ErrorKind::NotFound && kind == "tags" => false,
            folder => folder.and_then(|folder| folder.entries(1)).map_or(true, |(entries, _)| !entries.is_empty()),
        }
    };
    !symbolic || refs("heads") || refs("tags")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::parsers::git_index::tests::index;

    fn put(root: &std::path::Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn git_of(root: &std::path::Path) -> DirHandle {
        let tree = DirHandle::open_root(root, false).unwrap();
        open(&tree).expect("a repository").expect("a readable .git")
    }

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const C: &str = "cccccccccccccccccccccccccccccccccccccccc";
    const ORIGIN: &str = "[remote \"origin\"]\n\turl = https://u:hunter2@example.com/x.git\n\
                          \tfetch = +refs/heads/*:refs/remotes/origin/*\n";

    #[test]
    fn branches_remotes_and_stashes_come_from_the_repositorys_own_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, ".git/HEAD", b"ref: refs/heads/ahead\n");
        let config = format!(
            "{ORIGIN}[branch \"same\"]\n\tremote = origin\n\tmerge = refs/heads/same\n\
             [branch \"behind\"]\n\tremote = origin\n\tmerge = refs/heads/behind\n\
             [branch \"ahead\"]\n\tremote = origin\n\tmerge = refs/heads/ahead\n"
        );
        put(root, ".git/config", config.as_bytes());
        // One branch only packed, whose loose remote ref wins over a stale packed one.
        put(
            root,
            ".git/packed-refs",
            format!("# pack\n{A} refs/heads/same\n{B} refs/remotes/origin/same\n").as_bytes(),
        );
        put(root, ".git/refs/remotes/origin/same", A.as_bytes());
        put(root, ".git/refs/heads/behind", A.as_bytes());
        put(root, ".git/refs/remotes/origin/behind", B.as_bytes());
        put(root, ".git/logs/refs/remotes/origin/behind", format!("{C} {A} A <a@b> 1 +0000\tfetch\n").as_bytes());
        put(root, ".git/refs/heads/ahead", C.as_bytes());
        put(root, ".git/refs/remotes/origin/ahead", A.as_bytes());
        put(root, ".git/refs/remotes/origin/HEAD", b"ref: refs/remotes/origin/same\n");
        put(root, ".git/refs/heads/feature/only-here", B.as_bytes());
        put(root, ".git/refs/stash", C.as_bytes());
        put(
            root,
            ".git/logs/refs/stash",
            format!("{A} {C} A <a@b> 1 +0000\tWIP\n{C} {B} A <a@b> 2 +0000\tWIP\n").as_bytes(),
        );
        put(root, ".git/logs/HEAD", format!("{A} {C} Ann <a@b> 1700000000 +0000\tcommit: x\n").as_bytes());

        let found = refs(&git_of(root)).unwrap();
        assert_eq!(found.branch.as_deref(), Some("ahead"));
        assert_eq!((found.remotes, found.branches, found.stashes), (1, 4, 2));
        assert_eq!((found.unpushed, found.unpushed_names.as_slice()), (1, ["ahead".to_string()].as_slice()));
        assert_eq!(
            (found.local_only, found.local_only_names.as_slice()),
            (1, ["feature/only-here".to_string()].as_slice())
        );
        assert_eq!(found.last_commit, Some(1_700_000_000));
        assert_eq!(found.worktrees, 0);
        put(root, ".git/worktrees/elsewhere/gitdir", b"D:/wt/.git\n");
        assert_eq!(refs(&git_of(root)).unwrap().worktrees, 1);
        assert!(!format!("{found:?}").contains("hunter2"));
    }

    #[test]
    fn a_hostile_repository_cannot_lead_the_read_outside_git() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, ".git/HEAD", b"ref: refs/heads/main\n");
        let config =
            format!("{ORIGIN}[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/../../../../../outside\n");
        put(root, ".git/config", config.as_bytes());
        put(root, ".git/refs/heads/main", A.as_bytes());
        put(root, ".git/packed-refs", format!("{B} refs/remotes/origin/../../../../outside\n").as_bytes());
        let found = refs(&git_of(root)).unwrap();
        assert_eq!((found.local_only, found.unpushed), (1, 0), "no upstream can be named this way");
        let mut budget = Budget::new(ScanLimits::default());
        assert!(matches!(
            nested_bytes(&git_of(root), "logs/../../outside", 10, &mut budget),
            Err(ReadIssue::Unsafe(_))
        ));
    }

    #[test]
    fn a_damaged_ref_or_a_vanished_branch_stays_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, ".git/HEAD", b"ref: refs/heads/main\n");
        put(root, ".git/refs/heads/main", A.as_bytes());
        put(root, ".git/packed-refs", format!("{A} refs/heads/feature\n").as_bytes());
        assert_eq!(refs(&git_of(root)).unwrap().branches, 2);
        put(root, ".git/refs/heads/feature", b"");
        assert!(refs(&git_of(root)).unwrap_err().contains("beschädigt"), "an empty file must not fall back to packed");
        std::fs::remove_file(root.join(".git/refs/heads/feature")).unwrap();
        put(root, ".git/refs/stash", b"\0\0\0");
        assert!(refs(&git_of(root)).unwrap_err().contains("beschädigt"), "the stash check must not disappear");
        std::fs::remove_file(root.join(".git/refs/stash")).unwrap();
        put(root, ".git/HEAD", b"ref: refs/heads/gone\n");
        assert!(refs(&git_of(root)).unwrap_err().contains("aktive Branch"));
    }

    #[test]
    fn a_remote_on_a_path_is_counted_as_such() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, ".git/HEAD", b"ref: refs/heads/main\n");
        put(
            root,
            ".git/config",
            b"[remote \"origin\"]\n\turl = D:/backup/a.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
        );
        put(root, ".git/refs/heads/main", A.as_bytes());
        put(root, ".git/refs/remotes/origin/main", A.as_bytes());
        let found = refs(&git_of(root)).unwrap();
        assert_eq!((found.remotes, found.local_remotes, found.on_local_remote, found.unpushed), (1, 1, 1, 0));
    }

    #[test]
    fn what_cannot_be_read_stays_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, ".git/config", b"[core]\n");
        assert!(refs(&git_of(root)).unwrap_err().contains("HEAD"), "no HEAD");
        put(root, ".git/HEAD", b"not a ref\n");
        assert!(refs(&git_of(root)).unwrap_err().contains("HEAD"));
        put(root, ".git/HEAD", b"ref: refs/heads/main\n");
        assert_eq!(refs(&git_of(root)).unwrap().branches, 0, "a new repository");
        put(root, ".git/config", b"[extensions]\n\trefstorage = reftable\n");
        assert!(refs(&git_of(root)).unwrap_err().contains("Reftable"));
        put(root, ".git/config", b"[core]\n");
        std::fs::create_dir_all(root.join(".git/reftable")).unwrap();
        assert!(refs(&git_of(root)).unwrap_err().contains("Reftable"));
    }

    /// An index whose entries carry the real size and time of `paths`, as
    /// Git writes them; `index()` alone leaves them empty.
    fn stamped_index(root: &std::path::Path, paths: &[&str]) -> Vec<u8> {
        let mut bytes = index(2, 20, paths, &[]);
        let mut at = 12;
        for path in paths {
            let meta = std::fs::metadata(root.join(path)).unwrap();
            let modified = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as u32;
            bytes[at + 8..at + 12].copy_from_slice(&modified.to_be_bytes());
            bytes[at + 36..at + 40].copy_from_slice(&(meta.len() as u32).to_be_bytes());
            at += (62 + path.len() + 8) & !7;
        }
        let end = bytes.len() - 20;
        let hash = crate::parsers::sha1::sha1(&bytes[..end]);
        bytes[end..].copy_from_slice(&hash);
        bytes
    }

    #[test]
    fn the_work_tree_is_compared_by_size_and_time_alone() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for rel in ["same.txt", "edited.txt", "gone.txt"] {
            put(root, rel, b"text");
        }
        put(root, ".git/index", &stamped_index(root, &["edited.txt", "gone.txt", "same.txt"]));
        let stat = |rel: &str| {
            let meta = std::fs::metadata(root.join(rel)).unwrap();
            (meta.len(), meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64)
        };
        let (same_size, same_time) = stat("same.txt");
        let mut check = TreeCheck::open(&git_of(root)).unwrap();
        assert!(check.file("SAME.txt", same_size, same_time), "Windows compares names without case");
        assert!(check.file("edited.txt", same_size + 3, same_time));
        assert!(!check.file("new.txt", 1, same_time), "Git does not list it");
        let tree = check.finish();
        assert_eq!((tree.changed, tree.deleted), (1, 1));
        assert_eq!(tree.samples, ["edited.txt", "gone.txt"]);
        assert!(!tree.staged, "this index has no cached tree and proves nothing");
    }

    #[test]
    fn a_new_time_alone_counts_and_a_git_at_work_is_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "a.txt", b"text");
        put(root, ".git/index", &stamped_index(root, &["a.txt"]));
        let mut check = TreeCheck::open(&git_of(root)).unwrap();
        assert!(check.file("a.txt", 4, 1));
        assert_eq!(check.finish().changed, 1);
        put(root, ".git/index.lock", b"");
        assert!(TreeCheck::open(&git_of(root)).unwrap_err().contains("index.lock"));
    }

    fn state(root: &std::path::Path, rel: &str) -> Option<GitState> {
        let tree = DirHandle::open_root(root, false).unwrap();
        let git = open(&tree)?;
        Some(match git {
            Ok(git) => tracked(&git, &[rel.to_string()]).remove(0),
            Err(reason) => GitState::Unknown(reason),
        })
    }

    #[test]
    fn the_index_answers_and_everything_else_stays_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        assert_eq!(state(root, "dist"), None, "no work tree");
        put(root, ".git/config", b"[core]\n");
        put(root, ".git/HEAD", b"ref: refs/heads/main\n");
        std::fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
        assert_eq!(state(root, "dist"), Some(GitState::Tracked(0)), "a new repository tracks nothing");
        put(root, ".git/refs/heads/main", b"0123\n");
        assert!(matches!(state(root, "dist"), Some(GitState::Unknown(_))), "commits without an index");
        put(root, ".git/index", &index(2, 20, &["dist/a.js", "src/b.rs"], &[]));
        assert_eq!(state(root, "dist"), Some(GitState::Tracked(1)));
        assert_eq!(state(root, "node_modules"), Some(GitState::Tracked(0)));
        put(root, ".git/index", b"DIRC broken");
        assert!(matches!(state(root, "dist"), Some(GitState::Unknown(_))));
        let worktree = tempfile::tempdir().unwrap();
        put(worktree.path(), ".git", b"gitdir: C:/elsewhere/.git/worktrees/x\n");
        assert_eq!(state(worktree.path(), "dist"), Some(GitState::Unknown(OUTSIDE.into())));
    }
}
