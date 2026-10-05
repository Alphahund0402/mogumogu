//! Static discovery of one approved scope into a [`GenerationResult`].
//!
//! Pure with respect to the database: the caller persists the result as a
//! generation. Traversal is handle-relative and bounded (see `fsread`);
//! adapters and AI profiles only ever receive bounded text.
mod access;
mod measure;
mod scanner;

pub use measure::{Measurement, measure};

use crate::domain::ScopeKind;
use crate::fsread::{Budget, RootError, ScopeRoot};
use crate::limits::ScanLimits;
use crate::platform::FileIdentity;
use crate::storage::GenerationResult;
use std::path::PathBuf;
use std::rc::Rc;

/// Software entries read from the registry per pass.
const REGISTRY_MAX: usize = 5_000;

#[derive(Clone, Debug)]
pub struct ScanRequest {
    pub kind: ScopeKind,
    pub path: PathBuf,
    pub identity: Option<FileIdentity>,
    pub limits: ScanLimits,
}

/// Why a scope could not be read at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanFailure {
    IdentityChanged,
    Unavailable(String),
}

#[derive(Debug)]
pub struct ScanOutcome {
    pub result: GenerationResult,
    pub failure: Option<ScanFailure>,
}

pub fn scan(request: &ScanRequest) -> ScanOutcome {
    if request.kind == ScopeKind::Winget {
        return scan_registry();
    }
    let root = match ScopeRoot::open(&request.path, request.identity) {
        Ok(root) => root,
        Err(error) => {
            let failure = match &error {
                RootError::IdentityChanged => ScanFailure::IdentityChanged,
                other => ScanFailure::Unavailable(other.to_string()),
            };
            let result = GenerationResult { failed: true, errors: vec![error.to_string()], ..Default::default() };
            return ScanOutcome { result, failure: Some(failure) };
        }
    };
    let mut budget = Budget::new(request.limits);
    let root = Rc::new(root.into_handle());
    let mut result = match request.kind {
        ScopeKind::Project => scanner::scan_project(Rc::clone(&root), &mut budget),
        ScopeKind::Scoop | ScopeKind::Chocolatey => {
            scanner::scan_system_root(request.kind, Rc::clone(&root), &mut budget)
        }
        ScopeKind::Winget => unreachable!("handled above"),
    };
    result.entries_seen = budget.entries;
    result.files_read = budget.files;
    result.bytes_read = budget.bytes;
    result.limit_reason = budget.limit_reason.take();
    ScanOutcome { result, failure: None }
}

fn scan_registry() -> ScanOutcome {
    match crate::adapters::winget_registry(REGISTRY_MAX) {
        Ok((items, truncated)) => ScanOutcome {
            result: GenerationResult {
                entries_seen: items.len() as u64,
                limit_reason: truncated.then(|| "Eintragslimit der Softwareliste erreicht".to_string()),
                items,
                ..Default::default()
            },
            failure: None,
        },
        Err(error) => ScanOutcome {
            result: GenerationResult { failed: true, errors: vec![error.clone()], ..Default::default() },
            failure: Some(ScanFailure::Unavailable(error)),
        },
    }
}
