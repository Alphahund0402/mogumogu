//! Scratchpads, resource decisions and the evidence behind review hints.
use super::Core;
use crate::Result;
use crate::clock::Timestamp;
use crate::domain::*;
use crate::review::{self, ReviewInput};
use serde::Serialize;

/// "Warum ist das hier?" for one resource.
#[derive(Clone, Debug, Serialize)]
pub struct Explanation {
    pub resource: Resource,
    pub owners: Vec<OwnerLink>,
    pub children: Vec<Resource>,
    pub hints: Vec<ReviewHint>,
    pub protection: String,
}

impl Core {
    pub fn explain(&mut self, resource_id: i64) -> Result<Explanation> {
        let resource = self.repo.resource(resource_id)?;
        let owners: Vec<OwnerLink> = self.repo.owners()?.into_iter().filter(|o| o.resource_id == resource_id).collect();
        let children = self.repo.children(resource_id)?;
        let hints = self.review()?.into_iter().filter(|h| h.resource_id == resource_id).collect();
        let protection = if resource.protected {
            "Geschützt. Registrierung, Alter oder fehlende Nutzung belegen keine Entbehrlichkeit.".into()
        } else {
            "Nicht geschützt. Entfernt wird trotzdem nur über einen bestätigten Plan.".into()
        };
        Ok(Explanation { resource, owners, children, hints, protection })
    }

    pub fn register_scratch(&mut self, project_id: i64, name: &str, path: &str) -> Result<i64> {
        self.repo.register_scratch(project_id, name, path)
    }

    pub fn adopt_scratch(&mut self, project_id: i64, name: &str, path: &str, purpose: Option<&str>) -> Result<i64> {
        self.repo.adopt_scratch(project_id, name, path, purpose)
    }

    pub fn protect(&mut self, resource_id: i64) -> Result<()> {
        self.repo.protect(resource_id)
    }

    pub fn extend_review(&mut self, resource_id: i64, days: i64) -> Result<Timestamp> {
        self.repo.extend_review(resource_id, days)
    }

    pub fn promote(&mut self, resource_id: i64) -> Result<i64> {
        self.repo.promote(resource_id)
    }

    pub fn review(&mut self) -> Result<Vec<ReviewHint>> {
        let resources = self.repo.resources()?;
        let owners = self.repo.owners()?;
        let settings = self.settings()?;
        let input = ReviewInput {
            resources: &resources,
            owners: &owners,
            settings: &settings,
            now: self.repo.now(),
            demo: self.is_demo(),
        };
        Ok(review::evaluate(&input))
    }
}
