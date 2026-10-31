//! Tidy-up report: what in an approved project can be rebuilt and what needs
//! a look. A class comes from folder names plus the project file that names
//! how to rebuild it (a lockfile next to `node_modules`, `Cargo.toml` next to
//! `target`). Names and age alone never make anything removable.
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    pub enum TidyClass {
        /// A known output, cache or environment whose project names how to
        /// rebuild it.
        Regenerable => "regenerable",
        /// Possibly needed: test data, databases, logs, large files and
        /// environments or outputs without a file that rebuilds them.
        Review => "review",
    }
}

impl TidyClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Regenerable => "Wiederherstellbar",
            Self::Review => "Prüfen",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TidyItem {
    /// Relative to the project, `/`-separated.
    pub rel_path: String,
    pub class: TidyClass,
    pub label: String,
    /// Logical size below this entry.
    pub bytes: u64,
    /// Space the drive reserved for it: rounded to clusters, smaller when
    /// compressed. Counted per name, so hard links count more than once.
    #[serde(default)]
    pub allocated: u64,
    /// Why this class: the names that support it.
    pub evidence: String,
    pub modified_at: Option<Timestamp>,
}

/// One measured project. Sizes are logical file sizes from directory
/// listings; an incomplete walk keeps `total` unknown.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectTidy {
    pub project_id: i64,
    pub name: String,
    pub path: String,
    pub total: Option<u64>,
    pub regenerable: u64,
    /// Allocated space of the recoverable entries (see [`TidyItem::allocated`]).
    #[serde(default)]
    pub regenerable_allocated: u64,
    pub review: u64,
    /// Version control data (`.git` …): kept, but shown for orientation.
    pub vcs: u64,
    /// Test data, part of `review`.
    #[serde(default)]
    pub test_data: u64,
    /// Leftovers of AI agents and their tools, part of `review`.
    #[serde(default)]
    pub agent: u64,
    /// Folders the person keeps (`/`-separated): never part of a plan.
    #[serde(default)]
    pub kept: Vec<String>,
    /// Runs registered for this project, newest first, bounded.
    #[serde(default)]
    pub runs: Vec<RunBrief>,
    /// How the project changed over the saved reports.
    #[serde(default)]
    pub growth: Option<Growth>,
    /// Largest entries first, bounded.
    pub items: Vec<TidyItem>,
    /// Newest file outside outputs, caches and version control.
    pub last_change: Option<Timestamp>,
    pub complete: bool,
    pub note: String,
    pub checked_at: Timestamp,
}

/// A registered run in a report: who, in what state, with how many artifacts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunBrief {
    pub id: i64,
    pub title: String,
    pub started_at: Timestamp,
    pub artifacts: usize,
}

/// One saved report of a project, for its history.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TidyPoint {
    pub checked_at: Timestamp,
    pub complete: bool,
    pub total: Option<u64>,
    pub regenerable: u64,
    pub review: u64,
    pub vcs: u64,
    pub test_data: u64,
    pub agent: u64,
    /// The largest entries then: path, class and size.
    pub items: Vec<(String, TidyClass, u64)>,
}

impl TidyPoint {
    pub fn of(report: &ProjectTidy) -> Self {
        Self {
            checked_at: report.checked_at,
            complete: report.complete,
            total: report.total,
            regenerable: report.regenerable,
            review: report.review,
            vcs: report.vcs,
            test_data: report.test_data,
            agent: report.agent,
            items: report.items.iter().take(HISTORY_ITEMS).map(|i| (i.rel_path.clone(), i.class, i.bytes)).collect(),
        }
    }
}

/// Entries kept per saved report.
pub const HISTORY_ITEMS: usize = 20;

/// Growth between the oldest and the newest complete report in the window.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Growth {
    pub since: Timestamp,
    pub until: Timestamp,
    pub reports: usize,
    pub total: Option<i64>,
    pub regenerable: i64,
    pub review: i64,
    /// Entries that grew most, by path: largest growth first, at most three.
    pub items: Vec<(String, i64)>,
}

/// How far back growth is measured.
pub const GROWTH_WINDOW: Timestamp = 90 * 86_400;

/// What grew between the oldest and the newest complete report of the last
/// [`GROWTH_WINDOW`]. Incomplete walks say nothing; one report is no trend.
/// An entry missing from the oldest report counts from zero, so a folder
/// that only now entered the largest entries may overstate its growth.
pub fn growth(points: &[TidyPoint], now: Timestamp) -> Option<Growth> {
    let window: Vec<&TidyPoint> =
        points.iter().filter(|p| p.complete && p.checked_at <= now && now - p.checked_at <= GROWTH_WINDOW).collect();
    let first = window.iter().min_by_key(|p| p.checked_at)?;
    let last = window.iter().max_by_key(|p| p.checked_at)?;
    if first.checked_at == last.checked_at {
        return None;
    }
    let delta =
        |a: u64, b: u64| i64::try_from(b).unwrap_or(i64::MAX).saturating_sub(i64::try_from(a).unwrap_or(i64::MAX));
    let mut items: Vec<(String, i64)> = last
        .items
        .iter()
        .map(|(rel, _, bytes)| {
            let before = first.items.iter().find(|(old, ..)| fold_case(old) == fold_case(rel)).map_or(0, |i| i.2);
            (rel.clone(), delta(before, *bytes))
        })
        .filter(|(_, grown)| *grown > 0)
        .collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items.truncate(3);
    Some(Growth {
        since: first.checked_at,
        until: last.checked_at,
        reports: window.len(),
        total: first.total.zip(last.total).map(|(a, b)| delta(a, b)),
        regenerable: delta(first.regenerable, last.regenerable),
        review: delta(first.review, last.review),
        items,
    })
}

/// The label of test data items; the context filter keeps them out too.
pub const TEST_DATA_LABEL: &str = "Testdaten";
/// The label of leftovers of AI agents (worktrees, recordings, caches).
pub const AGENT_LABEL: &str = "KI-Agent";

/// A name folded as Windows compares names: one character at a time.
/// `str::to_lowercase` would turn a final `Σ` into `ς`, which NTFS treats
/// as another name than `σ`.
pub fn fold_case(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

string_enum! {
    /// Where a hold comes from.
    pub enum HoldKind {
        /// The person keeps the folder.
        Keep => "keep",
        /// A registered run made it.
        Run => "run",
        /// A Docker container binds it in.
        Docker => "docker",
        /// The person monitors a database there.
        Database => "database",
    }
}

/// A folder that must stay where it is, at its real location – an absolute
/// path as the drive names it (long names, its own spelling). A hold belongs
/// to the folder, not to a project: it counts for every project that reaches
/// the folder, under any spelling. Holds only ever take folders out.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hold {
    pub kind: HoldKind,
    pub location: String,
    /// Lower case, for an evidence line: `von dir behalten`.
    pub reason: String,
}

/// The hold on the folder at `location`, if it is held, lies in a held
/// folder or holds one.
pub fn holding<'a>(holds: &'a [Hold], location: &str) -> Option<&'a Hold> {
    let path = normal_location(location);
    holds.iter().find(|hold| {
        let held = normal_location(&hold.location);
        path == held || path.starts_with(&format!("{held}\\")) || held.starts_with(&format!("{path}\\"))
    })
}

/// Whether two real locations name the same folder.
pub fn same_location(a: &str, b: &str) -> bool {
    normal_location(a) == normal_location(b)
}

fn normal_location(path: &str) -> String {
    fold_case(&path.replace('/', "\\")).trim_end_matches('\\').to_string()
}

/// Test data this large are worth a look on their own.
const TEST_DATA_LARGE: u64 = 1 << 30;
/// Below this, test data never stand out.
const TEST_DATA_NOTABLE: u64 = 100 << 20;

impl ProjectTidy {
    /// The project's own content: everything but outputs, caches,
    /// environments and version control. Unknown for an incomplete walk.
    pub fn own_content(&self) -> Option<u64> {
        self.total.map(|total| total.saturating_sub(self.regenerable.saturating_add(self.vcs)))
    }

    /// Test data over 1 GB, or over 100 MB and at least half of the
    /// project's own content. A reason to look, not a verdict.
    pub fn heavy_test_data(&self) -> bool {
        self.test_data >= TEST_DATA_LARGE
            || (self.test_data >= TEST_DATA_NOTABLE
                && self.own_content().is_some_and(|own| self.test_data.saturating_mul(2) >= own))
    }
}

/// An approved global store, measured as a whole.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreTidy {
    pub scope_id: i64,
    pub label: String,
    pub path: String,
    pub bytes: Option<u64>,
    pub class: TidyClass,
    pub evidence: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TidyReport {
    pub projects: Vec<ProjectTidy>,
    pub stores: Vec<StoreTidy>,
    /// Registered projects without a read approval: nothing is measured there.
    pub unapproved: usize,
    pub checked_at: Option<Timestamp>,
}

impl TidyReport {
    pub fn regenerable(&self) -> u64 {
        self.projects.iter().map(|p| p.regenerable).sum::<u64>()
            + self.stores.iter().filter(|s| s.class == TidyClass::Regenerable).filter_map(|s| s.bytes).sum::<u64>()
    }

    pub fn review(&self) -> u64 {
        self.projects.iter().map(|p| p.review).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(total: u64, regenerable: u64, test_data: u64) -> ProjectTidy {
        ProjectTidy {
            project_id: 1,
            name: "app".into(),
            path: String::new(),
            total: Some(total),
            regenerable,
            regenerable_allocated: regenerable,
            review: test_data,
            vcs: 0,
            test_data,
            agent: 0,
            kept: Vec::new(),
            runs: Vec::new(),
            growth: None,
            items: Vec::new(),
            last_change: None,
            complete: true,
            note: String::new(),
            checked_at: 0,
        }
    }

    #[test]
    fn test_data_stand_out_by_size_or_share_of_the_own_content() {
        let mb = 1 << 20;
        assert!(project(300 * mb, 100 * mb, 100 * mb).heavy_test_data(), "half of 200 MB own content");
        assert!(!project(2_000 * mb, 100 * mb, 200 * mb).heavy_test_data(), "a small share");
        assert!(!project(60 * mb, 0, 50 * mb).heavy_test_data(), "below 100 MB never");
        assert!(project(20_000 * mb, 0, 1_024 * mb).heavy_test_data(), "1 GB always");
        let unknown = ProjectTidy { total: None, ..project(0, 0, 200 * mb) };
        assert!(!unknown.heavy_test_data(), "an incomplete walk gives no share");
    }

    fn keep(location: &str) -> Hold {
        Hold { kind: HoldKind::Keep, location: location.into(), reason: "von dir behalten".into() }
    }

    #[test]
    fn a_hold_covers_its_folder_what_lies_in_it_and_what_holds_it() {
        let holds = vec![keep(r"C:\dev\app\web\dist")];
        for covered in
            [r"C:\dev\app\web\dist", r"c:\DEV\App\Web\Dist\", r"C:\dev\app\web\dist\assets", r"C:\dev\app\web"]
        {
            assert_eq!(holding(&holds, covered), Some(&holds[0]), "{covered}");
        }
        for free in [r"C:\dev\app\web\distribution", r"C:\dev\app\web\node_modules", r"D:\dev\app\web\dist"] {
            assert_eq!(holding(&holds, free), None, "{free}");
        }
    }

    #[test]
    fn names_fold_character_by_character_as_windows_compares_them() {
        // `to_lowercase` on the whole word would end it in a final sigma.
        assert_eq!(fold_case("ΟΔΟΣ"), "οδοσ");
        assert!(holding(&[keep(r"C:\ΟΔΟΣ\dist")], r"C:\οδοσ\dist").is_some());
    }

    fn point(at: i64, complete: bool, regenerable: u64, items: &[(&str, u64)]) -> TidyPoint {
        TidyPoint {
            checked_at: at,
            complete,
            total: Some(regenerable + 1_000),
            regenerable,
            review: 1_000,
            vcs: 0,
            test_data: 0,
            agent: 0,
            items: items.iter().map(|(rel, bytes)| (rel.to_string(), TidyClass::Regenerable, *bytes)).collect(),
        }
    }

    #[test]
    fn growth_compares_the_oldest_and_newest_complete_report_in_the_window() {
        let day = 86_400;
        let now = 200 * day;
        let points = [
            point(now - 120 * day, true, 0, &[]), // outside the window
            point(now - 30 * day, true, 500, &[("node_modules", 400), ("target", 100)]),
            point(now - 10 * day, false, 9_999, &[]), // incomplete: says nothing
            point(now, true, 1_700, &[("node_modules", 600), ("target", 900), (".next", 200)]),
        ];
        let growth = growth(&points, now).unwrap();
        assert_eq!((growth.since, growth.until, growth.reports), (now - 30 * day, now, 2));
        assert_eq!((growth.regenerable, growth.total, growth.review), (1_200, Some(1_200), 0));
        assert_eq!(growth.items, [("target".into(), 800), (".next".into(), 200), ("node_modules".into(), 200)]);
        assert_eq!(super::growth(&points[3..], now), None, "one report is no trend");
    }
}
