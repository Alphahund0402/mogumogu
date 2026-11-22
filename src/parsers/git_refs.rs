//! Git's small text files about branches, read as data: `HEAD`, loose refs,
//! `packed-refs`, the parts of `config` that say which branch follows which
//! remote, and reflogs. Only branch names, object names and times come out of
//! here; remote addresses, author names and e-mail addresses are never kept.
//! Nothing is executed and `include` directives are not followed.
use std::collections::{BTreeMap, BTreeSet};

/// More lines than any of these files has in a repository this tool is for.
const MAX_LINES: usize = 2_000_000;
/// The year 2100: a later time in a reflog is damage, not a commit.
const LATEST_TIME: i64 = 4_102_444_800;

/// What `HEAD` points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    Detached,
    Unknown,
}

pub fn head(text: &str) -> Head {
    let text = text.trim();
    match text.strip_prefix("ref:") {
        Some(target) => match target.trim().strip_prefix("refs/heads/") {
            Some(branch) if safe_ref(branch) => Head::Branch(branch.to_string()),
            _ => Head::Unknown,
        },
        None if object_id(text).is_some() => Head::Detached,
        None => Head::Unknown,
    }
}

/// An object name in lower case: 40 hex digits for SHA-1, 64 for SHA-256.
pub fn object_id(text: &str) -> Option<String> {
    let text = text.trim();
    (matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())).then(|| text.to_ascii_lowercase())
}

/// Whether every `/`-separated part of `name` can only ever be one folder or
/// file below `.git`. Paths are built from names in a repository's own files,
/// and a hostile repository must not climb out with `..` or name a stream.
pub fn safe_ref(name: &str) -> bool {
    !name.is_empty()
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.chars().any(|c| c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        })
}

/// `refs/…` names and their objects from `packed-refs`. Loose refs win over
/// these, so the caller adds them afterwards.
pub fn packed_refs(text: &str) -> BTreeMap<String, String> {
    let mut refs = BTreeMap::new();
    for line in text.lines().take(MAX_LINES) {
        if line.starts_with(['#', '^']) {
            continue;
        }
        if let Some((id, name)) = line.split_once(' ')
            && let Some(id) = object_id(id)
            && safe_ref(name)
        {
            refs.insert(name.to_string(), id);
        }
    }
    refs
}

/// The parts of a repository's `config` this tool asks about.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitConfig {
    /// Names of remotes that have an address. The address itself is dropped.
    pub remotes: BTreeSet<String>,
    /// Those with an address that leads to another computer.
    network: BTreeSet<String>,
    /// One-way keys of the addresses, to recognise copies of a repository.
    pub remote_keys: BTreeSet<String>,
    /// Remotes that keep their branches under `refs/remotes/<name>/`.
    usual_fetch: BTreeSet<String>,
    /// Branch name to its remote and the ref merged from there.
    upstreams: BTreeMap<String, (String, String)>,
    /// Refs live in the reftable format, which this tool does not read.
    pub reftable: bool,
}

impl GitConfig {
    /// The ref that shows `branch`'s upstream as this repository last saw it.
    pub fn upstream(&self, branch: &str) -> Option<String> {
        let (remote, merge) = self.upstreams.get(branch)?;
        self.tracking(remote, merge.strip_prefix("refs/heads/")?)
    }

    /// Where `remote` keeps its branch `name` here, if it follows the usual
    /// layout. A refspec of its own could put it anywhere.
    pub fn tracking(&self, remote: &str, name: &str) -> Option<String> {
        if !self.usual_fetch.contains(remote) {
            return None;
        }
        let tracking = format!("refs/remotes/{remote}/{name}");
        safe_ref(&tracking).then_some(tracking)
    }

    /// Remotes whose address names a path, not another computer.
    pub fn local_remotes(&self) -> usize {
        self.remotes.difference(&self.network).count()
    }

    /// Whether a tracking ref belongs to a remote on another computer.
    pub fn on_network(&self, tracking: &str) -> bool {
        self.network.iter().any(|remote| tracking.starts_with(&format!("refs/remotes/{remote}/")))
    }
}

/// A one-way key of a remote address, the same for its spellings: HTTPS and
/// SSH of one repository, with or without `.git`, user or trailing slash. The
/// address cannot be read back from it and it never leaves the process.
fn remote_key(url: &str) -> String {
    let mut place = url.trim().replace('\\', "/");
    if let Some((_, rest)) = place.split_once("://") {
        place = rest.to_string();
    } else if network_address(&place)
        && let Some((host, path)) = place.split_once(':')
    {
        place = format!("{host}/{path}");
    }
    let authority = place.find('/').unwrap_or(place.len());
    if let Some(at) = place[..authority].rfind('@') {
        place.drain(..=at);
    }
    let place = place.trim_end_matches('/').to_lowercase();
    let place = place.strip_suffix(".git").unwrap_or(&place);
    let digest = crate::sha256::Sha256::digest(place);
    let hex: String = digest[..16].iter().map(|byte| format!("{byte:02x}")).collect();
    format!("remote:{hex}")
}

/// Whether a remote address leads to another computer: a URL with a network
/// scheme or `host:path`. A drive letter, a path or `file:` may be this very
/// disk, which is no copy of anything. The address itself is not kept.
fn network_address(url: &str) -> bool {
    if let Some((scheme, _)) = url.split_once("://") {
        return !scheme.eq_ignore_ascii_case("file");
    }
    match url.split_once(':') {
        Some((host, _)) => host.len() > 1 && !host.contains(['/', '\\']) && !host.eq_ignore_ascii_case("file"),
        None => false,
    }
}

pub fn config(text: &str) -> GitConfig {
    let mut parsed = GitConfig::default();
    let mut section = String::new();
    let mut subsection: Option<String> = None;
    let mut partial: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    for line in text.lines().take(MAX_LINES) {
        let mut line = line.trim();
        if line.starts_with('[') {
            match section_header(line) {
                Some((name, sub, rest)) => {
                    section = name;
                    subsection = sub;
                    line = rest.trim();
                }
                None => {
                    section.clear();
                    subsection = None;
                    continue;
                }
            }
        }
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else { continue };
        let key = key.trim().to_ascii_lowercase();
        match (section.as_str(), subsection.as_deref(), key.as_str()) {
            ("extensions", None, "refstorage") => parsed.reftable = value(raw).eq_ignore_ascii_case("reftable"),
            ("remote", Some(remote), "url") if !value(raw).is_empty() => {
                parsed.remotes.insert(remote.to_string());
                parsed.remote_keys.insert(remote_key(&value(raw)));
                if network_address(&value(raw)) {
                    parsed.network.insert(remote.to_string());
                }
            }
            ("remote", Some(remote), "fetch") => {
                // All branches, or one branch as a single-branch clone has it.
                let refspec = value(raw);
                let mapped = refspec.trim_start_matches('+').split_once(':').is_some_and(|(from, to)| {
                    from.strip_prefix("refs/heads/").is_some_and(|from| {
                        to.strip_prefix(&format!("refs/remotes/{remote}/")).is_some_and(|to| from == to)
                    })
                });
                if mapped {
                    parsed.usual_fetch.insert(remote.to_string());
                }
            }
            ("branch", Some(branch), "remote") => partial.entry(branch.to_string()).or_default().0 = Some(value(raw)),
            ("branch", Some(branch), "merge") => partial.entry(branch.to_string()).or_default().1 = Some(value(raw)),
            _ => {}
        }
    }
    parsed.upstreams = partial
        .into_iter()
        .filter_map(|(branch, (remote, merge))| Some((branch, (remote?, merge?))))
        .filter(|(_, (remote, _))| parsed.remotes.contains(remote))
        .collect();
    parsed
}

/// `[name]`, `[name "sub"]` or the older `[name.sub]`, and what follows on
/// the line. Names are case-insensitive, subsections are not.
fn section_header(line: &str) -> Option<(String, Option<String>, &str)> {
    let body = line.strip_prefix('[')?;
    let end = body.find([' ', '\t', ']', '"'])?;
    let name = body[..end].to_ascii_lowercase();
    let after = body[end..].trim_start();
    if let Some(rest) = after.strip_prefix(']') {
        return Some(match name.split_once('.') {
            Some((section, sub)) => (section.to_string(), Some(sub.to_string()), rest),
            None => (name, None, rest),
        });
    }
    let quoted = after.strip_prefix('"')?;
    let mut sub = String::new();
    let mut chars = quoted.char_indices();
    while let Some((at, ch)) = chars.next() {
        match ch {
            '\\' => sub.push(chars.next()?.1),
            '"' => return Some((name, Some(sub), quoted[at + 1..].trim_start().strip_prefix(']')?)),
            _ => sub.push(ch),
        }
    }
    None
}

/// A config value without its quotes, escapes and trailing comment.
fn value(raw: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    let mut chars = raw.trim().chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => quoted = !quoted,
            '\\' => out.extend(chars.next()),
            '#' | ';' if !quoted => break,
            _ => out.push(ch),
        }
    }
    out.trim_end().to_string()
}

/// Whether a remote-tracking ref is known to still contain `id`: it was once
/// its tip and every later move was a fast-forward. A forced update can have
/// thrown the commit away, and a push from here does not say which it was.
pub fn reflog_keeps(text: &str, id: &str) -> bool {
    let lines: Vec<&str> = text.lines().take(MAX_LINES).collect();
    let last = lines.iter().rposition(|line| line.split(' ').nth(1).is_some_and(|new| new.eq_ignore_ascii_case(id)));
    last.is_some_and(|at| {
        lines[at + 1..].iter().all(|line| line.split_once('\t').is_some_and(|(_, why)| why.contains("fast-forward")))
    })
}

/// Entries of a reflog, for example the number of stashes.
pub fn reflog_entries(text: &str) -> u32 {
    text.lines().take(MAX_LINES).filter(|line| !line.trim().is_empty()).count() as u32
}

/// When a commit was last made here, from the reflog of `HEAD`. The line is
/// `old new author <mail> time zone<TAB>message`; only the time is taken.
pub fn last_commit(text: &str) -> Option<i64> {
    text.lines().rev().take(MAX_LINES).find_map(|line| {
        let (header, message) = line.split_once('\t')?;
        if !message.starts_with("commit") {
            return None;
        }
        header.rsplit(' ').nth(1)?.parse().ok().filter(|time| (1..=LATEST_TIME).contains(time))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn head_names_a_branch_a_commit_or_nothing_usable() {
        assert_eq!(head("ref: refs/heads/feature/x\n"), Head::Branch("feature/x".into()));
        assert_eq!(head(&format!("{A}\n")), Head::Detached);
        assert_eq!(head("ref: refs/tags/v1\n"), Head::Unknown);
        assert_eq!(head("ref: refs/heads/../../config\n"), Head::Unknown);
        assert_eq!(head("nonsense"), Head::Unknown);
    }

    #[test]
    fn object_names_are_hex_of_the_two_known_lengths() {
        assert_eq!(object_id(&format!("{}\n", A.to_uppercase())).as_deref(), Some(A));
        assert!(object_id(&"c".repeat(64)).is_some());
        assert!(object_id(&"c".repeat(41)).is_none());
        assert!(object_id(&format!("{}g", &A[..39])).is_none());
    }

    #[test]
    fn a_ref_name_stays_below_git() {
        for good in ["main", "feature/x", "refs/remotes/origin/main", "ümlaut"] {
            assert!(safe_ref(good), "{good}");
        }
        for bad in ["", "..", "a/../b", "a//b", "/a", "a/", "a\\b", "c:x", "a:stream", "a\nb", "./a"] {
            assert!(!safe_ref(bad), "{bad:?}");
        }
    }

    #[test]
    fn packed_refs_skip_comments_peeled_lines_and_unsafe_names() {
        let text = format!(
            "# pack-refs with: peeled fully-peeled sorted\n{A} refs/heads/main\n^{B}\n{B} refs/heads/../escape\nbroken line\n"
        );
        let refs = packed_refs(&text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs["refs/heads/main"], A);
    }

    #[test]
    fn config_gives_upstreams_and_never_the_address() {
        let parsed = config(concat!(
            "[core]\n\tbare = false\n",
            "[remote \"origin\"]\n\turl = https://user:hunter2@example.com/a.git\n",
            "\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
            "[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n",
            "[branch \"odd\"]\n\tremote = origin\n\tmerge = refs/heads/trunk ; comment\n",
            "[branch \"local\"]\n\tremote = .\n\tmerge = refs/heads/main\n",
        ));
        assert_eq!(parsed.remotes.iter().collect::<Vec<_>>(), ["origin"]);
        assert_eq!(parsed.upstream("main").as_deref(), Some("refs/remotes/origin/main"));
        assert_eq!(parsed.upstream("odd").as_deref(), Some("refs/remotes/origin/trunk"));
        assert_eq!(parsed.upstream("local"), None, "a local upstream is no remote");
        assert_eq!(parsed.upstream("missing"), None);
        assert!(!format!("{parsed:?}").contains("hunter2"), "the address is not kept");
    }

    #[test]
    fn an_own_refspec_leaves_the_tracking_ref_unknown() {
        let parsed = config(concat!(
            "[remote \"o\"]\nurl = x\nfetch = +refs/heads/*:refs/mirror/*\n",
            "[branch \"main\"]\nremote = o\nmerge = refs/heads/main\n",
        ));
        assert_eq!(parsed.upstream("main"), None);
        assert_eq!(parsed.remotes.len(), 1);
    }

    #[test]
    fn a_hostile_config_cannot_name_a_path_outside() {
        let parsed = config(concat!(
            "[remote \"origin\"]\nurl = x\nfetch = +refs/heads/*:refs/remotes/origin/*\n",
            "[branch \"main\"]\nremote = origin\nmerge = refs/heads/../../../../secret\n",
        ));
        assert_eq!(parsed.upstream("main"), None);
    }

    #[test]
    fn section_headers_follow_gits_syntax() {
        let parsed = config(concat!(
            "[Remote \"a\\\"b\"] url = x\n",
            "[remote.old]\nurl = \"y\" # comment\n",
            "[extensions]\n\trefStorage = reftable\n",
        ));
        assert!(parsed.remotes.contains("a\"b") && parsed.remotes.contains("old"), "{parsed:?}");
        assert!(parsed.reftable);
    }

    #[test]
    fn reflogs_give_ids_counts_and_the_last_commit_only() {
        let log = format!(
            "{A} {B} Ann <ann@example.com> 1700000000 +0100\tcommit: first\n\
             {B} {A} Ann <ann@example.com> 1700000100 +0100\tcheckout: moving from a to b\n"
        );
        assert_eq!(reflog_entries(&log), 2);
        assert_eq!(reflog_entries(""), 0);
        assert_eq!(last_commit(&log), Some(1_700_000_000), "the checkout is no commit");
        assert_eq!(last_commit("x y a <b> 5 +0000\tclone: from elsewhere\n"), None);
    }

    #[test]
    fn a_damaged_time_is_no_commit() {
        assert_eq!(last_commit("x y a <b> -9223372036854775808 +0000\tcommit: x\n"), None);
        assert_eq!(last_commit("x y a <b> 99999999999 +0000\tcommit: x\n"), None);
    }

    #[test]
    fn only_fast_forwards_keep_a_commit_on_the_remote() {
        let c = "c".repeat(40);
        let pushed = format!("{A} {B} A <a> 1 +0000\tupdate by push\n");
        assert!(reflog_keeps(&pushed, B), "the current tip");
        let forward = format!("{pushed}{B} {c} A <a> 2 +0000\tfetch origin: fast-forward\n");
        assert!(reflog_keeps(&forward, B));
        let forced = format!("{pushed}{B} {c} A <a> 2 +0000\tfetch origin: forced-update\n");
        assert!(!reflog_keeps(&forced, B), "a forced update can have thrown it away");
        let then_forward = format!("{forced}{c} {A} A <a> 3 +0000\tfetch origin: fast-forward\n");
        assert!(!reflog_keeps(&then_forward, B));
        assert!(!reflog_keeps(&pushed, &c), "never its tip");
    }

    #[test]
    fn a_remote_on_a_path_is_not_another_computer() {
        for far in ["https://example.com/a.git", "ssh://git@host/a.git", "git@github.com:a/b.git", "host:repo"] {
            assert!(network_address(far), "{far}");
        }
        for near in [
            "D:/backup/a.git",
            r"D:\backup\a.git",
            "../a.git",
            "/c/a.git",
            r"\\nas\a.git",
            "file:///c/a",
            "file:a",
            "a.git",
        ] {
            assert!(!network_address(near), "{near}");
        }
        let parsed = config(concat!(
            "[remote \"far\"]\nurl = git@github.com:a/b.git\nfetch = +refs/heads/*:refs/remotes/far/*\n",
            "[remote \"near\"]\nurl = D:/backup/a.git\nfetch = +refs/heads/*:refs/remotes/near/*\n",
        ));
        assert_eq!(parsed.local_remotes(), 1);
        assert!(parsed.on_network("refs/remotes/far/main"));
        assert!(!parsed.on_network("refs/remotes/near/main"));
    }

    #[test]
    fn spellings_of_one_address_share_a_key_that_hides_it() {
        let key = remote_key("https://github.com/Ann/App.git");
        for same in [
            "https://github.com/ann/app",
            "https://user:token@github.com/ann/app.git/",
            "git@github.com:ann/app.git",
            "ssh://git@github.com/ann/app.git",
        ] {
            assert_eq!(remote_key(same), key, "{same}");
        }
        assert_ne!(remote_key("https://github.com/ann/other"), key);
        assert_ne!(remote_key("https://gitlab.com/ann/app"), key);
        assert!(!key.contains("github") && !key.contains("ann"), "{key}");
        let parsed = config("[remote \"origin\"]\nurl = https://u:hunter2@example.com/a.git\n");
        assert_eq!(parsed.remote_keys.len(), 1);
        assert!(!format!("{parsed:?}").contains("hunter2"));
    }

    #[test]
    fn a_single_branch_clone_has_its_tracking_ref() {
        let parsed = config(concat!(
            "[remote \"origin\"]\nurl = x:y\nfetch = +refs/heads/main:refs/remotes/origin/main\n",
            "[branch \"main\"]\nremote = origin\nmerge = refs/heads/main\n",
        ));
        assert_eq!(parsed.upstream("main").as_deref(), Some("refs/remotes/origin/main"));
    }
}
