//! Bounded, handle-relative implementations of the adapter and profile
//! access traits.
use crate::adapters::DirAccess;
use crate::fsread::{self, Budget, ReadIssue};
use crate::platform::DirHandle;
use crate::profiles::RefCheck;
use std::ffi::OsStr;
use std::io;
use std::rc::Rc;

/// Access below one opened directory (claimed environment or system root).
pub(super) struct HandleAccess<'a> {
    pub base: Rc<DirHandle>,
    pub rel: String,
    pub budget: &'a mut Budget,
    pub errors: &'a mut Vec<String>,
}

fn navigate(base: &Rc<DirHandle>, path: &[&str]) -> io::Result<Rc<DirHandle>> {
    let mut current = Rc::clone(base);
    for component in path {
        current = Rc::new(current.open_dir(OsStr::new(component))?);
    }
    Ok(current)
}

impl HandleAccess<'_> {
    fn report(&mut self, path: &[&str], message: impl std::fmt::Display) {
        if self.errors.len() < 50 {
            self.errors.push(format!("{}: {message}", fsread::join(&self.rel, &path.join("/"))));
        }
    }
}

impl DirAccess for HandleAccess<'_> {
    fn read_text(&mut self, path: &[&str], max: u64) -> Option<String> {
        let (name, parent) = path.split_last()?;
        let dir = match navigate(&self.base, parent) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
            Err(e) => {
                self.report(path, e);
                return None;
            }
        };
        match fsread::read_text(&dir, OsStr::new(name), max, self.budget) {
            Ok(text) => Some(text),
            Err(ReadIssue::Io(e)) if e.kind() == io::ErrorKind::NotFound => None,
            Err(issue) => {
                self.report(path, issue);
                None
            }
        }
    }

    fn list(&mut self, path: &[&str]) -> Vec<(String, bool)> {
        let dir = match navigate(&self.base, path) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => {
                self.report(path, e);
                return Vec::new();
            }
        };
        let entries = match fsread::list(&dir, self.budget) {
            Ok((entries, _)) => entries,
            Err(e) => {
                self.report(path, e);
                return Vec::new();
            }
        };
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            if !self.budget.entry() {
                break;
            }
            if entry.is_reparse() || entry.is_placeholder() {
                continue;
            }
            if let Some(name) = entry.name_str() {
                out.push((name.to_string(), entry.is_dir()));
            }
        }
        out
    }
}

/// Existence checks for AI references, relative to the scope root.
pub(super) struct ScopeRefCheck {
    pub root: Rc<DirHandle>,
}

impl RefCheck for ScopeRefCheck {
    fn exists(&mut self, rel_path: &str) -> Option<bool> {
        let parts: Vec<&str> = rel_path.split('/').collect();
        let (name, parent) = parts.split_last()?;
        let dir = match navigate(&self.root, parent) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Some(false),
            Err(_) => return None,
        };
        for directory in [false, true] {
            match dir.child_identity(OsStr::new(name), directory) {
                Ok(Some(_)) => return Some(true),
                Ok(None) => {}
                Err(_) if !directory => {}
                Err(_) => return None,
            }
        }
        Some(false)
    }
}
