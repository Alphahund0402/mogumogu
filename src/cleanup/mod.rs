//! Controlled cleanup (F-21 – F-24; PROJEKTPLAN §10).
//!
//! Scope of this implementation, deliberately narrow:
//! - Only folders the tidy-up report proves recoverable can be planned: a
//!   known output, cache or environment with the file that rebuilds it next
//!   to it (rules in `inventory::tidy`). Sources, lockfiles, version control,
//!   test data and everything marked „Prüfen“ stay untouched.
//! - Inside a Git work tree a folder counts only if the index (read
//!   statically, Git is never started) lists no file in it; an index that
//!   cannot be read leaves it „Prüfen“. Folders the person keeps, folders a
//!   registered agent run made and folders a container of an approved Docker
//!   engine binds in never enter a plan ([`holds`]). Leftovers of AI agents
//!   are never recoverable.
//! - A folder moves to the Windows recycle bin as a whole and can be
//!   restored there. Nothing is deleted file by file, nothing silently for
//!   good.
//! - Until the independent review of gate G5 has happened, a plan only acts
//!   inside an explicitly registered *disposable test root* (marker file
//!   plus stored identity). [`PRODUCTION_ENABLED`] is false and there is no
//!   override switch.
//! - A plan binds folder identities, their evidence and the rule version.
//!   Before execution and again right before each move everything is
//!   checked on handles; any difference stops it.
//! - Each move is journaled durably before it happens; after a crash the
//!   journal is compared with the disk and nothing is repeated.
mod executor;
mod gate;
mod holds;
mod planner;

pub use executor::{ExecutorHooks, NoHooks, RecycleBin, Recycler, apply, reconcile};
pub use gate::{DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT, KEEP_MARKER, keep_location};
pub use holds::{Env, Holds, Places, holds};
pub use planner::{Candidate, approve, draft, summary};

/// Production cleanup stays disabled until gate G5 (independent review of
/// the critical deletion paths) is passed. This is not a runtime setting.
pub const PRODUCTION_ENABLED: bool = false;

/// Version of the decision rules; part of every plan fingerprint. Plans of
/// another version never run.
pub const RULE_VERSION: &str = "recycle-rules-3";
