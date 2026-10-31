//! Whether folders of a Git work tree hold versioned files, read from the
//! repository's own index below the approved root. Git is never started; only
//! `.git/config` and `.git/index` are read, through a handle to a `.git`
//! folder that is no link. Anything the index cannot answer stays unknown.
//! Above the approved root only the existence of `.git` is checked
//! ([`above`]): such an index is not approved, so the state stays unknown.
use crate::fsread::{self, Budget, ReadIssue};
use crate::limits::ScanLimits;
use crate::parsers::git_index;
use crate::platform::{DirHandle, FileIdentity, HandleStat};
use std::ffi::OsStr;
use std::io;
use std::io::Read;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

/// Large monorepos reach tens of megabytes; beyond this the state is unknown.
const INDEX_MAX: u64 = 128 << 20;
const CONFIG_MAX: u64 = 1 << 20;

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

fn counts(git: &DirHandle, rel_paths: &[String], cache: &IndexCache) -> Result<Vec<u32>, String> {
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
    let (file, stat) = match git.open_file(OsStr::new("index")) {
        Ok(opened) => opened,
        // A new repository has no index until something is staged.
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return if has_commits(git) {
                Err("Git-Index fehlt, obwohl Commits vorhanden sind".into())
            } else {
                Ok(vec![0; rel_paths.len()])
            };
        }
        Err(e) => return Err(format!("Git-Index nicht lesbar: {e}")),
    };
    if let Some(bytes) = cache.find(&stat, hash_len) {
        return git_index::tracked_below_checked(&bytes, hash_len, rel_paths, false).map_err(|e| e.to_string());
    }
    if stat.size > INDEX_MAX {
        return Err(format!("Git-Index zu groß ({} MB)", stat.size >> 20));
    }
    let mut bytes = Vec::with_capacity(stat.size as usize);
    file.take(INDEX_MAX + 1).read_to_end(&mut bytes).map_err(|e| format!("Git-Index nicht lesbar: {e}"))?;
    if bytes.len() as u64 > INDEX_MAX {
        return Err("Git-Index zu groß".into());
    }
    let counts = git_index::tracked_below(&bytes, hash_len, rel_paths).map_err(|e| e.to_string())?;
    cache.keep(&stat, hash_len, bytes);
    Ok(counts)
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
