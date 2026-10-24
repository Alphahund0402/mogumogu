//! Executor and recovery (F-23, F-24).
//!
//! Order of operations for an approved plan:
//! 1. `approved → revalidating`: check every folder again on handles (way,
//!    identity, evidence, protection, test root). Any difference
//!    invalidates the whole plan before anything moves.
//! 2. `revalidating → executing`. Per folder: check once more while holding
//!    its parent open, mark the row `started` (durable commit), confirm the
//!    folder is still the same, move it to the recycle bin by its real path,
//!    then confirm it left its place and record `done` or `failed`. A failed
//!    journal commit stops everything.
//! 3. After a crash the plan becomes `recovery_required`; [`reconcile`]
//!    compares journal and disk and never repeats a move.
use super::RULE_VERSION;
use super::gate::{Located, allowed, locate, unprotected};
use crate::domain::{JournalState, PlanState, ScopeKind, ScopeStatus};
use crate::platform::{self, FileIdentity};
use crate::storage::{Repository, StoredPlan, StoredTarget};
use crate::{Error, Result};
use std::path::Path;

/// Moves one folder to a place it can be restored from.
pub trait Recycler {
    fn recycle(&mut self, path: &Path) -> std::io::Result<()>;
}

/// The Windows recycle bin of the folder's drive.
#[derive(Debug, Default)]
pub struct RecycleBin;

impl Recycler for RecycleBin {
    fn recycle(&mut self, path: &Path) -> std::io::Result<()> {
        platform::recycle(path)
    }
}

/// Test seams for fault injection and cancellation at safe boundaries.
pub trait ExecutorHooks {
    /// Called after a folder was durably marked `started`, before it moves.
    /// Returning an error simulates a crash at this point.
    fn after_started(&mut self, _target_id: i64) -> Result<()> {
        Ok(())
    }
    fn cancel_requested(&mut self) -> bool {
        false
    }
}

#[derive(Debug, Default)]
pub struct NoHooks;
impl ExecutorHooks for NoHooks {}

pub fn apply(
    repo: &mut Repository,
    plan_id: i64,
    recycler: &mut dyn Recycler,
    hooks: &mut dyn ExecutorHooks,
) -> Result<PlanState> {
    repo.require_local()?;
    let plan = repo.plan(plan_id)?;
    if plan.state != PlanState::Approved {
        return Err(Error::conflict(
            "Nur bestätigte Pläne können ausgeführt werden; jeder Plan läuft höchstens einmal.",
        ));
    }
    if plan.rule_version != RULE_VERSION {
        return Err(Error::conflict("Plan mit anderer Regelversion; bitte einen neuen Plan erstellen."));
    }
    repo.transition_plan(plan_id, PlanState::Approved, PlanState::Revalidating, None)?;
    let (root, reasons) = match revalidate(repo, &plan) {
        Ok(root) => {
            let reasons: Vec<String> = plan
                .targets
                .iter()
                .filter_map(|t| check(repo, &root, t).err().map(|e| format!("{}: {e}", t.rel_path)))
                .collect();
            (root, reasons)
        }
        Err(reason) => ((String::new(), None), vec![reason]),
    };
    if !reasons.is_empty() {
        let note = format!("Ungültig geworden: {}", reasons.join(" "));
        repo.transition_plan(plan_id, PlanState::Revalidating, PlanState::Invalidated, Some(&note))?;
        return Err(Error::blocked(note));
    }
    repo.transition_plan(plan_id, PlanState::Revalidating, PlanState::Executing, None)?;
    let (mut done, mut failed) = (0_usize, 0_usize);
    for target in &plan.targets {
        if hooks.cancel_requested() {
            repo.transition_plan(plan_id, PlanState::Executing, PlanState::Cancelled, Some(&progress(done, failed)))?;
            return Ok(PlanState::Cancelled);
        }
        // Checked again while the parent is held; nothing on the way can change now.
        let located = match check(repo, &root, target) {
            Ok(located) => located,
            Err(reason) => {
                failed += 1;
                repo.mark_target(target.id, JournalState::Planned, JournalState::Failed, Some(&reason))?;
                continue;
            }
        };
        // Durable before the folder moves; on failure nothing else happens.
        repo.mark_target(target.id, JournalState::Planned, JournalState::Started, None)?;
        hooks.after_started(target.id)?;
        // The commit took a moment: the folder at the place must still be ours.
        if !matches!(located.current(), Ok(Some(identity)) if identity == target.identity) {
            failed += 1;
            let reason = "vor dem Verschieben verändert – nicht verschoben";
            repo.mark_target(target.id, JournalState::Started, JournalState::Failed, Some(reason))?;
            continue;
        }
        let outcome = match recycler.recycle(&located.shell) {
            Err(e) => Err(format!("Papierkorb: {e}")),
            Ok(()) => match located.current() {
                Ok(Some(identity)) if identity == target.identity => Err("noch am Ort – nicht verschoben".into()),
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Ergebnis nicht prüfbar: {e}")),
            },
        };
        drop(located);
        match outcome {
            Ok(()) => {
                done += 1;
                repo.mark_target(target.id, JournalState::Started, JournalState::Done, None)?;
            }
            Err(reason) => {
                failed += 1;
                repo.mark_target(target.id, JournalState::Started, JournalState::Failed, Some(&reason))?;
            }
        }
    }
    let state = match (done, failed) {
        (_, 0) => PlanState::Completed,
        (0, _) => PlanState::Failed,
        _ => PlanState::Partial,
    };
    repo.transition_plan(plan_id, PlanState::Executing, state, Some(&progress(done, failed)))?;
    Ok(state)
}

fn progress(done: usize, failed: usize) -> String {
    format!("{done} in den Papierkorb verschoben, {failed} nicht verschoben.")
}

type Root = (String, Option<FileIdentity>);

/// The project root of a plan: still an approved project scope.
fn revalidate(repo: &mut Repository, plan: &StoredPlan) -> std::result::Result<Root, String> {
    let scope_id = plan.targets.first().map(|t| t.scope_id).ok_or("Plan ohne Ziele.")?;
    if plan.targets.iter().any(|t| t.scope_id != scope_id || !t.blockers.is_empty()) {
        return Err("Plan ist nicht ausführbar.".into());
    }
    let scope = repo.scope(scope_id).map_err(|e| e.to_string())?;
    if scope.kind != ScopeKind::Project || scope.status != ScopeStatus::Approved {
        return Err("Die Lesefreigabe des Projekts besteht nicht mehr.".into());
    }
    let identity = repo.scope_identity(scope_id).map_err(|e| e.to_string())?;
    Ok((scope.path, identity))
}

/// Everything a move needs, checked on handles: way, identity, protection,
/// evidence unchanged since approval, and the G5 gate.
fn check(repo: &mut Repository, root: &Root, target: &StoredTarget) -> std::result::Result<Located, String> {
    let roots = repo.disposable_roots().map_err(|e| e.to_string())?;
    let resources = repo.resources().map_err(|e| e.to_string())?;
    let located = locate(&root.0, root.1, &target.rel_path)?;
    let (identity, _, evidence) = located.inspect()?;
    if identity != target.identity {
        return Err("Ein anderer Ordner steht jetzt an diesem Ort.".into());
    }
    if evidence != target.evidence {
        return Err(format!("Beleg geändert („{}“ statt „{}“).", evidence, target.evidence));
    }
    unprotected(&resources, &[&located.path.to_string_lossy(), &located.shell.to_string_lossy()])?;
    allowed(&roots, &located.path.to_string_lossy(), identity)?;
    Ok(located)
}

/// Compares the journal of an interrupted plan with the disk. Folders that
/// left their place are recorded as such; folders still there are *not*
/// moved again — a new, freshly approved plan is required.
pub fn reconcile(repo: &mut Repository, plan_id: i64) -> Result<PlanState> {
    repo.require_local()?;
    let plan = repo.plan(plan_id)?;
    if plan.state != PlanState::RecoveryRequired {
        return Err(Error::conflict("Abgleich ist nur für unterbrochene Pläne vorgesehen."));
    }
    let root = revalidate(repo, &plan).ok();
    let (mut gone, mut kept) = (0_usize, 0_usize);
    for target in plan.targets.iter().filter(|t| matches!(t.state, JournalState::Started | JournalState::Planned)) {
        let located = root.as_ref().map(|root| locate(&root.0, root.1, &target.rel_path));
        let present = match located {
            Some(Ok(located)) => match located.current() {
                Ok(None) => Some(false),
                Ok(Some(identity)) if identity == target.identity => Some(true),
                _ => None,
            },
            _ => None,
        };
        // Only a started folder that left its place counts as moved; one that
        // was never started and is gone now went some other way.
        let (state, note) = match (target.state, present) {
            (JournalState::Started, Some(false)) => {
                (JournalState::ReconciledMissing, "nach Unterbrechung nicht mehr am Ort; nicht erneut ausgeführt")
            }
            (_, Some(true)) => (JournalState::Pending, "nach Unterbrechung noch am Ort; nicht erneut ausgeführt"),
            (_, Some(false)) => (JournalState::Failed, "fehlte schon vor dem Verschieben"),
            (_, None) => (JournalState::Blocked, "nach Unterbrechung unklar; nicht erneut ausgeführt"),
        };
        if state == JournalState::ReconciledMissing {
            gone += 1;
        } else {
            kept += 1;
        }
        repo.mark_target(target.id, target.state, state, Some(note))?;
    }
    let previously_done = plan.targets.iter().filter(|t| t.state == JournalState::Done).count();
    let next = if previously_done + gone == 0 {
        PlanState::Failed
    } else if kept == 0 {
        PlanState::Completed
    } else {
        PlanState::Partial
    };
    let note = format!(
        "Abgleich: {} verschoben, {kept} noch vorhanden oder unklar – kein erneuter Versuch.",
        previously_done + gone
    );
    repo.transition_plan(plan_id, PlanState::RecoveryRequired, next, Some(&note))?;
    Ok(next)
}
