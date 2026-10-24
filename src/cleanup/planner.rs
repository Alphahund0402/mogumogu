//! Drafting and approving plans (F-22). A draft never changes anything on
//! disk; approval binds a person's confirmation to the exact fingerprint.
use super::RULE_VERSION;
use super::gate::{allowed, locate, unprotected};
use crate::domain::{JournalState, PlanState, PlanSummary, PlanTarget, Scope, ScopeKind, ScopeStatus};
use crate::limits::CLEANUP_MAX_TARGETS;
use crate::platform::FileIdentity;
use crate::storage::{NewTarget, Repository};
use crate::{Error, Result};
use sha2::{Digest, Sha256};

/// A recoverable folder from a fresh tidy-up walk of the project.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub rel_path: String,
    pub bytes: u64,
}

/// Drafts a plan for recoverable folders of one approved project. Each one
/// is checked on handles now; folders that fail keep their blockers in the
/// plan, so the person sees why nothing will happen.
pub fn draft(repo: &mut Repository, scope: &Scope, candidates: &[Candidate]) -> Result<i64> {
    repo.require_local()?;
    if scope.kind != ScopeKind::Project || scope.status != ScopeStatus::Approved {
        return Err(Error::blocked("Bereinigung nur in Projekten mit Lesefreigabe."));
    }
    if candidates.is_empty() || candidates.len() > CLEANUP_MAX_TARGETS {
        return Err(Error::invalid(format!("Ein Plan umfasst 1–{CLEANUP_MAX_TARGETS} wiederherstellbare Ordner.")));
    }
    let roots = repo.disposable_roots()?;
    let resources = repo.resources()?;
    let identity = repo.scope_identity(scope.id)?;
    let mut targets = Vec::new();
    for candidate in candidates {
        let mut target = NewTarget {
            scope_id: scope.id,
            rel_path: candidate.rel_path.clone(),
            identity: FileIdentity { volume: 0, id: [0; 16] },
            label: String::new(),
            evidence: String::new(),
            bytes: candidate.bytes,
            blockers: Vec::new(),
        };
        let checked = locate(&scope.path, identity, &candidate.rel_path).and_then(|located| {
            let paths = [located.path.to_string_lossy().into_owned(), located.shell.to_string_lossy().into_owned()];
            Ok((located.inspect()?, paths))
        });
        match checked {
            Ok(((identity, label, evidence), [path, shell])) => {
                target.identity = identity;
                target.label = label.into();
                target.evidence = evidence;
                if let Err(blocker) = unprotected(&resources, &[&path, &shell]) {
                    target.blockers.push(blocker);
                }
                if let Err(blocker) = allowed(&roots, &path, identity) {
                    target.blockers.push(blocker);
                }
            }
            Err(blocker) => {
                target.label = "Ordner".into();
                target.blockers.push(blocker);
            }
        }
        targets.push(target);
    }
    let fingerprint = fingerprint(scope.id, &targets);
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

fn fingerprint(scope_id: i64, targets: &[NewTarget]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(RULE_VERSION.as_bytes());
    hasher.update(scope_id.to_le_bytes());
    for target in targets {
        for part in [&target.rel_path, &target.identity.to_string(), &target.evidence] {
            hasher.update(part.as_bytes());
            hasher.update([0]);
        }
    }
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Human-readable plan with what stays, recovery and progress.
pub fn summary(repo: &mut Repository, plan_id: i64) -> Result<PlanSummary> {
    let plan = repo.plan(plan_id)?;
    let scope = match plan.targets.first() {
        Some(target) => Some(repo.scope(target.scope_id)?),
        None => None,
    };
    let project_id = scope.as_ref().and_then(|s| s.project_id);
    let project = match project_id {
        Some(id) => repo.projects()?.into_iter().find(|p| p.id == id).map(|p| p.name),
        None => None,
    };
    use JournalState::*;
    let count = |states: &[JournalState]| plan.targets.iter().filter(|t| states.contains(&t.state)).count() as i64;
    Ok(PlanSummary {
        id: plan.id,
        state: plan.state,
        created_at: plan.created_at,
        fingerprint: plan.fingerprint.clone(),
        project_id,
        project: project.unwrap_or_else(|| "Projekt".into()),
        root: scope.map(|s| s.path).unwrap_or_default(),
        done: count(&[Done, ReconciledMissing]),
        failed: count(&[Failed, Blocked, Pending]),
        targets: plan
            .targets
            .iter()
            .map(|t| PlanTarget {
                id: t.id,
                rel_path: t.rel_path.clone(),
                label: t.label.clone(),
                evidence: t.evidence.clone(),
                bytes: t.bytes,
                blockers: t.blockers.clone(),
                state: t.state,
                error: t.error.clone(),
            })
            .collect(),
        kept: vec![
            "Quellen, Projekt- und Lockfiles".into(),
            "Versionsverwaltung (.git)".into(),
            "Alles unter „Prüfen“: Testdaten, Datenbanken, Logs, große Dateien".into(),
            "Jeder Ordner, der nicht im Plan steht".into(),
        ],
        recovery: "Die Ordner landen im Windows-Papierkorb und lassen sich dort wiederherstellen. Neu erzeugen \
                   lassen sie sich mit dem Werkzeug aus dem Beleg (etwa npm ci, cargo build oder uv sync). Der \
                   Speicher wird erst frei, wenn du den Papierkorb leerst."
            .into(),
        note: plan.note,
    })
}
