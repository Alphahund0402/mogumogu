//! Approved, identity-checked, bounded reading (F-05 – F-07).
//!
//! Everything that reads below an approved root goes through this module:
//! the root is opened once and compared with its approved identity, every
//! child is opened relative to its parent handle (see `platform`), reparse
//! points and cloud placeholders are skipped, and every read is bounded.
use crate::limits::ScanLimits;
use crate::platform::{DirEntryInfo, DirHandle, FileIdentity};
use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::Path;
use std::time::Instant;

/// Why an approved root cannot be used right now.
#[derive(Debug)]
pub enum RootError {
    /// The folder at the approved location is a different object.
    IdentityChanged,
    /// Missing drive, deleted folder or permission problem.
    Unavailable(io::Error),
    /// The root itself is a link/reparse point or otherwise unsafe.
    Unsafe(io::Error),
}

impl std::fmt::Display for RootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdentityChanged => f.write_str("Ordner wurde ersetzt; die frühere Freigabe gilt nicht für ihn."),
            Self::Unavailable(e) => write!(f, "Ordner nicht erreichbar: {e}"),
            Self::Unsafe(e) => write!(f, "Ordner wird nicht gelesen: {e}"),
        }
    }
}

/// An approved root, held open for the duration of one pass.
#[derive(Debug)]
pub struct ScopeRoot {
    handle: DirHandle,
    identity: FileIdentity,
}

impl ScopeRoot {
    /// Opens `path` without following a final link and verifies that it is
    /// still the approved object. While held, the root cannot be renamed.
    pub fn open(path: &Path, expected: Option<FileIdentity>) -> Result<Self, RootError> {
        let handle = DirHandle::open_root(path, true).map_err(|e| match e.kind() {
            io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidInput => RootError::Unsafe(e),
            _ => RootError::Unavailable(e),
        })?;
        let identity = handle.identity().map_err(RootError::Unavailable)?;
        if expected.is_some_and(|e| e != identity) {
            return Err(RootError::IdentityChanged);
        }
        Ok(Self { handle, identity })
    }

    pub fn handle(&self) -> &DirHandle {
        &self.handle
    }

    pub fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub fn into_handle(self) -> DirHandle {
        self.handle
    }
}

/// Work budget of one pass. Exceeding it marks the result partial; it never
/// turns missing coverage into absence.
#[derive(Debug)]
pub struct Budget {
    limits: ScanLimits,
    started: Instant,
    pub entries: u64,
    pub bytes: u64,
    pub files: u64,
    pub limit_reason: Option<String>,
}

impl Budget {
    pub fn new(limits: ScanLimits) -> Self {
        Self { limits, started: Instant::now(), entries: 0, bytes: 0, files: 0, limit_reason: None }
    }

    pub fn limits(&self) -> ScanLimits {
        self.limits
    }

    fn stop(&mut self, reason: &str) -> bool {
        if self.limit_reason.is_none() {
            self.limit_reason = Some(reason.to_string());
        }
        false
    }

    /// Accounts one directory entry; false once a limit is reached.
    pub fn entry(&mut self) -> bool {
        if self.limit_reason.is_some() {
            return false;
        }
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return self.stop("Eintragslimit erreicht");
        }
        if self.started.elapsed() > self.limits.time_budget {
            return self.stop("Zeitbudget erreicht");
        }
        true
    }

    pub fn remaining_entries(&self) -> usize {
        self.limits.max_entries.saturating_sub(self.entries).min(usize::MAX as u64) as usize
    }

    fn charge_bytes(&mut self, bytes: u64) -> bool {
        self.bytes += bytes;
        self.files += 1;
        if self.bytes > self.limits.max_total_bytes {
            return self.stop("Leselimit für Metadaten erreicht");
        }
        true
    }

    pub fn exhausted(&self) -> bool {
        self.limit_reason.is_some()
    }
}

/// Why a single file was not read.
#[derive(Debug)]
pub enum ReadIssue {
    TooLarge(u64),
    NotText,
    Unsafe(String),
    Io(io::Error),
    Budget,
}

impl std::fmt::Display for ReadIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(size) => write!(f, "zu groß ({size} Bytes) – nicht gelesen"),
            Self::NotText => f.write_str("kein UTF-8-Text"),
            Self::Unsafe(reason) => f.write_str(reason),
            Self::Io(e) => write!(f, "nicht lesbar: {e}"),
            Self::Budget => f.write_str("Budget erschöpft"),
        }
    }
}

/// Reads a child file of `dir` as UTF-8 text, at most `max` bytes.
pub fn read_text(dir: &DirHandle, name: &OsStr, max: u64, budget: &mut Budget) -> Result<String, ReadIssue> {
    if budget.exhausted() {
        return Err(ReadIssue::Budget);
    }
    let (file, stat) = dir.open_file(name).map_err(|e| match e.kind() {
        io::ErrorKind::PermissionDenied => ReadIssue::Unsafe(e.to_string()),
        _ => ReadIssue::Io(e),
    })?;
    if stat.size > max {
        return Err(ReadIssue::TooLarge(stat.size));
    }
    let mut bytes = Vec::with_capacity(stat.size as usize);
    file.take(max + 1).read_to_end(&mut bytes).map_err(ReadIssue::Io)?;
    if bytes.len() as u64 > max {
        return Err(ReadIssue::TooLarge(bytes.len() as u64));
    }
    if !budget.charge_bytes(bytes.len() as u64) {
        return Err(ReadIssue::Budget);
    }
    let text = String::from_utf8(bytes).map_err(|_| ReadIssue::NotText)?;
    Ok(text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text))
}

/// Lists a directory within the remaining entry budget.
pub fn list(dir: &DirHandle, budget: &mut Budget) -> io::Result<(Vec<DirEntryInfo>, bool)> {
    let (entries, truncated) = dir.entries(budget.remaining_entries().max(1))?;
    if truncated {
        budget.stop("Eintragslimit erreicht");
    }
    Ok((entries, truncated))
}

/// Joins a `/`-separated relative path.
pub fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") }
}
