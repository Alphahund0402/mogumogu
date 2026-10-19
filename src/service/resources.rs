//! Scratchpads, resource decisions and the evidence behind review hints.
use super::Core;
use crate::clock::Timestamp;
use crate::domain::*;
use crate::review::{self, ReviewInput};
use crate::sessions;
use crate::{Error, Result, validation};
use serde::Serialize;

/// "Warum ist das hier?" for one resource.
#[derive(Clone, Debug, Serialize)]
pub struct Explanation {
    pub resource: Resource,
    pub owners: Vec<OwnerLink>,
    pub sessions: Vec<Session>,
    pub children: Vec<Resource>,
    pub hints: Vec<ReviewHint>,
    pub protection: String,
}

impl Core {
    pub fn explain(&mut self, resource_id: i64) -> Result<Explanation> {
        self.reconcile_sessions()?;
        let resource = self.repo.resource(resource_id)?;
        let owners: Vec<OwnerLink> = self.repo.owners()?.into_iter().filter(|o| o.resource_id == resource_id).collect();
        let scratch = if resource.kind == ResourceKind::Scratchpad { Some(resource.id) } else { resource.parent_id };
        let sessions = match scratch {
            Some(id) => self.repo.sessions_for_scratch(id)?,
            None => Vec::new(),
        };
        let children = self.repo.children(resource_id)?;
        let hints = self.review()?.into_iter().filter(|h| h.resource_id == resource_id).collect();
        let protection = if resource.protected {
            "Geschützt. Registrierung, Alter oder fehlende Nutzung belegen keine Entbehrlichkeit.".into()
        } else if resource.expendable {
            "Lokal als entbehrlich markiert. Bereinigung nur über einen bestätigten Plan.".into()
        } else {
            "Nicht geschützt, aber auch nicht als entbehrlich markiert.".into()
        };
        Ok(Explanation { resource, owners, sessions, children, hints, protection })
    }

    pub fn register_scratch(&mut self, project_id: i64, name: &str, path: &str) -> Result<i64> {
        self.repo.register_scratch(project_id, name, path)
    }

    pub fn adopt_scratch(&mut self, project_id: i64, name: &str, path: &str, purpose: Option<&str>) -> Result<i64> {
        self.repo.adopt_scratch(project_id, name, path, purpose)
    }

    /// Creates a managed scratchpad below the configured scratch root.
    pub fn create_scratch(
        &mut self,
        project_id: i64,
        name: &str,
        purpose: &str,
        review_days: Option<i64>,
    ) -> Result<i64> {
        self.repo.require_local()?;
        let settings = self.repo.settings()?;
        let root = settings
            .scratch_root
            .ok_or_else(|| Error::invalid("Zuerst eine Scratchpad-Wurzel in den Einstellungen festlegen."))?;
        validation::name(name)?;
        self.repo.project(project_id)?;
        let review_days = review_days.unwrap_or(settings.scratch_review_days);
        if !(1..=365).contains(&review_days) {
            return Err(Error::invalid("Prüftermin: 1–365 Tage."));
        }
        let (scratch, children) = sessions::create_layout(&root, name, self.repo.now())?;
        self.repo.record_managed_scratch(project_id, name, purpose, &scratch, &children, review_days)
    }

    pub fn protect(&mut self, resource_id: i64) -> Result<()> {
        self.repo.protect(resource_id)
    }

    pub fn mark_expendable(&mut self, resource_id: i64) -> Result<()> {
        self.repo.mark_expendable(resource_id)
    }

    pub fn extend_review(&mut self, resource_id: i64, days: i64) -> Result<Timestamp> {
        self.repo.extend_review(resource_id, days)
    }

    pub fn promote(&mut self, resource_id: i64) -> Result<i64> {
        self.repo.promote(resource_id)
    }

    pub fn review(&mut self) -> Result<Vec<ReviewHint>> {
        let resources = self.repo.resources()?;
        let sessions = self.repo.sessions()?;
        let owners = self.repo.owners()?;
        let settings = self.settings()?;
        let input = ReviewInput {
            resources: &resources,
            sessions: &sessions,
            owners: &owners,
            settings: &settings,
            now: self.repo.now(),
            demo: self.is_demo(),
        };
        Ok(review::evaluate(&input))
    }
}
