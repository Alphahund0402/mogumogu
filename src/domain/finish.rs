//! What finishing a project would mean, from a fresh tidy-up report: is the
//! work saved, what stays without asking, what the person decides and what
//! can go to the recycle bin through a plan (docs/LEBENSZYKLUS.md, step 1).
//! Read only: it decides nothing, changes no protection and makes nothing
//! recoverable that the report does not already call recoverable.
use super::{
    CheckGroup, CheckState, GROWTH_WINDOW, Hold, ProjectCheck, ProjectTidy, RunBrief, TidyClass, TidyItem, TidyPoint,
    fold_case, holding,
};
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    /// Where a project stands. No row means active; finishing changes no protection.
    pub enum Lifecycle {
        Active => "active",
        Finished => "finished",
    }
}

/// One change of a project's state, newest first in a history.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LifecycleEvent {
    pub state: Lifecycle,
    pub at: Timestamp,
}

/// One folder of the report.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FinishItem {
    /// Relative to the project, `/`-separated.
    pub rel_path: String,
    pub label: String,
    pub bytes: u64,
    /// Why the folder is in this part: its evidence, or why it is held.
    pub evidence: String,
    pub modified_at: Option<Timestamp>,
    /// Growth since [`FinishReport::growth_since`]. `None` when that report
    /// did not list the folder among its largest entries: unknown, not zero.
    pub grown: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FinishReport {
    pub project_id: i64,
    pub name: String,
    pub path: String,
    pub checked_at: Timestamp,
    /// The walk read everything; otherwise sizes are lower bounds and the
    /// project's own content is unknown.
    pub complete: bool,
    pub note: String,
    /// The oldest complete saved report of the growth window that growth is
    /// measured against; `None` without an earlier report: no trend yet.
    pub growth_since: Option<Timestamp>,
    /// Is the work saved? Open and unknown points first; unknown is never done.
    pub saved: Vec<ProjectCheck>,
    /// What a finished project usually has, for information.
    pub completeness: Vec<ProjectCheck>,
    /// Stays without asking: the project's own content outside the folders
    /// below and outside version control. Unknown for an incomplete walk.
    pub own: Option<u64>,
    /// Version control data, kept.
    pub vcs: u64,
    /// Kept by the person, or held by a run, a monitored database or Docker.
    pub held: Vec<FinishItem>,
    /// Kept folders the report did not list among its largest entries.
    pub held_unmeasured: Vec<String>,
    /// The person decides: possibly needed, never part of a plan.
    pub decide: Vec<FinishItem>,
    /// Size of everything under "Prüfen" that is not held, listed or not.
    pub decide_bytes: u64,
    /// Recoverable with evidence; goes only through a recycle-bin plan.
    pub recoverable: Vec<FinishItem>,
    pub recoverable_bytes: u64,
    pub recoverable_allocated: u64,
    /// Registered runs of this project the person has not released.
    pub runs: Vec<RunBrief>,
    /// Newest file outside outputs, caches and version control.
    pub last_change: Option<Timestamp>,
    /// When the project was finished; `None` while it is active.
    pub finished_at: Option<Timestamp>,
    /// What finishing confirms, see [`FinishReport::decision_fingerprint`].
    pub fingerprint: String,
}

impl FinishReport {
    /// Open or unknown points about whether the work is saved.
    pub fn unsaved(&self) -> usize {
        self.saved.iter().filter(|c| c.state != CheckState::Done).count()
    }

    /// A finished project with a file newer than the finish: worth a look,
    /// and maybe a reactivation. New commits alone do not show here, since
    /// mogumogu keeps no Git state in its database.
    pub fn changed_since_finish(&self) -> bool {
        self.finished_at.zip(self.last_change).is_some_and(|(finished, changed)| changed > finished)
    }

    /// What the person confirms when finishing: the points about saved work,
    /// which folders stay and why, which they decide and which can go. Sizes
    /// and times stay out, so a growing log does not ask for a new report; a
    /// changed point or folder does.
    pub fn decision_fingerprint(&self) -> String {
        let mut lines = vec![format!("project\t{}\t{}\t{}", self.project_id, fold_case(&self.path), self.complete)];
        lines.extend(self.saved.iter().map(|c| format!("saved\t{}\t{}\t{}", c.state.as_str(), c.title, c.detail)));
        let mut folders: Vec<String> = self
            .held
            .iter()
            .map(|i| format!("held\t{}\t{}", fold_case(&i.rel_path), i.evidence))
            .chain(self.held_unmeasured.iter().map(|rel| format!("held\t{}", fold_case(rel))))
            .chain(self.decide.iter().map(|i| format!("decide\t{}\t{}", fold_case(&i.rel_path), i.label)))
            .chain(self.recoverable.iter().map(|i| format!("recoverable\t{}\t{}", fold_case(&i.rel_path), i.label)))
            .chain(self.runs.iter().map(|r| format!("run\t{}", r.id)))
            .collect();
        folders.sort();
        lines.extend(folders);
        crate::sha256::hex(&crate::sha256::Sha256::digest(lines.join("\n")))
    }
}

/// A size for the report: a few kilobytes are not "0 MB", something is there.
pub fn report_size(bytes: u64) -> String {
    match bytes {
        1..1_000_000 => "unter 1 MB".into(),
        _ => super::format_bytes(i64::try_from(bytes).ok()),
    }
}

/// How a folder grew since the report named by `since` (a display date).
pub fn growth_text(grown: Option<i64>, since: Option<&str>) -> String {
    match (grown, since) {
        (_, None) => "noch kein früherer Bericht".into(),
        (None, Some(_)) => "Wachstum unbekannt".into(),
        (Some(0), Some(since)) => format!("unverändert seit {since}"),
        (Some(g), Some(since)) if g > 0 => format!("+{} seit {since}", report_size(g.unsigned_abs())),
        (Some(g), Some(since)) => format!("−{} seit {since}", report_size(g.unsigned_abs())),
    }
}

/// Builds the report from a fresh tidy-up report of the project, its saved
/// reports (for growth), the holds and its unreleased runs.
pub fn finish_report(
    tidy: &ProjectTidy,
    history: &[TidyPoint],
    holds: &[Hold],
    runs: Vec<RunBrief>,
    now: Timestamp,
) -> FinishReport {
    let rank = |state: CheckState| match state {
        CheckState::Open => 0,
        CheckState::Unknown => 1,
        CheckState::Done => 2,
    };
    let mut saved: Vec<ProjectCheck> = tidy.checks.iter().filter(|c| c.group == CheckGroup::Saved).cloned().collect();
    saved.sort_by_key(|c| rank(c.state));
    let completeness = tidy.checks.iter().filter(|c| c.group == CheckGroup::Complete).cloned().collect();

    let since = earliest_in_window(history, tidy.checked_at, now);
    let item = |source: &TidyItem, evidence: String| FinishItem {
        rel_path: source.rel_path.clone(),
        label: source.label.clone(),
        bytes: source.bytes,
        evidence,
        modified_at: source.modified_at,
        grown: since.and_then(|point| grown(point, source)),
    };
    let (mut held, mut decide, mut recoverable) = (Vec::new(), Vec::new(), Vec::new());
    for source in &tidy.items {
        match (source.class, held_reason(tidy, holds, &source.rel_path)) {
            (TidyClass::Regenerable, _) => recoverable.push(item(source, source.evidence.clone())),
            (TidyClass::Review, Some(reason)) => held.push(item(source, reason)),
            (TidyClass::Review, None) => decide.push(item(source, source.evidence.clone())),
        }
    }
    let held_unmeasured =
        tidy.kept.iter().filter(|kept| !held.iter().any(|h| related(&h.rel_path, kept))).cloned().collect();
    let held_bytes: u64 = held.iter().map(|h| h.bytes).sum();
    // Never less than what the part lists, whatever the totals say.
    let decide_bytes = tidy.review.saturating_sub(held_bytes).max(decide.iter().map(|d: &FinishItem| d.bytes).sum());
    let mut report = FinishReport {
        project_id: tidy.project_id,
        name: tidy.name.clone(),
        path: tidy.path.clone(),
        checked_at: tidy.checked_at,
        complete: tidy.complete,
        note: tidy.note.clone(),
        growth_since: since.map(|point| point.checked_at),
        saved,
        completeness,
        own: tidy.own_content().map(|own| own.saturating_sub(tidy.review)),
        vcs: tidy.vcs,
        held,
        held_unmeasured,
        decide,
        decide_bytes,
        recoverable,
        recoverable_bytes: tidy.regenerable,
        recoverable_allocated: tidy.regenerable_allocated,
        runs,
        last_change: tidy.last_change,
        finished_at: None,
        fingerprint: String::new(),
    };
    report.fingerprint = report.decision_fingerprint();
    report
}

/// Why a folder stays: a keep of the person or a hold at it, in it or
/// around it, as a plan would see it.
fn held_reason(tidy: &ProjectTidy, holds: &[Hold], rel_path: &str) -> Option<String> {
    if tidy.kept.iter().any(|kept| related(rel_path, kept)) {
        return Some("von dir behalten".into());
    }
    let location = format!("{}\\{}", tidy.path.trim_end_matches('\\'), rel_path.replace('/', "\\"));
    holding(holds, &location).map(|hold| hold.reason.clone())
}

/// The same folder, one inside the other, compared as Windows compares names.
fn related(a: &str, b: &str) -> bool {
    let (a, b) = (fold_case(a.trim_end_matches('/')), fold_case(b.trim_end_matches('/')));
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

/// The oldest complete saved report in the growth window, if it is older
/// than the report itself: one report is no trend.
fn earliest_in_window(history: &[TidyPoint], checked_at: Timestamp, now: Timestamp) -> Option<&TidyPoint> {
    history
        .iter()
        .filter(|p| p.complete && p.checked_at < checked_at && now - p.checked_at <= GROWTH_WINDOW)
        .min_by_key(|p| p.checked_at)
}

fn grown(since: &TidyPoint, item: &TidyItem) -> Option<i64> {
    let (_, _, before) = since.items.iter().find(|(rel, ..)| fold_case(rel) == fold_case(&item.rel_path))?;
    let signed = |bytes: u64| i64::try_from(bytes).unwrap_or(i64::MAX);
    Some(signed(item.bytes).saturating_sub(signed(*before)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::HoldKind;

    const DAY: Timestamp = 86_400;

    fn item(rel: &str, class: TidyClass, bytes: u64) -> TidyItem {
        TidyItem {
            rel_path: rel.into(),
            class,
            label: "Ordner".into(),
            bytes,
            allocated: bytes,
            evidence: format!("Beleg {rel}"),
            modified_at: Some(10),
        }
    }

    fn check(group: CheckGroup, state: CheckState, title: &str) -> ProjectCheck {
        ProjectCheck { group, state, title: title.into(), detail: String::new() }
    }

    fn tidy() -> ProjectTidy {
        ProjectTidy {
            project_id: 7,
            name: "shop".into(),
            path: r"C:\dev\shop".into(),
            total: Some(1_000),
            regenerable: 300,
            regenerable_allocated: 280,
            review: 500,
            vcs: 50,
            test_data: 400,
            agent: 0,
            kept: vec!["fixtures/golden".into(), "notes".into()],
            runs: Vec::new(),
            growth: None,
            git: None,
            checks: vec![
                check(CheckGroup::Saved, CheckState::Done, "Remote"),
                check(CheckGroup::Complete, CheckState::Open, "README"),
                check(CheckGroup::Saved, CheckState::Unknown, "Änderungen"),
                check(CheckGroup::Saved, CheckState::Open, "Commits auf dem Remote"),
            ],
            items: vec![
                item("node_modules", TidyClass::Regenerable, 300),
                item("tests/data", TidyClass::Review, 300),
                item("Fixtures", TidyClass::Review, 120),
                item("db", TidyClass::Review, 80),
            ],
            last_change: Some(10),
            complete: true,
            note: String::new(),
            checked_at: 100 * DAY,
        }
    }

    fn docker(location: &str) -> Hold {
        Hold { kind: HoldKind::Docker, location: location.into(), reason: "Container web bindet ein".into() }
    }

    #[test]
    fn each_folder_lands_in_exactly_one_part_and_holds_win() {
        let report = finish_report(&tidy(), &[], &[docker(r"c:\DEV\shop\db")], Vec::new(), 100 * DAY);
        let paths = |items: &[FinishItem]| items.iter().map(|i| i.rel_path.clone()).collect::<Vec<_>>();
        assert_eq!(paths(&report.recoverable), ["node_modules"]);
        assert_eq!(paths(&report.decide), ["tests/data"]);
        // A folder holding a kept one stays, compared without case; Docker holds by real place.
        assert_eq!(paths(&report.held), ["Fixtures", "db"]);
        assert_eq!(report.held[0].evidence, "von dir behalten");
        assert_eq!(report.held[1].evidence, "Container web bindet ein");
        assert_eq!(report.held_unmeasured, ["notes"]);
        assert_eq!((report.decide_bytes, report.recoverable_bytes, report.recoverable_allocated), (300, 300, 280));
        // Own content: total minus recoverable, version control and everything under review.
        assert_eq!(report.own, Some(150));
    }

    #[test]
    fn unsaved_work_comes_first_and_unknown_counts_as_unsaved() {
        let report = finish_report(&tidy(), &[], &[], Vec::new(), 100 * DAY);
        let order: Vec<&str> = report.saved.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(order, ["Commits auf dem Remote", "Änderungen", "Remote"]);
        assert_eq!(report.unsaved(), 2);
        assert_eq!(report.completeness.len(), 1);
    }

    #[test]
    fn the_fingerprint_follows_points_and_folders_but_not_sizes_or_times() {
        let report = |tidy: ProjectTidy| finish_report(&tidy, &[], &[], Vec::new(), 100 * DAY).fingerprint;
        let base = report(tidy());
        assert_eq!(base.len(), 64);
        let mut grown = tidy();
        grown.items[1].bytes *= 3;
        grown.review *= 3;
        grown.checked_at += DAY;
        grown.items.reverse(); // largest first changes with the sizes
        assert_eq!(report(grown), base, "a growing log asks for no new report");
        let mut pushed = tidy();
        pushed.checks[3].state = CheckState::Done;
        assert_ne!(report(pushed), base, "a changed point is a new decision");
        let mut new_folder = tidy();
        new_folder.items.push(item("logs", TidyClass::Review, 10));
        assert_ne!(report(new_folder), base, "a new folder is a new decision");
        let mut unkept = tidy();
        unkept.kept.retain(|k| k != "fixtures/golden");
        assert_ne!(report(unkept), base, "a folder that no longer stays is a new decision");
    }

    #[test]
    fn a_file_newer_than_the_finish_is_a_change_since() {
        let mut report = finish_report(&tidy(), &[], &[], Vec::new(), 100 * DAY);
        assert!(!report.changed_since_finish(), "active projects have no finish to compare with");
        report.finished_at = Some(5);
        assert!(report.changed_since_finish(), "last change 10 is after 5");
        report.finished_at = Some(10);
        assert!(!report.changed_since_finish());
    }

    #[test]
    fn the_total_to_decide_is_never_less_than_what_it_lists() {
        let report = finish_report(&ProjectTidy { review: 100, ..tidy() }, &[], &[], Vec::new(), 100 * DAY);
        assert_eq!(report.decide_bytes, 380, "tests/data and db alone are 380");
    }

    #[test]
    fn an_incomplete_walk_leaves_the_own_content_unknown() {
        let report = finish_report(&ProjectTidy { total: None, complete: false, ..tidy() }, &[], &[], Vec::new(), 0);
        assert_eq!(report.own, None);
        assert!(!report.complete);
    }

    #[test]
    fn growth_needs_an_earlier_report_in_the_window_that_listed_the_folder() {
        let point = |day: Timestamp, items: Vec<(&str, u64)>| TidyPoint {
            checked_at: day * DAY,
            complete: true,
            total: Some(0),
            regenerable: 0,
            review: 0,
            vcs: 0,
            test_data: 0,
            agent: 0,
            items: items.into_iter().map(|(rel, bytes)| (rel.to_string(), TidyClass::Review, bytes)).collect(),
        };
        let history = [
            point(5, vec![("tests/data", 1)]), // outside the 90-day window
            point(20, vec![("TESTS/data", 100)]),
            point(60, vec![("tests/data", 200)]),
            point(100, vec![("tests/data", 300)]), // the report itself
        ];
        let report = finish_report(&tidy(), &history, &[], Vec::new(), 100 * DAY);
        assert_eq!(report.decide[0].grown, Some(200), "from day 20, the oldest in the window");
        assert_eq!(report.growth_since, Some(20 * DAY));
        assert_eq!(report.recoverable[0].grown, None, "never listed before: unknown, not zero");
        let alone = finish_report(&tidy(), &history[3..], &[], Vec::new(), 100 * DAY);
        assert_eq!((alone.decide[0].grown, alone.growth_since), (None, None), "one report is no trend");
    }
}
