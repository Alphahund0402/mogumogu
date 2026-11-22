//! What is still open in a project, from the facts of its last tidy-up
//! report: is the work also somewhere else, and does the project have what a
//! finished project usually has. A check can only add a reason to look.
//! "Done" says that the data read show nothing open; it never means that a
//! project can go, and nothing here makes anything recoverable.
use super::{ProjectTidy, format_bytes};
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    pub enum CheckState {
        Done => "done",
        Open => "open",
        /// The data needed could not be read; never counted as done.
        Unknown => "unknown",
    }
}

impl CheckState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Done => "erledigt",
            Self::Open => "offen",
            Self::Unknown => "unbekannt",
        }
    }
}

string_enum! {
    pub enum CheckGroup {
        /// Would the work survive losing this folder?
        Saved => "saved",
        /// What a finished project usually has.
        Complete => "complete",
    }
}

impl CheckGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Saved => "Gesichert",
            Self::Complete => "Vollständig",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectCheck {
    pub group: CheckGroup,
    pub state: CheckState,
    pub title: String,
    pub detail: String,
}

/// Branches and remotes of a repository, as its own files show them. "On the
/// remote" always means: as this repository last saw it (fetch or push).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitRefs {
    /// The branch checked out; `None` on a detached `HEAD`.
    pub branch: Option<String>,
    pub detached: bool,
    pub remotes: u32,
    /// Remotes whose address is a path: possibly this very disk.
    pub local_remotes: u32,
    /// Branches saved on such a remote and on no other.
    pub on_local_remote: u32,
    pub branches: u32,
    /// Branches whose tip the remote was never seen to have.
    pub unpushed: u32,
    pub unpushed_names: Vec<String>,
    /// Branches without a counterpart on any remote.
    pub local_only: u32,
    pub local_only_names: Vec<String>,
    pub stashes: u32,
    /// Linked work trees of this repository; often outside the project.
    pub worktrees: u32,
    /// One-way keys of the remotes and the current commit, to recognise
    /// copies among the approved projects. Never leaves the process.
    #[serde(skip)]
    pub keys: Vec<String>,
    pub last_commit: Option<Timestamp>,
}

/// The work tree compared with Git's index, from file metadata only.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitTree {
    /// The index proves that changes wait for a commit. False proves nothing:
    /// without the commit objects a staged change can stay invisible.
    pub staged: bool,
    /// Versioned files whose size or last write differ from the index. The
    /// content is not read, so a file only touched counts as well.
    pub changed: u32,
    pub deleted: u32,
    pub conflicts: u32,
    /// Example paths of changed and deleted files.
    pub samples: Vec<String>,
    /// Files Git does not list, outside recoverable folders and `.git`.
    /// With or without `.gitignore`: what exists nowhere else either way.
    pub outside_files: u64,
    pub outside_bytes: u64,
    /// The largest of those files.
    pub outside_samples: Vec<String>,
}

/// What a project's Git repository shows. A part that could not be read
/// stays `None` and names its reason in `problems`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitFacts {
    pub refs: Option<GitRefs>,
    pub tree: Option<GitTree>,
    pub problems: Vec<String>,
    /// Further repositories inside the project; not looked into.
    pub nested: u32,
}

/// What the project's own folder shows by name.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectMarkers {
    pub readme: bool,
    pub license: bool,
    pub tests: bool,
    pub ci: bool,
    pub manifest: bool,
    pub lockfile: bool,
    /// Mercurial or Subversion, which this tool does not read.
    pub other_vcs: bool,
}

/// Names shown per check.
pub const CHECK_NAMES: usize = 5;

fn check(group: CheckGroup, state: CheckState, title: &str, detail: impl Into<String>) -> ProjectCheck {
    ProjectCheck { group, state, title: title.into(), detail: detail.into() }
}

fn counted(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `: a, b +3`, or nothing without names.
fn examples(names: &[String], total: u64) -> String {
    if names.is_empty() {
        return String::new();
    }
    let shown = names.iter().map(|name| name.replace('/', "\\")).collect::<Vec<_>>().join(", ");
    match total.saturating_sub(names.len() as u64) {
        0 => format!(": {shown}"),
        more => format!(": {shown} +{more}"),
    }
}

fn reason(git: &GitFacts) -> String {
    if git.problems.is_empty() { "Nicht lesbar.".into() } else { git.problems.join(" · ") }
}

fn branch_checks(checks: &mut Vec<ProjectCheck>, refs: &GitRefs) {
    use CheckGroup::Saved;
    use CheckState::{Done, Open};
    if refs.branches == 0 && !refs.detached {
        checks.push(check(Saved, Open, "Commits", "Noch kein Commit: Alles liegt nur als Dateien hier."));
    }
    if refs.detached {
        checks.push(check(
            Saved,
            Open,
            "Branch",
            "HEAD zeigt direkt auf einen Commit. Commits von hier aus hängen an keinem Branch.",
        ));
    }
    if refs.remotes == 0 {
        checks.push(check(Saved, Open, "Remote", "Kein Remote eingetragen: Alle Commits liegen nur hier."));
    } else {
        if refs.local_remotes == refs.remotes {
            checks.push(check(
                Saved,
                CheckState::Unknown,
                "Remote",
                "Nur Remotes auf einem lokalen Pfad: Ob sie auf einem anderen Datenträger liegen, ist unbekannt.",
            ));
        } else {
            checks.push(check(
                Saved,
                Done,
                "Remote",
                counted(refs.remotes.into(), "Remote eingetragen", "Remotes eingetragen"),
            ));
        }
        if refs.unpushed + refs.local_only > 0 {
            let mut parts = Vec::new();
            if refs.unpushed > 0 {
                parts.push(format!(
                    "{} mit Commits, die der Remote laut letztem Abruf nicht hat{}",
                    counted(refs.unpushed.into(), "Branch", "Branches"),
                    examples(&refs.unpushed_names, refs.unpushed.into())
                ));
            }
            if refs.local_only > 0 {
                parts.push(format!(
                    "{} ohne Gegenstück auf einem Remote{}",
                    counted(refs.local_only.into(), "Branch", "Branches"),
                    examples(&refs.local_only_names, refs.local_only.into())
                ));
            }
            checks.push(check(Saved, Open, "Commits auf dem Remote", parts.join(" · ")));
        } else if refs.on_local_remote > 0 {
            checks.push(check(
                Saved,
                CheckState::Unknown,
                "Commits auf dem Remote",
                format!(
                    "{} nur auf einem Remote mit lokalem Pfad.",
                    counted(refs.on_local_remote.into(), "Branch liegt", "Branches liegen")
                ),
            ));
        } else if refs.branches > 0 {
            checks.push(check(
                Saved,
                Done,
                "Commits auf dem Remote",
                format!(
                    "{}: nichts Neues gegenüber dem letzten Abruf.",
                    counted(refs.branches.into(), "Branch", "Branches")
                ),
            ));
        }
    }
    if refs.worktrees > 0 {
        checks.push(check(
            Saved,
            CheckState::Unknown,
            "Weitere Arbeitskopien",
            format!(
                "{}; deren Änderungen sind nicht geprüft.",
                counted(refs.worktrees.into(), "verknüpfte Arbeitskopie", "verknüpfte Arbeitskopien")
            ),
        ));
    }
    if refs.stashes > 0 {
        checks.push(check(
            Saved,
            Open,
            "Zurückgelegte Änderungen",
            format!("{} liegen nur hier.", counted(refs.stashes.into(), "Stash-Eintrag", "Stash-Einträge")),
        ));
    }
}

fn tree_checks(checks: &mut Vec<ProjectCheck>, tree: &GitTree) {
    use CheckGroup::Saved;
    use CheckState::{Done, Open};
    let mut parts = Vec::new();
    if tree.conflicts > 0 {
        parts.push(counted(tree.conflicts.into(), "Datei mit Konflikt", "Dateien mit Konflikt"));
    }
    if tree.changed > 0 {
        parts.push(counted(
            tree.changed.into(),
            "Datei geändert oder neu gespeichert",
            "Dateien geändert oder neu gespeichert",
        ));
    }
    if tree.deleted > 0 {
        parts.push(counted(tree.deleted.into(), "Datei fehlt", "Dateien fehlen"));
    }
    checks.push(if tree.staged {
        check(Saved, Open, "Vorgemerkte Änderungen", "Der Index belegt Änderungen, die auf einen Commit warten.")
    } else {
        check(
            Saved,
            CheckState::Unknown,
            "Vorgemerkte Änderungen",
            "Nichts belegt; ausschließen ließe es sich nur mit den Commit-Objekten, die mogumogu nicht liest.",
        )
    });
    if parts.is_empty() {
        checks.push(check(Saved, Done, "Änderungen", "Keine versionierte Datei weicht vom Index ab."));
    } else {
        let total = u64::from(tree.changed) + u64::from(tree.deleted);
        checks.push(check(
            Saved,
            Open,
            "Änderungen",
            format!("{}{}", parts.join(" · "), examples(&tree.samples, total)),
        ));
    }
    if tree.outside_files > 0 {
        let size = format_bytes(i64::try_from(tree.outside_bytes).ok());
        checks.push(check(
            Saved,
            Open,
            "Nur auf diesem Rechner",
            format!(
                "{}, {size}, die Git nicht führt und die nicht als wiederherstellbar belegt sind{}",
                counted(tree.outside_files, "Datei", "Dateien"),
                examples(&tree.outside_samples, tree.outside_files)
            ),
        ));
    } else {
        checks.push(check(
            Saved,
            Done,
            "Nur auf diesem Rechner",
            "Keine Datei außerhalb von Git und wiederherstellbaren Ordnern.",
        ));
    }
}

fn git_checks(checks: &mut Vec<ProjectCheck>, git: &GitFacts) {
    use CheckGroup::Saved;
    use CheckState::Unknown;
    match &git.refs {
        Some(refs) => branch_checks(checks, refs),
        None => checks.push(check(Saved, Unknown, "Commits und Branches", reason(git))),
    }
    match &git.tree {
        Some(tree) => tree_checks(checks, tree),
        None => checks.push(check(Saved, Unknown, "Änderungen", reason(git))),
    }
    if git.nested > 0 {
        checks.push(check(
            Saved,
            Unknown,
            "Weitere Repositories",
            format!(
                "{} im Ordner; nicht einzeln geprüft.",
                counted(git.nested.into(), "Git-Repository", "Git-Repositories")
            ),
        ));
    }
}

/// The checks of one project, in a fixed order. `heavy_test_data` is the size
/// of test data that stand out, if they do.
pub fn project_checks(
    git: Option<&GitFacts>,
    markers: &ProjectMarkers,
    heavy_test_data: Option<u64>,
) -> Vec<ProjectCheck> {
    use CheckGroup::{Complete, Saved};
    use CheckState::{Done, Open, Unknown};
    let mut checks = Vec::new();
    match git {
        Some(git) => git_checks(&mut checks, git),
        None if markers.other_vcs => {
            checks.push(check(
                Saved,
                Unknown,
                "Versionsverwaltung",
                "Mercurial oder Subversion; deren Stand liest mogumogu nicht.",
            ));
        }
        None => checks.push(check(
            Saved,
            Open,
            "Versionsverwaltung",
            "Kein Git-Repository: Was hier liegt, gibt es womöglich nur hier.",
        )),
    }
    let marker = |present: bool, title: &str, missing: &str| {
        check(Complete, if present { Done } else { Open }, title, if present { "Vorhanden." } else { missing })
    };
    checks.push(marker(markers.readme, "README", "Keine README im Projektordner."));
    checks.push(marker(markers.license, "Lizenz", "Keine Lizenzdatei im Projektordner."));
    checks.push(marker(markers.tests, "Tests", "Weder Testordner noch Testdateien nach Namen gefunden."));
    checks.push(marker(markers.ci, "CI", "Keine CI-Konfiguration gefunden."));
    if markers.manifest {
        checks.push(marker(
            markers.lockfile,
            "Lockfile",
            "Paketmanifest ohne Lockfile: Versionen lassen sich nicht genau wiederherstellen.",
        ));
    }
    if let Some(bytes) = heavy_test_data {
        checks.push(check(
            Complete,
            Open,
            "Testdaten",
            format!("{} Testdaten: behalten oder entfernen entscheidest du.", format_bytes(i64::try_from(bytes).ok())),
        ));
    }
    checks
}

const COPIES: &str = "Weitere Kopien";

/// Names the other measured projects that share a remote or the current
/// commit with each project. It says nothing about which copy is current, so
/// it can never count as done.
pub fn mark_copies(projects: &mut [ProjectTidy]) {
    let keys: Vec<(String, Vec<String>)> = projects
        .iter()
        .map(|p| {
            (p.name.clone(), p.git.as_ref().and_then(|g| g.refs.as_ref()).map_or_else(Vec::new, |r| r.keys.clone()))
        })
        .collect();
    for (index, project) in projects.iter_mut().enumerate() {
        project.checks.retain(|c| c.title != COPIES);
        let mine = &keys[index].1;
        let others: Vec<String> = keys
            .iter()
            .enumerate()
            .filter(|(other, (_, theirs))| *other != index && theirs.iter().any(|key| mine.contains(key)))
            .map(|(_, (name, _))| name.clone())
            .collect();
        if others.is_empty() {
            continue;
        }
        let shown: Vec<String> = others.iter().take(CHECK_NAMES).cloned().collect();
        let detail = format!(
            "Gleicher Remote oder Commit auch in{}. Welche Kopie aktuell ist, ist unbekannt.",
            examples(&shown, others.len() as u64)
        );
        let at = project.checks.iter().position(|c| c.group != CheckGroup::Saved).unwrap_or(project.checks.len());
        project.checks.insert(at, check(CheckGroup::Saved, CheckState::Unknown, COPIES, detail));
    }
}

/// `3 offen · 1 unbekannt · 8 erledigt`.
pub fn checks_summary(checks: &[ProjectCheck]) -> String {
    [CheckState::Open, CheckState::Unknown, CheckState::Done]
        .iter()
        .map(|state| (checks.iter().filter(|c| c.state == *state).count(), state.label()))
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean_refs() -> GitRefs {
        GitRefs { branch: Some("main".into()), remotes: 1, branches: 2, ..GitRefs::default() }
    }

    fn clean_git() -> GitFacts {
        GitFacts { refs: Some(clean_refs()), tree: Some(GitTree::default()), ..GitFacts::default() }
    }

    fn state<'a>(checks: &'a [ProjectCheck], title: &str) -> &'a ProjectCheck {
        checks.iter().find(|c| c.title == title).unwrap_or_else(|| panic!("{title} in {checks:?}"))
    }

    fn all_markers() -> ProjectMarkers {
        ProjectMarkers {
            readme: true,
            license: true,
            tests: true,
            ci: true,
            manifest: true,
            lockfile: true,
            other_vcs: false,
        }
    }

    #[test]
    fn a_clean_repository_has_nothing_open_but_staging_stays_unknown() {
        let checks = project_checks(Some(&clean_git()), &all_markers(), None);
        assert!(checks.iter().all(|c| c.state != CheckState::Open), "{checks:?}");
        let unknown: Vec<&str> =
            checks.iter().filter(|c| c.state == CheckState::Unknown).map(|c| c.title.as_str()).collect();
        assert_eq!(unknown, ["Vorgemerkte Änderungen"], "the index alone cannot rule staged work out");
        assert_eq!(checks_summary(&checks), format!("1 unbekannt · {} erledigt", checks.len() - 1));
    }

    #[test]
    fn a_remote_on_a_path_saves_nothing_for_certain() {
        let mut git = clean_git();
        let refs = git.refs.as_mut().unwrap();
        (refs.local_remotes, refs.on_local_remote) = (1, 2);
        let checks = project_checks(Some(&git), &all_markers(), None);
        assert_eq!(state(&checks, "Remote").state, CheckState::Unknown);
        assert_eq!(state(&checks, "Commits auf dem Remote").state, CheckState::Unknown);
        // Work missing from it stays open, whatever kind of remote it is.
        git.refs.as_mut().unwrap().unpushed = 1;
        let checks = project_checks(Some(&git), &all_markers(), None);
        assert_eq!(state(&checks, "Commits auf dem Remote").state, CheckState::Open);
    }

    #[test]
    fn each_kind_of_unsaved_work_is_open() {
        let mut git = clean_git();
        let refs = git.refs.as_mut().unwrap();
        refs.unpushed = 7;
        refs.unpushed_names = vec!["main".into(), "dev".into()];
        refs.local_only = 1;
        refs.local_only_names = vec!["spike".into()];
        refs.stashes = 2;
        let tree = git.tree.as_mut().unwrap();
        tree.changed = 3;
        tree.deleted = 1;
        tree.conflicts = 1;
        tree.staged = true;
        tree.samples = vec!["src/a.rs".into()];
        tree.outside_files = 12;
        tree.outside_bytes = 2048;
        tree.outside_samples = vec![".env".into()];
        let checks = project_checks(Some(&git), &all_markers(), None);
        let pushed = state(&checks, "Commits auf dem Remote");
        assert_eq!(pushed.state, CheckState::Open);
        assert!(pushed.detail.contains("7 Branches") && pushed.detail.contains("main, dev +5"), "{}", pushed.detail);
        assert!(pushed.detail.contains("1 Branch ohne") && pushed.detail.contains("spike"), "{}", pushed.detail);
        assert_eq!(state(&checks, "Zurückgelegte Änderungen").state, CheckState::Open);
        let changes = state(&checks, "Änderungen");
        assert_eq!(changes.state, CheckState::Open);
        for part in ["Konflikt", "3 Dateien geändert", "1 Datei fehlt", r"src\a.rs"] {
            assert!(changes.detail.contains(part), "{part}: {}", changes.detail);
        }
        assert_eq!(state(&checks, "Vorgemerkte Änderungen").state, CheckState::Open);
        let outside = state(&checks, "Nur auf diesem Rechner");
        assert_eq!(outside.state, CheckState::Open);
        assert!(outside.detail.contains("12 Dateien") && outside.detail.contains(".env +11"), "{}", outside.detail);
    }

    #[test]
    fn no_remote_or_no_commit_is_open_and_says_why() {
        let mut git = clean_git();
        git.refs.as_mut().unwrap().remotes = 0;
        let checks = project_checks(Some(&git), &all_markers(), None);
        assert_eq!(state(&checks, "Remote").state, CheckState::Open);
        assert!(!checks.iter().any(|c| c.title == "Commits auf dem Remote"), "nothing to compare with");
        git.refs.as_mut().unwrap().branches = 0;
        assert_eq!(state(&project_checks(Some(&git), &all_markers(), None), "Commits").state, CheckState::Open);
    }

    #[test]
    fn what_could_not_be_read_is_never_done() {
        let git = GitFacts { problems: vec!["Reftable".into()], ..GitFacts::default() };
        let checks = project_checks(Some(&git), &all_markers(), None);
        for title in ["Commits und Branches", "Änderungen"] {
            let found = state(&checks, title);
            assert_eq!(found.state, CheckState::Unknown);
            assert!(found.detail.contains("Reftable"));
        }
    }

    fn measured(name: &str, keys: &[&str]) -> ProjectTidy {
        let refs = GitRefs { keys: keys.iter().map(|k| k.to_string()).collect(), ..clean_refs() };
        ProjectTidy {
            project_id: 1,
            name: name.into(),
            path: String::new(),
            total: None,
            regenerable: 0,
            regenerable_allocated: 0,
            review: 0,
            vcs: 0,
            test_data: 0,
            agent: 0,
            kept: Vec::new(),
            runs: Vec::new(),
            growth: None,
            git: Some(GitFacts { refs: Some(refs), ..GitFacts::default() }),
            checks: vec![
                check(CheckGroup::Saved, CheckState::Done, "Remote", ""),
                check(CheckGroup::Complete, CheckState::Done, "README", ""),
            ],
            items: Vec::new(),
            last_change: None,
            complete: true,
            note: String::new(),
            checked_at: 0,
        }
    }

    #[test]
    fn projects_sharing_a_remote_or_commit_name_each_other_and_stay_unknown() {
        let mut projects = vec![
            measured("a", &["remote:1"]),
            measured("b", &["remote:1", "commit:9"]),
            measured("c", &["commit:9"]),
            measured("d", &["remote:2"]),
        ];
        mark_copies(&mut projects);
        let copies = |p: &ProjectTidy| p.checks.iter().find(|c| c.title == COPIES).cloned();
        let of_a = copies(&projects[0]).expect("a shares a remote with b");
        assert_eq!(of_a.state, CheckState::Unknown, "a copy says nothing about which is current");
        assert!(of_a.detail.contains(": b."), "{}", of_a.detail);
        assert!(copies(&projects[1]).unwrap().detail.contains("a, c"));
        assert!(copies(&projects[3]).is_none());
        assert_eq!(projects[0].checks[1].title, COPIES, "with the other saved-work points, before the rest");

        // A later report replaces the note; a key that has gone removes it.
        projects[1] = measured("b", &["remote:3"]);
        mark_copies(&mut projects);
        assert!(copies(&projects[0]).is_none());
        assert_eq!(projects[1].checks.iter().filter(|c| c.title == COPIES).count(), 0);
    }

    #[test]
    fn the_keys_of_copies_never_leave_the_process() {
        let refs = GitRefs { keys: vec!["remote:abc".into()], ..GitRefs::default() };
        assert!(!serde_json::to_string(&refs).unwrap().contains("remote:abc"));
    }

    #[test]
    fn a_folder_without_git_has_nothing_saved() {
        let checks = project_checks(None, &all_markers(), None);
        assert_eq!(state(&checks, "Versionsverwaltung").state, CheckState::Open);
        let other = ProjectMarkers { other_vcs: true, ..all_markers() };
        assert_eq!(state(&project_checks(None, &other, None), "Versionsverwaltung").state, CheckState::Unknown);
    }

    #[test]
    fn nested_repositories_and_linked_work_trees_stay_unknown() {
        let mut git = GitFacts { nested: 2, ..clean_git() };
        git.refs.as_mut().unwrap().worktrees = 1;
        let checks = project_checks(Some(&git), &all_markers(), None);
        assert_eq!(state(&checks, "Weitere Repositories").state, CheckState::Unknown);
        assert_eq!(state(&checks, "Weitere Arbeitskopien").state, CheckState::Unknown);
    }

    #[test]
    fn missing_project_parts_and_heavy_test_data_are_listed() {
        let bare = ProjectMarkers { manifest: true, ..ProjectMarkers::default() };
        let checks = project_checks(Some(&clean_git()), &bare, Some(2 << 30));
        for title in ["README", "Lizenz", "Tests", "CI", "Lockfile", "Testdaten"] {
            assert_eq!(state(&checks, title).state, CheckState::Open, "{title}");
        }
        let no_manifest = project_checks(Some(&clean_git()), &ProjectMarkers::default(), None);
        assert!(!no_manifest.iter().any(|c| c.title == "Lockfile" || c.title == "Testdaten"));
    }

    #[test]
    fn the_summary_names_open_points_first_and_leaves_out_empty_states() {
        let checks = vec![
            check(CheckGroup::Saved, CheckState::Done, "a", ""),
            check(CheckGroup::Saved, CheckState::Open, "b", ""),
            check(CheckGroup::Saved, CheckState::Done, "c", ""),
        ];
        assert_eq!(checks_summary(&checks), "1 offen · 2 erledigt");
        assert_eq!(checks_summary(&[]), "");
    }
}
