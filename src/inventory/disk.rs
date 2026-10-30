//! A single bounded metadata index, built on request through the existing
//! handle-relative read boundary. No files are opened, hashed or hydrated.
use crate::domain::*;
use crate::fsread::{self, Budget, ScanControl, ScopeRoot};
use crate::limits::{
    DISK_MATCH_STEPS, DISK_MAX_NODES, DISK_MAX_PAGE, DISK_NAME_BYTES, DISK_PAGE, DISK_TILES, LABEL_MAX_CHARS,
    ScanLimits,
};
use crate::platform::DirHandle;
use crate::{Error, Result, privacy};
use nodes::Nodes;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

mod nodes;

#[derive(Debug)]
pub struct DiskIndex {
    pub scope_id: i64,
    pub root: String,
    /// Token of this analysis; folder numbers are only valid within it.
    pub run: u64,
    pub summary: DiskSummary,
    nodes: Nodes,
    /// The reason is a limit, timeout or cancel, which outranks any other.
    stopped: bool,
}

impl DiskIndex {
    pub fn scan(
        scope_id: i64,
        path: &str,
        root: ScopeRoot,
        now: i64,
        limits: ScanLimits,
        control: Option<Arc<ScanControl>>,
    ) -> Self {
        let mut index = Self {
            scope_id,
            root: path.into(),
            run: 0,
            stopped: false,
            summary: DiskSummary {
                checked_at: now,
                total_bytes: None,
                observed_bytes: 0,
                allocated_bytes: 0,
                files: 0,
                folders: 0,
                skipped: 0,
                unreadable: 0,
                entries: 0,
                complete: true,
                reason: String::new(),
            },
            nodes: Nodes::default(),
        };
        index.nodes.push(0, "", true, 0, 0, 0);
        let mut budget = Budget::with_control(limits, control);
        let mut name_bytes = 0;
        // Open children only when popped. Rc keeps at most the ancestor handles
        // of this depth-first walk alive, including while a parent is renamed.
        let mut stack = vec![(Rc::new(root.into_handle()), None::<std::ffi::OsString>, 0_usize, 0_u32)];
        'walk: while let Some((parent, name, node, depth)) = stack.pop() {
            let dir: Rc<DirHandle> = match name {
                None => parent,
                Some(name) => match parent.open_dir(&name) {
                    Ok(dir) => Rc::new(dir),
                    Err(_) => {
                        index.summary.unreadable += 1;
                        index.nodes[node].set_incomplete();
                        index.partial("Unterordner nicht lesbar");
                        continue;
                    }
                },
            };
            let (entries, truncated) = match fsread::list_sample(&dir, &budget) {
                Ok(listing) => listing,
                Err(_) => {
                    index.summary.unreadable += 1;
                    index.nodes[node].set_incomplete();
                    index.partial("Ordner nicht lesbar");
                    continue;
                }
            };
            for entry in entries {
                if !budget.entry() {
                    index.nodes[node].set_incomplete();
                    break 'walk;
                }
                // A placeholder's data and a name that cannot be shown are
                // missing from this folder; a link's target is not its content.
                if entry.is_placeholder() || entry.name_str().is_none() {
                    index.summary.skipped += 1;
                    index.nodes[node].set_incomplete();
                    index.partial(if entry.is_placeholder() {
                        "Cloud-Platzhalter ausgelassen"
                    } else {
                        "Dateiname nicht als Unicode darstellbar"
                    });
                    continue;
                }
                if entry.is_reparse() {
                    index.summary.skipped += 1;
                    index.partial("Verknüpfungen ausgelassen");
                    continue;
                }
                let Some(name) = entry.name_str() else { continue };
                if index.nodes.len() >= DISK_MAX_NODES || name_bytes + name.len() > DISK_NAME_BYTES {
                    index.nodes[node].set_incomplete();
                    index.stop("Speicherbudget des Dateiverzeichnisses erreicht");
                    break 'walk;
                }
                name_bytes += name.len();
                let directory = entry.is_dir();
                let (bytes, allocated) = if directory { (0, 0) } else { (entry.size, entry.allocated) };
                let id = index.nodes.push(node, name, directory, bytes, allocated, entry.modified);
                if directory {
                    index.summary.folders += 1;
                    if depth >= limits.max_depth {
                        index.nodes[id].set_incomplete();
                        index.partial("Tiefenlimit erreicht");
                    } else {
                        stack.push((Rc::clone(&dir), Some(entry.name), id, depth + 1));
                    }
                } else {
                    index.summary.files += 1;
                }
            }
            if truncated {
                index.nodes[node].set_incomplete();
                index.stop("Eintragslimit erreicht");
                break;
            }
            if budget.exhausted() {
                break;
            }
        }
        // Folders still waiting were never listed: unknown, not empty.
        for (.., pending, _) in stack {
            index.nodes[pending].set_incomplete();
        }
        index.summary.entries = budget.entries;
        if let Some(reason) = budget.limit_reason {
            index.stop(&reason);
        }
        index.aggregate();
        index.mark_duplicates();
        index
    }

    /// Parents are always inserted before their descendants: one backwards
    /// pass sums the whole tree without rescanning any directory.
    fn aggregate(&mut self) {
        for id in (1..self.nodes.len()).rev() {
            let node = &self.nodes[id];
            let (parent, bytes, allocated, files, folders, complete) = (
                node.parent as usize,
                node.bytes,
                node.allocated,
                node.files(),
                node.folders + u32::from(node.directory()),
                node.complete(),
            );
            let parent = &mut self.nodes[parent];
            if !complete {
                parent.set_incomplete();
            }
            parent.allocated = parent.allocated.saturating_add(allocated);
            parent.add_files(files);
            parent.folders = parent.folders.saturating_add(folders);
            if let Some(total) = parent.bytes.checked_add(bytes) {
                parent.bytes = total;
            } else {
                parent.bytes = u64::MAX;
                self.summary.complete = false;
                self.summary.reason = "Größensumme nicht darstellbar".into();
            }
        }
        self.summary.observed_bytes = self.nodes[0].bytes;
        self.summary.allocated_bytes = self.nodes[0].allocated;
        self.summary.total_bytes = self.summary.complete.then_some(self.summary.observed_bytes);
    }

    /// Same size and the same name in any case. Sorting ids instead of hashing
    /// copies of the names keeps a drive-sized index from doubling here.
    fn mark_duplicates(&mut self) {
        let nodes = &self.nodes;
        let mut files: Vec<u32> =
            nodes.iter().filter(|(_, n)| !n.directory() && n.bytes > 0).map(|(id, _)| id as u32).collect();
        let order = |nodes: &Nodes, a: u32, b: u32| {
            let (a, b) = (a as usize, b as usize);
            nodes[a].bytes.cmp(&nodes[b].bytes).then_with(|| caseless(nodes.name(a), nodes.name(b)))
        };
        files.sort_unstable_by(|&a, &b| order(nodes, a, b));
        let mut start = 0;
        while start < files.len() {
            let mut end = start + 1;
            while end < files.len() && order(&self.nodes, files[start], files[end]) == Ordering::Equal {
                end += 1;
            }
            for &id in &files[start..end] {
                self.nodes[id as usize].set_duplicates((end - start) as u32);
            }
            start = end;
        }
    }

    fn partial(&mut self, reason: &str) {
        self.summary.complete = false;
        if self.summary.reason.is_empty() {
            self.summary.reason = reason.into();
        }
    }

    /// Ending the walk early says the most about coverage: it replaces an
    /// earlier reason such as a skipped link.
    fn stop(&mut self, reason: &str) {
        if !self.stopped {
            self.stopped = true;
            self.summary.complete = false;
            self.summary.reason = reason.into();
        }
    }

    /// Heap held by this analysis, for measurements.
    pub fn memory_bytes(&self) -> usize {
        self.nodes.memory_bytes()
    }

    fn under(&self, mut id: usize, folder: usize) -> bool {
        if folder == 0 {
            return true;
        }
        while id != 0 {
            id = self.nodes[id].parent as usize;
            if id == folder {
                return true;
            }
        }
        false
    }

    fn path(&self, mut id: usize) -> String {
        let mut components = Vec::new();
        while id != 0 {
            components.push(self.nodes.name(id));
            id = self.nodes[id].parent as usize;
        }
        components.reverse();
        // Paths are display data, never used to reopen a filesystem object.
        // A deep path loses its start, never the name at its end.
        let path = components.iter().map(|c| privacy::path(c)).collect::<Vec<_>>().join("/");
        let chars = path.chars().count();
        if chars <= LABEL_MAX_CHARS {
            return path;
        }
        format!("…{}", path.chars().skip(chars - (LABEL_MAX_CHARS - 1)).collect::<String>())
    }

    fn row(&self, id: usize) -> DiskRow {
        let node = &self.nodes[id];
        DiskRow {
            id,
            name: privacy::path(self.nodes.name(id)),
            path: self.path(id),
            directory: node.directory(),
            bytes: node.bytes,
            allocated: node.allocated,
            files: u64::from(node.files()),
            folders: u64::from(node.folders),
            modified: node.modified(),
            duplicate_candidates: node.duplicates() as usize,
            complete: node.complete(),
        }
    }

    pub fn page(&self, query: &DiskQuery) -> Result<DiskPage> {
        if query.analysis != 0 && query.analysis != self.run {
            return Err(Error::conflict("Inzwischen wurde neu analysiert; Ordnernummern gelten nur für ihre Analyse."));
        }
        if query.text.chars().count() > 200 {
            return Err(Error::invalid("Dateisuche auf 200 Zeichen begrenzen."));
        }
        if query.folder >= self.nodes.len() || !self.nodes[query.folder].directory() {
            return Err(Error::invalid("Ordner gehört nicht zu diesem Analysestand."));
        }
        let pattern = query.text.to_lowercase();
        let folder_complete = self.nodes[query.folder].complete();
        // Bounds a pathological pattern on the single owner thread.
        let (mut steps, mut scanned) = (0_u64, 0_u64);
        let mut matches = |name: &str| {
            if pattern.is_empty() {
                return Ok(true);
            }
            scanned += name.len() as u64;
            let found = matches_name(&name.to_lowercase(), &pattern, &mut steps);
            if !within_budget(steps, scanned) {
                return Err(Error::invalid("Suchmuster zu aufwendig; bitte genauer suchen oder weniger * verwenden."));
            }
            Ok(found)
        };
        let mut ids: Vec<u32> = Vec::new();
        let mut types = HashMap::<String, DiskRow>::new();
        for (id, node) in self.nodes.iter().skip(1) {
            let in_view = match query.view {
                DiskView::Tree => node.parent as usize == query.folder,
                DiskView::Files | DiskView::Types => !node.directory() && self.under(id, query.folder),
                DiskView::Duplicates => !node.directory() && node.duplicates() > 1 && self.under(id, query.folder),
            };
            if !in_view {
                continue;
            }
            if query.view == DiskView::Types {
                let extension = self
                    .nodes
                    .name(id)
                    .rsplit_once('.')
                    .filter(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
                    .map_or_else(|| "(ohne Endung)".into(), |(_, ext)| format!(".{}", ext.to_lowercase()));
                if !matches(&extension)? {
                    continue;
                }
                let row = types.entry(extension.clone()).or_insert_with(|| DiskRow {
                    id: 0,
                    name: privacy::path(&extension),
                    path: String::new(),
                    directory: false,
                    bytes: 0,
                    allocated: 0,
                    files: 0,
                    folders: 0,
                    modified: 0,
                    duplicate_candidates: 0,
                    complete: folder_complete,
                });
                row.bytes = row.bytes.saturating_add(node.bytes);
                row.allocated = row.allocated.saturating_add(node.allocated);
                row.files += 1;
                row.modified = row.modified.max(node.modified());
            } else if matches(self.nodes.name(id))? {
                ids.push(id as u32);
            }
        }
        // Keep all paths out of the sorting/filtering pass; only a bounded page
        // and the largest tiles allocate display rows.
        let compare = |a: &u32, b: &u32| {
            let (a, b) = (*a as usize, *b as usize);
            let (a_node, b_node) = (&self.nodes[a], &self.nodes[b]);
            let order = match query.sort {
                DiskSort::Size => b_node.bytes.cmp(&a_node.bytes),
                DiskSort::Allocated => b_node.allocated.cmp(&a_node.allocated),
                DiskSort::Name => self.nodes.name(a).cmp(self.nodes.name(b)),
                DiskSort::Modified => b_node.modified().cmp(&a_node.modified()),
            };
            order.then_with(|| self.nodes.name(a).cmp(self.nodes.name(b))).then_with(|| a.cmp(&b))
        };
        let limit = if query.limit == 0 { DISK_PAGE } else { query.limit.min(DISK_MAX_PAGE) };
        let total = if query.view == DiskView::Types { types.len() } else { ids.len() };
        let offset = query.offset.min(total.saturating_sub(1) / limit * limit);
        // Only the entries up to this page are put in order: on a drive with
        // millions of files a page costs one linear pass, not a full sort.
        let end = (offset + limit).min(ids.len());
        if end < ids.len() {
            ids.select_nth_unstable_by(end, compare);
        }
        ids[..end].sort_unstable_by(compare);
        let (rows, tiles) = if query.view == DiskView::Types {
            let mut rows: Vec<_> = types.into_values().collect();
            rows.sort_by(|a, b| {
                match query.sort {
                    DiskSort::Name => a.name.cmp(&b.name),
                    DiskSort::Modified => b.modified.cmp(&a.modified),
                    DiskSort::Size => b.bytes.cmp(&a.bytes),
                    DiskSort::Allocated => b.allocated.cmp(&a.allocated),
                }
                .then_with(|| a.name.cmp(&b.name))
            });
            let observed = rows.iter().fold(0_u64, |sum, r| sum.saturating_add(r.bytes));
            let mut largest: Vec<_> = rows.iter().collect();
            largest.sort_unstable_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
            let tiles = tiles(largest.into_iter().take(DISK_TILES).cloned(), observed, folder_complete);
            (rows.into_iter().skip(offset).take(limit).collect(), tiles)
        } else {
            let rows = ids.iter().skip(offset).take(limit).map(|&id| self.row(id as usize)).collect();
            let observed = ids.iter().fold(0_u64, |sum, &id| sum.saturating_add(self.nodes[id as usize].bytes));
            // The largest entries only, without sorting the whole selection again.
            let by_size = |a: &u32, b: &u32| {
                self.nodes[*b as usize].bytes.cmp(&self.nodes[*a as usize].bytes).then_with(|| a.cmp(b))
            };
            if ids.len() > DISK_TILES {
                ids.select_nth_unstable_by(DISK_TILES, by_size);
                ids.truncate(DISK_TILES);
            }
            // Tiles need only names, not full paths.
            let tiles = tiles(
                ids.into_iter().map(|id| {
                    let id = id as usize;
                    let n = &self.nodes[id];
                    DiskRow {
                        id,
                        name: privacy::path(self.nodes.name(id)),
                        path: String::new(),
                        directory: n.directory(),
                        bytes: n.bytes,
                        allocated: 0,
                        files: 0,
                        folders: 0,
                        modified: 0,
                        duplicate_candidates: 0,
                        complete: n.complete(),
                    }
                }),
                observed,
                folder_complete,
            );
            (rows, tiles)
        };
        Ok(DiskPage {
            scope_id: self.scope_id,
            root: self.root.clone(),
            analysis: self.run,
            folder: query.folder,
            parent: (query.folder != 0).then_some(self.nodes[query.folder].parent as usize),
            path: self.path(query.folder),
            folder_bytes: self.nodes[query.folder].bytes,
            folder_complete,
            view: query.view,
            summary: self.summary.clone(),
            total,
            offset,
            limit,
            rows,
            tiles,
        })
    }
}

/// Any pattern may walk each name a few times; only work beyond that, which
/// grows with name length times pattern length, is cut off.
fn within_budget(steps: u64, scanned_bytes: u64) -> bool {
    steps <= scanned_bytes.saturating_mul(4).saturating_add(DISK_MATCH_STEPS)
}

/// Orders names as their lowercase forms would, without allocating them.
fn caseless(a: &str, b: &str) -> Ordering {
    a.chars().flat_map(char::to_lowercase).cmp(b.chars().flat_map(char::to_lowercase))
}

fn tiles(rows: impl Iterator<Item = DiskRow>, observed: u64, folder_complete: bool) -> Vec<DiskTile> {
    let mut rows: Vec<_> = rows.collect();
    rows.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    let remainder = observed.saturating_sub(rows.iter().fold(0_u64, |sum, r| sum.saturating_add(r.bytes)));
    let mut tiles: Vec<_> = rows
        .into_iter()
        .take(DISK_TILES)
        .map(|r| DiskTile {
            id: r.id,
            name: r.name,
            bytes: r.bytes,
            directory: r.directory,
            remainder: false,
            complete: r.complete,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        })
        .collect();
    if remainder > 0 {
        tiles.push(DiskTile {
            id: 0,
            name: "Weitere Einträge".into(),
            bytes: remainder,
            directory: false,
            remainder: true,
            complete: folder_complete,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
    }
    disk_treemap(tiles)
}

/// Name substring, or a full-name glob when '*'/'?' occur. No regex engine or
/// filesystem access; '?' matches a Unicode scalar, rather than a UTF-8 byte.
fn matches_name(name: &str, pattern: &str, steps: &mut u64) -> bool {
    if !pattern.contains(['*', '?']) {
        *steps += name.len() as u64;
        return name.contains(pattern);
    }
    let name: Vec<_> = name.chars().collect();
    let pattern: Vec<_> = pattern.chars().collect();
    let (mut n, mut p, mut star, mut retry) = (0, 0, None, 0);
    while n < name.len() {
        *steps += 1;
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            n += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            retry = n;
        } else if let Some(at) = star {
            retry += 1;
            n = retry;
            p = at + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_search_supports_unicode_and_literal_substrings() {
        let matches = |name, pattern| matches_name(name, pattern, &mut 0);
        assert!(matches("möwe.rs", "m?we.*"));
        assert!(matches("file.tar.gz", "*.gz"));
        assert!(matches("file.tar.gz", "tar"));
        assert!(!matches("file.rs", "*.gz"));
        assert!(!matches("file", "file?"));
        assert!(matches("", "*"));
    }

    #[test]
    fn a_backtracking_pattern_is_counted_per_step() {
        // Each retry after the star walks the whole literal again.
        let (name, pattern) = ("a".repeat(255), format!("*{}b", "a".repeat(127)));
        let mut steps = 0;
        assert!(!matches_name(&name, &pattern, &mut steps));
        assert!(steps > 10_000, "quadratic work stays visible to the budget: {steps}");
    }

    #[test]
    fn the_budget_admits_a_drive_but_not_quadratic_work() {
        // Measured on a system drive: `*.dll` over 1.6 million file names.
        let (names, bytes) = (1_640_000_u64, 44_u64);
        let mut steps = 0;
        matches_name(&format!("{}.dll", "x".repeat(bytes as usize - 4)), "*.dll", &mut steps);
        assert!(within_budget(steps * names, bytes * names), "an ordinary pattern stays linear");
        let mut steps = 0;
        matches_name(&"a".repeat(255), &format!("*{}b", "a".repeat(127)), &mut steps);
        assert!(!within_budget(steps * 2_000, 255 * 2_000), "a pathological one is cut off early");
    }

    #[test]
    fn caseless_order_matches_lowercase_equality() {
        assert_eq!(caseless("Same.BIN", "same.bin"), Ordering::Equal);
        assert_eq!(caseless("ÄRGER", "ärger"), Ordering::Equal);
        assert_eq!(caseless("a", "B"), "a".cmp("b"));
    }
}
