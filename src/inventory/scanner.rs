//! Project and system-root traversal.
//!
//! Depth-first and lazy: a subdirectory is opened (relative to its parent
//! handle) only when it is processed, so open handles stay bounded by the
//! depth of the tree, not its width.
use super::access::{HandleAccess, ScopeRefCheck};
use crate::adapters::{self, Adapter, AdapterScope, FoundFile};
use crate::domain::ScopeKind;
use crate::fsread::{self, Budget};
use crate::limits::PACKAGE_FILE_MAX_BYTES;
use crate::platform::{DirEntryInfo, DirHandle};
use crate::profiles::{self, Catalog};
use crate::storage::GenerationResult;
use std::ffi::OsStr;
use std::rc::Rc;

/// Folders that are never descended into: version-control internals and
/// tool caches without inventory value. Hidden AI folders are traversed.
const SKIP_ALWAYS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".gradle",
    ".idea",
    ".vs",
    "$RECYCLE.BIN",
    "System Volume Information",
];
const MAX_REPORTED_ERRORS: usize = 50;

#[derive(Default)]
struct Notes {
    links: u64,
    placeholders: u64,
    non_unicode: u64,
}

/// A subdirectory waiting to be opened.
struct Pending {
    parent: Rc<DirHandle>,
    parent_names: Rc<Vec<String>>,
    name: String,
    rel: String,
    depth: u32,
}

struct ProjectScan<'a> {
    budget: &'a mut Budget,
    result: GenerationResult,
    notes: Notes,
    catalog: &'static Catalog,
    refs: ScopeRefCheck,
    adapters: Vec<&'static dyn Adapter>,
    stack: Vec<Pending>,
}

pub(super) fn scan_project(root: Rc<DirHandle>, budget: &mut Budget) -> GenerationResult {
    let mut scan = ProjectScan {
        budget,
        result: GenerationResult::default(),
        notes: Notes::default(),
        catalog: profiles::catalog(),
        refs: ScopeRefCheck { root: Rc::clone(&root) },
        adapters: adapters::all().iter().copied().filter(|a| a.descriptor().scope == AdapterScope::Project).collect(),
        stack: Vec::new(),
    };
    match fsread::list(&root, scan.budget) {
        Ok((entries, _)) => scan.process(root, String::new(), 0, &entries),
        Err(e) => {
            scan.result.failed = true;
            scan.result.errors.push(format!("Wurzel nicht lesbar: {e}"));
            return scan.result;
        }
    }
    while let Some(pending) = scan.stack.pop() {
        if scan.budget.exhausted() {
            break;
        }
        scan.open_pending(pending);
    }
    add_notes(&mut scan.result, &scan.notes);
    scan.result
}

impl ProjectScan<'_> {
    fn open_pending(&mut self, pending: Pending) {
        let child = match pending.parent.open_dir(OsStr::new(&pending.name)) {
            Ok(child) => Rc::new(child),
            Err(e) => return self.report(&pending.rel, e),
        };
        let entries = match fsread::list(&child, self.budget) {
            Ok((entries, _)) => entries,
            Err(e) => return self.report(&pending.rel, e),
        };
        let child_names: Vec<String> = entries.iter().filter_map(|e| e.name_str().map(str::to_string)).collect();
        let claim = self
            .adapters
            .iter()
            .find_map(|a| a.claim_dir(&pending.name, &pending.parent_names, &child_names).map(|c| (*a, c)));
        match claim {
            Some((adapter, claim)) => {
                let mut errors = Vec::new();
                let mut access =
                    HandleAccess { base: child, rel: pending.rel.clone(), budget: self.budget, errors: &mut errors };
                match adapter.inspect_dir(claim, &pending.rel, &mut access) {
                    Ok(items) => self.result.items.extend(items),
                    Err(e) => self.report(&pending.rel, e),
                }
                for error in errors {
                    self.report("", error);
                }
            }
            None => self.process(child, pending.rel, pending.depth, &entries),
        }
    }

    fn process(&mut self, dir: Rc<DirHandle>, rel: String, depth: u32, entries: &[DirEntryInfo]) {
        let names = Rc::new(entries.iter().filter_map(|e| e.name_str().map(str::to_string)).collect::<Vec<_>>());
        for entry in entries {
            if !self.budget.entry() {
                return;
            }
            if entry.is_reparse() {
                self.notes.links += 1;
                continue;
            }
            if entry.is_placeholder() {
                self.notes.placeholders += 1;
                continue;
            }
            let Some(name) = entry.name_str() else {
                self.notes.non_unicode += 1;
                continue;
            };
            let rel_child = fsread::join(&rel, name);
            if !entry.is_dir() {
                self.file(&dir, &rel, name, &rel_child, &names);
            } else if SKIP_ALWAYS.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                continue;
            } else if depth + 1 > self.budget.limits().max_depth {
                if self.budget.limit_reason.is_none() {
                    self.budget.limit_reason = Some("Tiefenlimit erreicht".into());
                }
            } else {
                self.stack.push(Pending {
                    parent: Rc::clone(&dir),
                    parent_names: Rc::clone(&names),
                    name: name.to_string(),
                    rel: rel_child,
                    depth: depth + 1,
                });
            }
        }
    }

    fn file(&mut self, dir: &DirHandle, rel: &str, name: &str, rel_child: &str, names: &[String]) {
        let wanting: Vec<&dyn Adapter> = self.adapters.iter().copied().filter(|a| a.descriptor().wants(name)).collect();
        let ai = if self.catalog.may_match(name) { self.catalog.matches(rel_child) } else { Vec::new() };
        if wanting.is_empty() && ai.is_empty() {
            return;
        }
        let max = if wanting.is_empty() {
            ai.iter().map(|(p, _)| p.max_bytes()).min().unwrap_or(crate::limits::AI_FILE_MAX_BYTES)
        } else {
            PACKAGE_FILE_MAX_BYTES
        };
        let content = match fsread::read_text(dir, OsStr::new(name), max, self.budget) {
            Ok(content) => content,
            Err(issue) => return self.report(rel_child, issue),
        };
        for adapter in wanting {
            let file = FoundFile { name, rel_dir: rel, content: &content, siblings: names };
            match adapter.parse_file(&file) {
                Ok(items) => self.result.items.extend(items),
                Err(e) => self.report(rel_child, e),
            }
        }
        for (profile, detection) in ai {
            match profiles::parse_detection(profile, detection, rel_child, &content, &mut self.refs) {
                Ok(items) => self.result.items.extend(items),
                Err(e) => self.report(rel_child, e),
            }
        }
    }

    fn report(&mut self, rel: &str, message: impl std::fmt::Display) {
        report(&mut self.result, rel, message);
    }
}

pub(super) fn scan_system_root(kind: ScopeKind, root: Rc<DirHandle>, budget: &mut Budget) -> GenerationResult {
    let mut result = GenerationResult::default();
    let adapter_id = if kind == ScopeKind::Scoop { "scoop" } else { "chocolatey" };
    let adapter = adapters::by_id(adapter_id).expect("built-in adapter");
    let mut errors = Vec::new();
    let mut access = HandleAccess { base: root, rel: String::new(), budget, errors: &mut errors };
    match adapter.inventory_root(&mut access) {
        Ok(items) => result.items = items,
        Err(e) => result.errors.push(e),
    }
    for error in errors {
        report(&mut result, "", error);
    }
    result
}

fn report(result: &mut GenerationResult, rel: &str, message: impl std::fmt::Display) {
    if result.errors.len() < MAX_REPORTED_ERRORS {
        let message = if rel.is_empty() { message.to_string() } else { format!("{rel}: {message}") };
        result.errors.push(crate::privacy::label(&message));
    }
}

fn add_notes(result: &mut GenerationResult, notes: &Notes) {
    if notes.links > 0 {
        result.notes.push(format!("{} Verknüpfungen/Reparse Points nicht verfolgt", notes.links));
    }
    if notes.placeholders > 0 {
        result.notes.push(format!("{} Cloud-Platzhalter nicht geladen", notes.placeholders));
    }
    if notes.non_unicode > 0 {
        result.notes.push(format!("{} Einträge mit nicht darstellbarem Namen übersprungen", notes.non_unicode));
    }
}
