//! Versioned command protocol shared by IPC and dashboard. Registration,
//! program start, protection changes and plan confirmation are separate
//! commands; none of them grants another.
use super::Core;
use crate::domain::{PackageQuery, ScopeKind, Settings};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Status,
    Snapshot,
    Adapters,
    Catalog,
    RegisterProject {
        name: String,
        path: String,
    },
    ApproveProject {
        project_id: i64,
    },
    ApproveSystem {
        kind: ScopeKind,
        path: Option<String>,
    },
    RevokeScope {
        scope_id: i64,
    },
    Scan {
        scope_id: Option<i64>,
    },
    MeasureResource {
        resource_id: i64,
    },
    MeasureScope {
        scope_id: i64,
    },
    StorageOverview,
    AiContent {
        scope_id: Option<i64>,
        #[serde(default)]
        rel_path: String,
    },
    Why {
        resource_id: i64,
    },
    ScratchRegister {
        project_id: i64,
        name: String,
        path: String,
    },
    ScratchAdopt {
        project_id: i64,
        name: String,
        path: String,
        purpose: Option<String>,
    },
    Protect {
        resource_id: i64,
    },
    Extend {
        resource_id: i64,
        days: i64,
    },
    Promote {
        resource_id: i64,
    },
    Review,
    /// Recoverable folders of one approved project; all when `paths` is empty.
    CleanupPlan {
        project_id: i64,
        #[serde(default)]
        paths: Vec<String>,
    },
    CleanupApprove {
        plan_id: i64,
        confirmation: String,
    },
    CleanupApply {
        plan_id: i64,
    },
    CleanupCancel {
        plan_id: i64,
    },
    CleanupReconcile {
        plan_id: i64,
    },
    RegisterDisposableRoot {
        path: String,
    },
    GetSettings,
    /// Tidy-up report for all approved projects, or one.
    Tidy {
        project_id: Option<i64>,
    },
    /// Takes over every untracked scratchpad finding, protected.
    AdoptScratchpads,
    /// Preview of a context filter for AI tools in one approved project.
    ContextFilter {
        scope_id: i64,
    },
    /// Writes the previewed filter; the fingerprint binds it to the preview.
    ApplyContextFilter {
        scope_id: i64,
        fingerprint: String,
    },
    /// Preview of the context filter for every approved project.
    ContextFilterAll,
    /// Writes the previewed batch, project by project.
    ApplyContextFilterAll {
        fingerprint: String,
    },
    SaveSettings {
        settings: Settings,
    },
    Packages {
        offset: usize,
        limit: usize,
        #[serde(default)]
        query: PackageQuery,
    },
    /// Metadata-only system check; `refresh` checks again instead of the cache.
    SystemFindings {
        #[serde(default)]
        refresh: bool,
    },
    Search {
        query: String,
    },
    RetainPackage {
        key: String,
        retained: bool,
    },
    RegisterCandidate {
        scope_id: i64,
        rel_path: String,
    },
    ProjectCandidates,
    /// Certain outer projects of complete searches, before anything is registered.
    FoundProjects {
        #[serde(default)]
        scope_id: Option<i64>,
    },
    /// Registers and approves the previewed projects; the fingerprint binds it.
    AdoptFoundProjects {
        #[serde(default)]
        scope_id: Option<i64>,
        fingerprint: String,
    },
    DiscoveryCandidate {
        scope_id: i64,
        rel_path: String,
    },
    AdoptCandidate {
        scope_id: i64,
        rel_path: String,
        project_id: i64,
        purpose: Option<String>,
    },
    Backup {
        path: String,
    },
    /// Per-user start entry of the tray owner.
    /// Owner process control; handled by the desktop/headless host.
    ShowDashboard,
    HideDashboard,
    Shutdown,
}

impl Request {
    /// Whether the request changes state (used for logging and UI refresh).
    pub fn is_write(&self) -> bool {
        !matches!(
            self,
            Self::Status
                | Self::Snapshot
                | Self::Adapters
                | Self::Catalog
                | Self::Why { .. }
                | Self::Review
                | Self::GetSettings
                | Self::ContextFilter { .. }
                | Self::ContextFilterAll
                | Self::Packages { .. }
                | Self::SystemFindings { .. }
                | Self::Search { .. }
                | Self::ProjectCandidates
                | Self::FoundProjects { .. }
                | Self::DiscoveryCandidate { .. }
                | Self::StorageOverview
                | Self::AiContent { .. }
                | Self::ShowDashboard
                | Self::HideDashboard
                | Self::Shutdown
        )
    }
}

impl Core {
    /// Executes one request and returns its JSON result.
    pub fn dispatch(&mut self, request: Request) -> Result<Value> {
        use Request::*;
        let value = match request {
            Status => json!(self.status()?),
            Snapshot => json!(self.snapshot()?),
            Adapters => json!(crate::adapters::descriptors()),
            Catalog => json!(catalog_view()),
            RegisterProject { name, path } => {
                json!({ "project_id": self.register_project(&name, &path)?, "protected": true, "scanned": false })
            }
            ApproveProject { project_id } => json!({ "scope_id": self.approve_project(project_id)? }),
            ApproveSystem { kind, path } => json!({ "scope_id": self.approve_system(kind, path.as_deref())? }),
            RevokeScope { scope_id } => {
                self.revoke_scope(scope_id)?;
                json!({ "revoked": scope_id })
            }
            Scan { scope_id: Some(id) } => json!([self.scan(id)?]),
            Scan { scope_id: None } => json!(self.scan_all()?),
            MeasureResource { resource_id } => json!(measurement(self.measure_resource(resource_id)?)),
            MeasureScope { scope_id } => json!(measurement(self.measure_scope(scope_id)?)),
            StorageOverview => {
                self.storage_cache = None;
                json!(self.storage_overview()?)
            }
            AiContent { scope_id, rel_path } => json!(self.ai_content(scope_id, &rel_path)?),
            Why { resource_id } => json!(self.explain(resource_id)?),
            ScratchRegister { project_id, name, path } => {
                json!({ "resource_id": self.register_scratch(project_id, &name, &path)?, "protected": true, "scanned": false })
            }
            ScratchAdopt { project_id, name, path, purpose } => {
                json!({ "resource_id": self.adopt_scratch(project_id, &name, &path, purpose.as_deref())?, "protected": true })
            }
            Protect { resource_id } => {
                self.protect(resource_id)?;
                json!({ "protected": resource_id })
            }
            Extend { resource_id, days } => json!({ "review_at": self.extend_review(resource_id, days)? }),
            Promote { resource_id } => json!({ "project_id": self.promote(resource_id)? }),
            Review => json!(self.review()?),
            CleanupPlan { project_id, paths } => json!(self.cleanup_plan(project_id, &paths)?),
            CleanupApprove { plan_id, confirmation } => json!(self.cleanup_approve(plan_id, &confirmation)?),
            CleanupApply { plan_id } => json!(self.cleanup_apply(plan_id)?),
            CleanupCancel { plan_id } => json!(self.cleanup_cancel(plan_id)?),
            CleanupReconcile { plan_id } => json!(self.cleanup_reconcile(plan_id)?),
            RegisterDisposableRoot { path } => json!({ "root_id": self.register_disposable_root(&path)? }),
            GetSettings => json!(self.settings()?),
            Tidy { project_id } => json!(self.tidy_report(project_id)?),
            AdoptScratchpads => json!({ "adopted": self.adopt_found_scratchpads()? }),
            ContextFilter { scope_id } => json!(self.context_filter(scope_id)?),
            ApplyContextFilter { scope_id, fingerprint } => json!(self.apply_context_filter(scope_id, &fingerprint)?),
            ContextFilterAll => json!(self.context_filter_all()?),
            ApplyContextFilterAll { fingerprint } => json!(self.apply_context_filter_all(&fingerprint)?),
            SaveSettings { settings } => {
                self.save_settings(&settings)?;
                json!(settings)
            }
            Packages { offset, limit, query } => json!(self.package_page(&query, offset, limit)?),
            SystemFindings { refresh } => json!(self.system_report(refresh)?),
            Search { query } => json!(self.search(&query)?),
            RetainPackage { key, retained } => {
                self.retain_package(&key, retained)?;
                json!({"retained":retained})
            }
            RegisterCandidate { scope_id, rel_path } => {
                json!({"project_id":self.register_candidate(scope_id,&rel_path)?,"scanned":false})
            }
            ProjectCandidates => json!(self.project_candidates()?),
            FoundProjects { scope_id } => json!(self.found_projects(scope_id)?),
            AdoptFoundProjects { scope_id, fingerprint } => json!(self.adopt_found_projects(scope_id, &fingerprint)?),
            DiscoveryCandidate { scope_id, rel_path } => json!(self.discovery_candidate(scope_id, &rel_path)?),
            AdoptCandidate { scope_id, rel_path, project_id, purpose } => {
                json!({"resource_id": self.adopt_candidate(scope_id, &rel_path, project_id, purpose.as_deref())?, "protected": true})
            }
            Backup { path } => json!({ "backup": self.backup(&path)? }),
            ShowDashboard | HideDashboard | Shutdown => {
                return Err(Error::unsupported("Nur der Besitzerprozess steuert sein Fenster."));
            }
        };
        Ok(value)
    }
}

fn measurement(m: crate::inventory::Measurement) -> Value {
    json!({
        "bytes": m.bytes,
        "known": m.bytes.is_some(),
        "entries": m.entries,
        "skipped_links": m.skipped_links,
        "reason": m.reason,
    })
}

fn catalog_view() -> Value {
    let catalog = crate::profiles::catalog();
    json!({
        "version": catalog.version,
        "profiles": catalog.profiles.iter().map(|p| json!({
            "id": p.id,
            "name": p.name,
            "status": p.status,
            "source": p.source,
            "patterns": p.detect.iter().map(|d| &d.pattern).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_rejects_unknown_fields_and_commands() {
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"status"}"#).is_ok());
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"delete_everything"}"#).is_err());
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"cleanup_apply","plan_id":1,"force":true}"#).is_err());
    }
}
