//! On-demand size measurement, separate from discovery (§12.3). Sizes come
//! from directory listings; nothing is opened or hydrated. An incomplete
//! measurement yields `None` (unknown), never a partial sum.
use crate::fsread::{self, Budget, ScopeRoot};
use crate::limits::ScanLimits;
use crate::platform::{DirHandle, FileIdentity};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Measurement {
    /// Total logical size of regular files, if the walk was complete.
    pub bytes: Option<u64>,
    pub entries: u64,
    pub skipped_links: u64,
    pub reason: Option<String>,
}

pub fn measure(path: &Path, identity: Option<FileIdentity>) -> Measurement {
    let limits =
        ScanLimits { max_depth: 64, max_entries: 1_000_000, max_total_bytes: 0, time_budget: Duration::from_secs(30) };
    let mut measurement = Measurement { bytes: None, entries: 0, skipped_links: 0, reason: None };
    let root = match ScopeRoot::open(path, identity) {
        Ok(root) => Rc::new(root.into_handle()),
        Err(e) => {
            measurement.reason = Some(e.to_string());
            return measurement;
        }
    };
    let mut budget = Budget::new(limits);
    let mut total = 0_u64;
    let mut complete = true;
    // (parent, child name, depth); the root has no parent.
    let mut stack: Vec<(Rc<DirHandle>, Option<OsString>, u32)> = vec![(root, None, 0)];
    while let Some((parent, name, depth)) = stack.pop() {
        let dir = match name {
            None => parent,
            Some(name) => match parent.open_dir(OsStr::new(&name)) {
                Ok(child) => Rc::new(child),
                Err(e) => {
                    measurement.reason = Some(format!("Unterordner nicht lesbar: {e}"));
                    complete = false;
                    continue;
                }
            },
        };
        let entries = match fsread::list(&dir, &mut budget) {
            Ok((entries, _)) => entries,
            Err(e) => {
                measurement.reason = Some(format!("Ordner nicht lesbar: {e}"));
                complete = false;
                continue;
            }
        };
        for entry in entries {
            if !budget.entry() {
                break;
            }
            if entry.is_reparse() {
                measurement.skipped_links += 1;
            } else if entry.is_placeholder() {
                measurement.reason = Some("Cloud-Platzhalter enthalten – lokale Größe unbekannt".into());
                complete = false;
            } else if !entry.is_dir() {
                total = total.saturating_add(entry.size);
            } else if depth + 1 > limits.max_depth {
                complete = false;
            } else {
                stack.push((Rc::clone(&dir), Some(entry.name), depth + 1));
            }
        }
        if budget.exhausted() {
            break;
        }
    }
    measurement.entries = budget.entries;
    if let Some(reason) = budget.limit_reason.take() {
        measurement.reason = Some(reason);
        complete = false;
    }
    measurement.bytes = complete.then_some(total);
    measurement
}
