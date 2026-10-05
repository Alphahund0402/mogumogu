//! Versioned command protocol shared by IPC and dashboard. Registration,
//! program start, protection changes and plan confirmation are separate
//! commands; none of them grants another.
use super::Core;
use crate::domain::{ProcessIdentity, ScopeKind, Settings};
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
    Why {
        resource_id: i64,
    },
    ScratchRegister {
        project_id: i64,
        name: String,
        path: String,
    },
    ScratchCreate {
        project_id: i64,
        name: String,
        purpose: String,
        review_days: Option<i64>,
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
    MarkExpendable {
        resource_id: i64,
    },
    Extend {
        resource_id: i64,
        days: i64,
    },
    Promote {
        resource_id: i64,
    },
    SessionStart {
        scratch_id: i64,
        purpose: String,
    },
    SessionAttach {
        session_id: i64,
        pid: u32,
        created: u64,
    },
    SessionHeartbeat {
        session_id: i64,
    },
    SessionReport {
        session_id: i64,
        exit_code: i64,
        remaining: i64,
    },
    SessionRegister {
        scratch_id: i64,
        purpose: String,
        pid: Option<u32>,
    },
    SessionComplete {
        session_id: i64,
    },
    SessionRelease {
        session_id: i64,
        reason: String,
    },
    Sessions,
    Review,
    CleanupPlan {
        resource_ids: Vec<i64>,
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
    SaveSettings {
        settings: Settings,
    },
    Updates,
    UpdateList,
    Export {
        include_paths: bool,
    },
    Backup {
        path: String,
    },
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
                | Self::Sessions
                | Self::Review
                | Self::GetSettings
                | Self::UpdateList
                | Self::Export { .. }
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
            Why { resource_id } => json!(self.explain(resource_id)?),
            ScratchRegister { project_id, name, path } => {
                json!({ "resource_id": self.register_scratch(project_id, &name, &path)?, "protected": true, "scanned": false })
            }
            ScratchCreate { project_id, name, purpose, review_days } => {
                json!({ "resource_id": self.create_scratch(project_id, &name, &purpose, review_days)? })
            }
            ScratchAdopt { project_id, name, path, purpose } => {
                json!({ "resource_id": self.adopt_scratch(project_id, &name, &path, purpose.as_deref())?, "protected": true })
            }
            Protect { resource_id } => {
                self.protect(resource_id)?;
                json!({ "protected": resource_id })
            }
            MarkExpendable { resource_id } => {
                self.mark_expendable(resource_id)?;
                json!({ "expendable": resource_id, "note": "Ein Plan muss separat erstellt und bestätigt werden." })
            }
            Extend { resource_id, days } => json!({ "review_at": self.extend_review(resource_id, days)? }),
            Promote { resource_id } => json!({ "project_id": self.promote(resource_id)? }),
            SessionStart { scratch_id, purpose } => json!(self.session_start(scratch_id, &purpose)?),
            SessionAttach { session_id, pid, created } => {
                json!(self.session_attach(session_id, ProcessIdentity { pid, created })?)
            }
            SessionHeartbeat { session_id } => {
                self.session_heartbeat(session_id)?;
                json!({ "ok": true })
            }
            SessionReport { session_id, exit_code, remaining } => {
                json!(self.session_report(session_id, exit_code, remaining)?)
            }
            SessionRegister { scratch_id, purpose, pid } => json!(self.session_register(scratch_id, &purpose, pid)?),
            SessionComplete { session_id } => json!(self.session_complete(session_id)?),
            SessionRelease { session_id, reason } => json!(self.session_release(session_id, &reason)?),
            Sessions => {
                self.reconcile_sessions()?;
                json!(self.repo.sessions()?)
            }
            Review => json!(self.review()?),
            CleanupPlan { resource_ids } => json!(self.cleanup_plan(&resource_ids)?),
            CleanupApprove { plan_id, confirmation } => json!(self.cleanup_approve(plan_id, &confirmation)?),
            CleanupApply { plan_id } => json!(self.cleanup_apply(plan_id)?),
            CleanupCancel { plan_id } => json!(self.cleanup_cancel(plan_id)?),
            CleanupReconcile { plan_id } => json!(self.cleanup_reconcile(plan_id)?),
            RegisterDisposableRoot { path } => json!({ "root_id": self.register_disposable_root(&path)? }),
            GetSettings => json!(self.settings()?),
            SaveSettings { settings } => {
                self.save_settings(&settings)?;
                json!(settings)
            }
            Updates => json!(self.check_updates()?),
            UpdateList => json!(self.update_records()?),
            Export { include_paths } => self.export(include_paths)?,
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
