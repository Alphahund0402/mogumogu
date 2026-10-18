//! Status is a summary query, independent of the dashboard read model.
use super::Core;
use crate::{Result, cleanup, profiles};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub mode: &'static str,
    pub data_dir: String,
    pub schema_version: i64,
    pub sqlite_version: &'static str,
    pub projects: usize,
    pub resources: usize,
    pub scopes: usize,
    pub open_sessions: usize,
    pub catalog: String,
    pub cleanup: &'static str,
    pub network: &'static str,
}

impl Core {
    pub fn status(&mut self) -> Result<Status> {
        self.reconcile_sessions()?;
        let counts = self.repo.status_counts()?;
        Ok(Status {
            mode: if self.is_demo() { "Demo" } else { "Lokal" },
            data_dir: self.config.directory.to_string_lossy().into_owned(),
            schema_version: crate::storage::SCHEMA_VERSION,
            sqlite_version: self.repo.sqlite_version(),
            projects: counts.projects,
            resources: counts.resources,
            scopes: counts.scopes,
            open_sessions: counts.open_sessions,
            catalog: profiles::catalog().basis(),
            cleanup: if cleanup::PRODUCTION_ENABLED {
                "produktiv freigegeben"
            } else {
                "nur Testexecutor in registrierten Wegwerf-Testwurzeln (G5 offen)"
            },
            network: if cfg!(feature = "network") { "opt-in, standardmäßig aus" } else { "nicht einkompiliert" },
        })
    }
}
