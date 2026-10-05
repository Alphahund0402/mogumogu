//! Controlled cleanup (F-21 – F-24; PROJEKTPLAN §10).
//!
//! Scope of this implementation, deliberately narrow:
//! - Only the *contents* of a managed `temporary` folder of a scratchpad
//!   can be removed; the folder itself, sources, results, environments,
//!   AI configuration and anything unknown stay untouched.
//! - Until the independent review of gate G5 has happened, the executor
//!   only runs inside an explicitly registered *disposable test root*
//!   (marker file plus stored identity). [`PRODUCTION_ENABLED`] is false
//!   and there is no override switch.
//! - Every plan binds identities and a content manifest. Before execution
//!   everything is re-checked; any difference invalidates the plan.
//! - Each removal is journaled durably before it happens and acts on the
//!   very handle whose identity was just verified.
mod executor;
mod manifest;
mod planner;
mod policy;

pub use executor::{ExecutorHooks, NoHooks, apply, reconcile};
pub use manifest::{DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT, KEEP_MARKER};
pub use planner::{approve, draft, summary};
pub use policy::static_blockers;

/// Production cleanup stays disabled until gate G5 (independent review of
/// the critical deletion paths) is passed. This is not a runtime setting.
pub const PRODUCTION_ENABLED: bool = false;

/// Version of the decision rules; part of every plan fingerprint.
pub const RULE_VERSION: &str = "cleanup-rules-1";
