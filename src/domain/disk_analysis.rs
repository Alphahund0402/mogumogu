//! Bounded, read-only disk analysis views. A size is an observation, never a
//! cleanup recommendation. No file contents are needed for any view.
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskView {
    #[default]
    Tree,
    Files,
    Types,
    Duplicates,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskSort {
    #[default]
    Size,
    /// Space on disk; see [`DiskRow::allocated`].
    Allocated,
    Name,
    Modified,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiskQuery {
    /// Token from [`DiskPage::analysis`]; zero accepts the current analysis.
    pub analysis: u64,
    pub folder: usize,
    pub view: DiskView,
    pub sort: DiskSort,
    pub text: String,
    pub offset: usize,
    /// Zero selects the default page size.
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskSummary {
    pub checked_at: Timestamp,
    /// Unknown if enumeration was incomplete or skipped a placeholder/link.
    pub total_bytes: Option<u64>,
    pub observed_bytes: u64,
    /// Allocated space of the observed files; a partial sum like `observed_bytes`.
    pub allocated_bytes: u64,
    pub files: u64,
    pub folders: u64,
    pub skipped: u64,
    pub unreadable: u64,
    pub entries: u64,
    pub complete: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskRow {
    /// Opaque index into this scan; never interpreted as a filesystem path.
    pub id: usize,
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub bytes: u64,
    /// Allocated on disk, once per name: hardlinks count for each of their
    /// names, compressed and sparse files can need less than their size.
    pub allocated: u64,
    pub files: u64,
    pub folders: u64,
    pub modified: Timestamp,
    /// Same name and logical size only; content equality has not been tested.
    pub duplicate_candidates: usize,
    /// False if something below was not read: sizes and counts are lower bounds.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiskTile {
    pub id: usize,
    pub name: String,
    pub bytes: u64,
    pub directory: bool,
    /// Remainder after the displayed largest entries; not a navigable folder.
    pub remainder: bool,
    pub complete: bool,
    /// Normalized rectangle coordinates in [0, 1].
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiskPage {
    pub scope_id: i64,
    pub root: String,
    pub analysis: u64,
    pub folder: usize,
    pub parent: Option<usize>,
    pub path: String,
    /// Logical size of the shown folder: the base of each row's share.
    pub folder_bytes: u64,
    /// False if the shown folder was not read in full; an empty list then
    /// means "unknown", not "empty".
    pub folder_complete: bool,
    pub view: DiskView,
    pub summary: DiskSummary,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub rows: Vec<DiskRow>,
    /// Largest matching entries, independent of table pagination and sorting.
    pub tiles: Vec<DiskTile>,
}

impl DiskPage {
    /// One page of CSV for stdout/clipboard, with formula-like names escaped.
    /// No export operation writes inside an approved root.
    pub fn csv(&self) -> String {
        fn cell(text: &str) -> String {
            let text = text.replace('"', "\"\"");
            let prefix = if text.trim_start().starts_with(['=', '+', '-', '@']) { "'" } else { "" };
            format!("\"{prefix}{text}\"")
        }
        let mut csv = String::from(
            "name,relative_path,directory,logical_bytes,allocated_bytes,files,folders,modified,duplicate_candidates,complete,scan_complete\r\n",
        );
        for row in &self.rows {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{}\r\n",
                cell(&row.name),
                cell(&row.path),
                row.directory,
                row.bytes,
                row.allocated,
                row.files,
                row.folders,
                row.modified,
                row.duplicate_candidates,
                row.complete,
                self.summary.complete
            ));
        }
        csv
    }
}

/// Balanced binary treemap. Positive weights partition the entire rectangle;
/// zero-byte entries remain in the table, where they can still be inspected.
pub fn disk_treemap(mut tiles: Vec<DiskTile>) -> Vec<DiskTile> {
    tiles.retain(|tile| tile.bytes > 0);
    tiles.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    fn place(tiles: &mut [DiskTile], x: f64, y: f64, width: f64, height: f64) {
        if tiles.is_empty() {
            return;
        }
        if tiles.len() == 1 {
            let tile = &mut tiles[0];
            tile.x = x as f32;
            tile.y = y as f32;
            tile.width = width as f32;
            tile.height = height as f32;
            return;
        }
        let total: f64 = tiles.iter().map(|tile| tile.bytes as f64).sum();
        let mut left = tiles[0].bytes as f64;
        let mut split = 1;
        while split + 1 < tiles.len() && (left + tiles[split].bytes as f64 / 2.0) < total / 2.0 {
            left += tiles[split].bytes as f64;
            split += 1;
        }
        let share = left / total;
        let (a, b) = tiles.split_at_mut(split);
        if width >= height {
            place(a, x, y, width * share, height);
            place(b, x + width * share, y, width * (1.0 - share), height);
        } else {
            place(a, x, y, width, height * share);
            place(b, x, y + height * share, width, height * (1.0 - share));
        }
    }
    place(&mut tiles, 0.0, 0.0, 1.0, 1.0);
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treemap_preserves_area_and_never_overlaps() {
        let tiles = disk_treemap(
            (0..41)
                .map(|i| DiskTile {
                    id: i,
                    name: i.to_string(),
                    bytes: i as u64,
                    directory: false,
                    remainder: false,
                    complete: true,
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                })
                .collect(),
        );
        assert_eq!(tiles.len(), 40);
        let total = (1..41).sum::<u64>() as f32;
        for (i, a) in tiles.iter().enumerate() {
            assert!((a.width * a.height - a.bytes as f32 / total).abs() < 0.00001);
            assert!(a.x >= 0.0 && a.y >= 0.0 && a.x + a.width <= 1.00001 && a.y + a.height <= 1.00001);
            for b in tiles.iter().skip(i + 1) {
                let overlap_x = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
                let overlap_y = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
                assert!(overlap_x < 0.00001 || overlap_y < 0.00001);
            }
        }
        assert!((tiles.iter().map(|t| t.width * t.height).sum::<f32>() - 1.0).abs() < 0.00001);
        assert!(disk_treemap(Vec::new()).is_empty());
    }

    #[test]
    fn csv_quotes_paths_neutralizes_formulas_and_discloses_partial_results() {
        let page = DiskPage {
            scope_id: 1,
            root: "C:\\fixture".into(),
            analysis: 1,
            folder: 0,
            parent: None,
            path: String::new(),
            folder_bytes: 1,
            folder_complete: false,
            view: DiskView::Files,
            summary: DiskSummary {
                checked_at: 0,
                total_bytes: None,
                observed_bytes: 1,
                allocated_bytes: 4096,
                files: 1,
                folders: 0,
                skipped: 1,
                unreadable: 0,
                entries: 2,
                complete: false,
                reason: "partial".into(),
            },
            total: 1,
            offset: 0,
            limit: 1,
            tiles: Vec::new(),
            rows: vec![DiskRow {
                id: 1,
                name: "=formula,\"name\"".into(),
                path: "\t+formula".into(),
                directory: false,
                bytes: 1,
                allocated: 4096,
                files: 1,
                folders: 0,
                modified: 0,
                duplicate_candidates: 0,
                complete: true,
            }],
        };
        let csv = page.csv();
        assert!(csv.contains("\"'=formula,\"\"name\"\"\""));
        assert!(csv.contains("\"'\t+formula\""));
        assert!(csv.contains(",false,1,4096,1,0,"), "logical and allocated bytes side by side");
        assert!(csv.ends_with(",0,true,false\r\n"), "the row is whole, the scan around it is not");
    }
}
