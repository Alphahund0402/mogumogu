//! Safe executor and recovery (F-23, F-24).
//!
//! Order of operations for an approved plan:
//! 1. `approved → revalidating`: rebuild every manifest from the disk and
//!    compare it with the approved one. Any difference invalidates the plan.
//! 2. `revalidating → executing`, write all journal rows as `planned`.
//! 3. Per batch: mark rows `started` (durable commit), then remove each
//!    object through the handle whose identity was just verified, then
//!    record `done`/`failed`. A failed journal commit stops everything.
//! 4. After a crash the plan becomes `recovery_required`; [`reconcile`]
//!    compares journal and disk and never repeats a removal.
use super::manifest::{OpenedTarget, build, open_target, parent_of};
use super::static_blockers;
use crate::domain::{JournalState, PlanState};
use crate::limits::CLEANUP_JOURNAL_BATCH;
use crate::storage::{JournalEntry, ManifestEntry, Repository};
use crate::{Error, Result};
use std::collections::HashMap;
use std::ffi::OsStr;

/// Test seams for fault injection and cancellation at safe boundaries.
pub trait ExecutorHooks {
    /// Called after a batch was durably marked `started`, before any removal.
    /// Returning an error simulates a crash at this point.
    fn after_started(&mut self, _batch: &[i64]) -> Result<()> {
        Ok(())
    }
    fn cancel_requested(&mut self) -> bool {
        false
    }
}

#[derive(Debug, Default)]
pub struct NoHooks;
impl ExecutorHooks for NoHooks {}

pub fn apply(repo: &mut Repository, plan_id: i64, hooks: &mut dyn ExecutorHooks) -> Result<PlanState> {
    repo.require_local()?;
    let plan = repo.plan(plan_id)?;
    if plan.state != PlanState::Approved {
        return Err(Error::conflict(
            "Nur bestätigte Pläne können ausgeführt werden; jeder Plan läuft höchstens einmal.",
        ));
    }
    repo.transition_plan(plan_id, PlanState::Approved, PlanState::Revalidating, None)?;
    let opened = match revalidate(repo, &plan) {
        Ok(opened) => opened,
        Err(reasons) => {
            let note = format!("Ungültig geworden: {}", reasons.join(" "));
            repo.transition_plan(plan_id, PlanState::Revalidating, PlanState::Invalidated, Some(&note))?;
            return Err(Error::blocked(note));
        }
    };
    repo.transition_plan(plan_id, PlanState::Revalidating, PlanState::Executing, None)?;
    repo.journal_create(plan_id)?;
    let manifests: HashMap<i64, HashMap<String, ManifestEntry>> = plan
        .targets
        .iter()
        .map(|t| Ok((t.id, repo.manifest(t.id)?.into_iter().map(|e| (e.rel_path.clone(), e)).collect())))
        .collect::<Result<_>>()?;
    let journal = repo.journal(plan_id)?;
    let (mut done, mut failed) = (0_usize, 0_usize);
    for batch in journal.chunks(CLEANUP_JOURNAL_BATCH) {
        if hooks.cancel_requested() {
            repo.transition_plan(plan_id, PlanState::Executing, PlanState::Cancelled, Some(&progress(done, failed)))?;
            return Ok(PlanState::Cancelled);
        }
        let ids: Vec<i64> = batch.iter().map(|e| e.id).collect();
        // Durable before the first byte is touched; on failure nothing else happens.
        repo.journal_mark(&ids, JournalState::Started, None)?;
        hooks.after_started(&ids)?;
        let mut done_ids = Vec::new();
        for entry in batch {
            let target = &opened[&entry.target_id];
            let expected = manifests.get(&entry.target_id).and_then(|m| m.get(&entry.rel_path));
            match remove(target, entry, expected) {
                Ok(()) => done_ids.push(entry.id),
                Err(reason) => {
                    failed += 1;
                    repo.journal_mark(&[entry.id], JournalState::Failed, Some(&reason))?;
                }
            }
        }
        done += done_ids.len();
        repo.journal_mark(&done_ids, JournalState::Done, None)?;
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
    format!("{done} entfernt, {failed} nicht entfernt (Teilerfolg ist kein Rollback).")
}

/// Rebuilds all facts and manifests; returns opened targets or reasons.
fn revalidate(
    repo: &mut Repository,
    plan: &crate::storage::StoredPlan,
) -> std::result::Result<HashMap<i64, OpenedTarget>, Vec<String>> {
    let mut reasons = Vec::new();
    let mut opened = HashMap::new();
    let roots = repo.disposable_roots().map_err(|e| vec![e.to_string()])?;
    for target in &plan.targets {
        let check = (|| -> std::result::Result<OpenedTarget, Vec<String>> {
            let resource = repo.resource(target.resource_id).map_err(|e| vec![e.to_string()])?;
            let sessions = match resource.parent_id {
                Some(parent) => repo.sessions_for_scratch(parent).map_err(|e| vec![e.to_string()])?,
                None => Vec::new(),
            };
            let blockers = static_blockers(&resource, &sessions);
            if !blockers.is_empty() {
                return Err(blockers);
            }
            let handle = open_target(&roots, &target.path, Some(target.identity)).map_err(|e| vec![e])?;
            let (now, found) = build(&handle.target);
            if !found.is_empty() {
                return Err(found);
            }
            let approved = repo.manifest(target.id).map_err(|e| vec![e.to_string()])?;
            if now != approved {
                let added = now.iter().filter(|e| !approved.iter().any(|a| a.rel_path == e.rel_path)).count();
                let missing = approved.iter().filter(|a| !now.iter().any(|e| e.rel_path == a.rel_path)).count();
                return Err(vec![format!(
                    "Inhalt seit Bestätigung verändert ({added} neu, {missing} fehlend, übrige mit geänderter Identität oder Größe)."
                )]);
            }
            Ok(handle)
        })();
        match check {
            Ok(handle) => {
                opened.insert(target.id, handle);
            }
            Err(found) => reasons.extend(found),
        }
    }
    if reasons.is_empty() { Ok(opened) } else { Err(reasons) }
}

/// Removes exactly the object recorded in the manifest, or nothing.
fn remove(
    target: &OpenedTarget,
    entry: &JournalEntry,
    expected: Option<&ManifestEntry>,
) -> std::result::Result<(), String> {
    let expected = expected.ok_or("nicht im bestätigten Manifest")?;
    let (parent, name) =
        parent_of(&target.target, &entry.rel_path).map_err(|e| format!("Elternordner nicht sicher erreichbar: {e}"))?;
    let handle = parent
        .open_for_delete(OsStr::new(&name), entry.is_dir)
        .map_err(|e| format!("nicht geöffnet (in Benutzung?): {e}"))?;
    let stat = handle.stat();
    if stat.identity != entry.identity {
        return Err("anderes Objekt am Ort – nicht entfernt".into());
    }
    if !entry.is_dir && (stat.size != expected.size || stat.modified != expected.modified) {
        return Err("seit Bestätigung geändert – nicht entfernt".into());
    }
    handle.delete().map_err(|e| format!("Entfernen fehlgeschlagen: {e}"))?;
    match parent.child_identity(OsStr::new(&name), entry.is_dir) {
        Ok(Some(identity)) if identity == entry.identity => Err("noch vorhanden (Löschung ausstehend)".into()),
        _ => Ok(()),
    }
}

/// Compares the journal of an interrupted plan with the disk. Objects that
/// are gone are recorded as such; objects that still exist are *not*
/// removed again — a new, freshly approved plan is required.
pub fn reconcile(repo: &mut Repository, plan_id: i64) -> Result<PlanState> {
    repo.require_local()?;
    let plan = repo.plan(plan_id)?;
    if plan.state != PlanState::RecoveryRequired {
        return Err(Error::conflict("Abgleich ist nur für unterbrochene Pläne vorgesehen."));
    }
    let roots = repo.disposable_roots()?;
    let journal = repo.journal(plan_id)?;
    let mut targets: HashMap<i64, Option<OpenedTarget>> = HashMap::new();
    let (mut gone, mut kept) = (0_usize, 0_usize);
    for entry in journal.iter().filter(|e| matches!(e.state, JournalState::Started | JournalState::Planned)) {
        let opened = targets.entry(entry.target_id).or_insert_with(|| {
            plan.targets
                .iter()
                .find(|t| t.id == entry.target_id)
                .and_then(|t| open_target(&roots, &t.path, Some(t.identity)).ok())
        });
        let state = match opened {
            None => JournalState::Blocked,
            Some(target) => match parent_of(&target.target, &entry.rel_path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => JournalState::ReconciledMissing,
                Err(_) => JournalState::Blocked,
                Ok((parent, name)) => match parent.child_identity(OsStr::new(&name), entry.is_dir) {
                    Ok(None) => JournalState::ReconciledMissing,
                    Ok(Some(identity)) if identity == entry.identity => JournalState::Pending,
                    Ok(Some(_)) => JournalState::Blocked,
                    Err(_) => JournalState::Blocked,
                },
            },
        };
        if state == JournalState::ReconciledMissing {
            gone += 1;
        } else {
            kept += 1;
        }
        repo.journal_mark(&[entry.id], state, Some("nach Unterbrechung abgeglichen; nicht erneut ausgeführt"))?;
    }
    let previously_done = journal.iter().filter(|e| e.state == JournalState::Done).count();
    let next = if previously_done + gone == 0 {
        PlanState::Failed
    } else if kept == 0 {
        PlanState::Completed
    } else {
        PlanState::Partial
    };
    let note = format!(
        "Abgleich: {} entfernt, {kept} noch vorhanden oder blockiert – kein erneuter Versuch.",
        previously_done + gone
    );
    repo.transition_plan(plan_id, PlanState::RecoveryRequired, next, Some(&note))?;
    Ok(next)
}
