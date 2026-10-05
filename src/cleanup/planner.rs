//! Drafting and approving plans (F-22). A draft never changes anything on
//! disk; approval binds a person's confirmation to the exact fingerprint.
use super::manifest::{build, open_target};
use super::{RULE_VERSION, static_blockers};
use crate::domain::{PlanState, PlanSummary, PlanTarget, ResourceKind};
use crate::storage::{NewTarget, Repository};
use crate::{Error, Result};
use sha2::{Digest, Sha256};

/// Drafts a plan for the given managed temporary folders. Ineligible
/// targets are kept in the plan *with* their blockers, so the person sees
/// why nothing will happen.
pub fn draft(repo: &mut Repository, resource_ids: &[i64]) -> Result<i64> {
    repo.require_local()?;
    if resource_ids.is_empty() || resource_ids.len() > 50 {
        return Err(Error::invalid("Ein Plan umfasst 1–50 Ziele."));
    }
    let roots = repo.disposable_roots()?;
    let mut targets = Vec::new();
    for &id in resource_ids {
        let resource = repo.resource(id)?;
        let sessions = match resource.parent_id {
            Some(parent) => repo.sessions_for_scratch(parent)?,
            None => Vec::new(),
        };
        let identity = repo.resource_identity(id)?;
        let mut blockers = static_blockers(&resource, &sessions);
        let mut manifest = Vec::new();
        if blockers.is_empty() {
            match open_target(&roots, &resource.path, identity) {
                Ok(opened) => {
                    let (entries, found) = build(&opened.target);
                    manifest = entries;
                    blockers.extend(found);
                    if manifest.is_empty() && blockers.is_empty() {
                        blockers.push("Der Ordner ist leer – nichts zu entfernen.".into());
                    }
                }
                Err(blocker) => blockers.push(blocker),
            }
        }
        targets.push(NewTarget {
            resource_id: id,
            path: resource.path.clone(),
            identity: identity.unwrap_or(crate::platform::FileIdentity { volume: 0, id: [0; 16] }),
            blockers,
            manifest,
        });
    }
    let fingerprint = fingerprint(&targets);
    repo.insert_plan(RULE_VERSION, &fingerprint, &targets)
}

/// Binds approval to the plan fingerprint shown to the person.
pub fn approve(repo: &mut Repository, plan_id: i64, confirmation: &str) -> Result<()> {
    let plan = summary(repo, plan_id)?;
    if plan.state != PlanState::Draft {
        return Err(Error::conflict("Nur Entwürfe können bestätigt werden."));
    }
    if !plan.executable() {
        return Err(Error::blocked("Der Plan enthält Blocker und kann nicht bestätigt werden."));
    }
    if !confirmation.trim().eq_ignore_ascii_case(plan.short_fingerprint()) {
        return Err(Error::invalid(format!(
            "Bestätigung passt nicht zum Plan. Erwartet wird der Fingerabdruck {}.",
            plan.short_fingerprint()
        )));
    }
    repo.transition_plan(plan_id, PlanState::Draft, PlanState::Approved, Some("Lokal bestätigt"))
}

pub(super) fn fingerprint(targets: &[NewTarget]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(RULE_VERSION.as_bytes());
    for target in targets {
        hasher.update(target.resource_id.to_le_bytes());
        hasher.update(target.identity.to_string().as_bytes());
        for entry in &target.manifest {
            hasher.update(entry.rel_path.as_bytes());
            hasher.update(entry.identity.to_string().as_bytes());
            hasher.update([u8::from(entry.is_dir)]);
            hasher.update(entry.size.to_le_bytes());
            hasher.update(entry.modified.to_le_bytes());
        }
    }
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Human-readable plan with kept contents, recovery and progress.
pub fn summary(repo: &mut Repository, plan_id: i64) -> Result<PlanSummary> {
    let plan = repo.plan(plan_id)?;
    let journal = repo.journal(plan_id)?;
    let mut kept = Vec::new();
    let mut targets = Vec::new();
    for target in &plan.targets {
        let resource = repo.resource(target.resource_id)?;
        if let Some(parent) = resource.parent_id {
            for sibling in repo.children(parent)? {
                if sibling.kind != ResourceKind::Temporary {
                    kept.push(format!("{} ({})", sibling.name, sibling.kind.label()));
                }
            }
        }
        kept.push(format!("Der Ordner „{}“ selbst bleibt bestehen", resource.name));
        targets.push(PlanTarget {
            id: target.id,
            resource_id: target.resource_id,
            resource_name: resource.name,
            path: target.path.clone(),
            entries: target.entries,
            bytes: target.bytes,
            blockers: target.blockers.clone(),
        });
    }
    kept.sort();
    kept.dedup();
    use crate::domain::JournalState::*;
    Ok(PlanSummary {
        id: plan.id,
        state: plan.state,
        created_at: plan.created_at,
        fingerprint: plan.fingerprint,
        targets,
        kept,
        recovery: "Keine Quarantäne und keine Sicherung: die aufgeführten Dateien werden endgültig entfernt. \
                   Rekonstruktion nur durch erneutes Ausführen des Laufs – ohne Erfolgsgarantie."
            .into(),
        done: journal.iter().filter(|j| matches!(j.state, Done | ReconciledMissing)).count() as i64,
        failed: journal.iter().filter(|j| matches!(j.state, Failed | Blocked | Pending)).count() as i64,
        note: plan.note,
    })
}
