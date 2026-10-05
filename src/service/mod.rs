//! Use cases shared by dashboard, CLI and IPC (PROJEKTPLAN §5.2: "UI und
//! CLI rufen dieselben validierten Anwendungsfälle im Kern auf").
//!
//! `Core` is owned by exactly one thread of the owner process; every write
//! goes through it. It holds no UI types.
mod request;
mod snapshot;

pub use request::Request;

use crate::cleanup::{self, DISPOSABLE_MARKER, DISPOSABLE_MARKER_TEXT, NoHooks};
use crate::clock::{Clock, SystemClock, Timestamp};
use crate::config::Config;
use crate::domain::*;
use crate::inventory::{self, ScanFailure, ScanRequest};
use crate::limits::ScanLimits;
use crate::platform::{self, Job};
use crate::profiles;
use crate::review::{self, ReviewInput};
use crate::sessions::{self, StartInfo};
use crate::storage::{PublishReport, Repository, SessionUpdate};
use crate::updates::{self, Registry, UpdateSummary};
use crate::{Error, Result, validation};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Core {
    config: Config,
    repo: Repository,
    registry: Box<dyn Registry + Send>,
    /// Job handles of managed sessions, held so jobs stay queryable.
    jobs: HashMap<i64, Job>,
    watched: Vec<i64>,
    /// Set when reconciliation changed stored state outside a write request.
    changed: bool,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core").field("config", &self.config).field("jobs", &self.jobs.len()).finish()
    }
}

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

#[derive(Clone, Debug, Serialize)]
pub struct ScanReport {
    pub scope_id: i64,
    pub state: Option<GenerationState>,
    pub baseline: bool,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub problem: Option<String>,
}

fn default_registry() -> Box<dyn Registry + Send> {
    #[cfg(feature = "network")]
    {
        Box::new(updates::HttpRegistry::default())
    }
    #[cfg(not(feature = "network"))]
    {
        Box::new(updates::NoNetwork)
    }
}

impl Core {
    /// Opens the owner core: database, crash recovery, session reconciliation.
    pub fn open(config: Config) -> Result<Self> {
        Self::open_with(config, Arc::new(SystemClock), default_registry())
    }

    pub fn open_with(config: Config, clock: Arc<dyn Clock>, registry: Box<dyn Registry + Send>) -> Result<Self> {
        let repo = Repository::open(&config, clock)?;
        let mut core = Self { config, repo, registry, jobs: HashMap::new(), watched: Vec::new(), changed: false };
        core.repo.recover_after_restart()?;
        core.reconcile_sessions()?;
        Ok(core)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn repository(&mut self) -> &mut Repository {
        &mut self.repo
    }

    pub fn is_demo(&self) -> bool {
        self.repo.is_demo()
    }

    /// True once after reconciliation changed stored state.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Whether sessions still wait for evidence (owner re-checks them).
    pub fn has_open_sessions(&mut self) -> bool {
        !self.is_demo() && self.repo.open_sessions().map(|s| !s.is_empty()).unwrap_or(false)
    }

    /// Scope ids currently covered by the desktop watcher.
    pub fn set_watched(&mut self, scopes: Vec<i64>) {
        self.watched = scopes;
    }

    pub fn status(&mut self) -> Result<Status> {
        let snapshot = self.snapshot()?;
        Ok(Status {
            mode: if self.is_demo() { "Demo" } else { "Lokal" },
            data_dir: self.config.directory.to_string_lossy().into_owned(),
            schema_version: crate::storage::SCHEMA_VERSION,
            sqlite_version: self.repo.sqlite_version(),
            projects: snapshot.projects.len(),
            resources: snapshot.resources.len(),
            scopes: snapshot.scopes.len(),
            open_sessions: snapshot.sessions.iter().filter(|s| s.state.locks_resources()).count(),
            catalog: profiles::catalog().basis(),
            cleanup: if cleanup::PRODUCTION_ENABLED {
                "produktiv freigegeben"
            } else {
                "nur Testexecutor in registrierten Wegwerf-Testwurzeln (G5 offen)"
            },
            network: if cfg!(feature = "network") { "opt-in, standardmäßig aus" } else { "nicht einkompiliert" },
        })
    }

    // ------------------------------------------------------------ projects

    pub fn register_project(&mut self, name: &str, path: &str) -> Result<i64> {
        self.repo.register_project(name, path)
    }

    /// Read approval for a registered project folder, bound to its identity.
    pub fn approve_project(&mut self, project_id: i64) -> Result<i64> {
        let project = self.repo.project(project_id)?;
        let identity = platform::path_identity(Path::new(&project.path))
            .map_err(|e| Error::blocked(format!("Ordner nicht freigebbar: {e}")))?;
        self.repo.approve_scope(ScopeKind::Project, Some(project_id), &project.path, Some(identity))
    }

    /// Read approval for a system-wide manager root or the software list.
    pub fn approve_system(&mut self, kind: ScopeKind, path: Option<&str>) -> Result<i64> {
        match kind {
            ScopeKind::Project => Err(Error::invalid("Projektordner werden über ihr Projekt freigegeben.")),
            ScopeKind::Winget => self.repo.approve_scope(kind, None, "Registry: installierte Programme", None),
            ScopeKind::Scoop | ScopeKind::Chocolatey => {
                let path = match path {
                    Some(path) => validation::windows_path(path)?,
                    None => default_system_root(kind)?,
                };
                let identity = platform::path_identity(Path::new(&path))
                    .map_err(|e| Error::blocked(format!("Ordner nicht freigebbar: {e}")))?;
                self.repo.approve_scope(kind, None, &path, Some(identity))
            }
        }
    }

    pub fn revoke_scope(&mut self, scope_id: i64) -> Result<()> {
        self.repo.revoke_scope(scope_id)
    }

    /// One static pass over an approved scope, stored as a generation.
    pub fn scan(&mut self, scope_id: i64) -> Result<ScanReport> {
        self.repo.require_local()?;
        let scope = self.repo.scope(scope_id)?;
        match scope.status {
            ScopeStatus::Approved | ScopeStatus::Unavailable => {}
            ScopeStatus::IdentityChanged => {
                return Err(Error::blocked("Der freigegebene Ordner wurde ersetzt. Bitte bewusst neu freigeben."));
            }
            ScopeStatus::Revoked => return Err(Error::blocked("Lesefreigabe wurde widerrufen.")),
        }
        let request = ScanRequest {
            kind: scope.kind,
            path: PathBuf::from(&scope.path),
            identity: self.repo.scope_identity(scope_id)?,
            limits: ScanLimits::default(),
        };
        let generation = self.repo.begin_generation(scope_id, &profiles::catalog().basis())?;
        let outcome = inventory::scan(&request);
        let problem = match &outcome.failure {
            Some(ScanFailure::IdentityChanged) => {
                self.repo.set_scope_status(scope_id, ScopeStatus::IdentityChanged, Some("Ordner ersetzt"))?;
                Some("Ordner wurde ersetzt; Freigabe blockiert.".to_string())
            }
            Some(ScanFailure::Unavailable(reason)) => {
                self.repo.set_scope_status(scope_id, ScopeStatus::Unavailable, Some(reason))?;
                Some(reason.clone())
            }
            None => {
                if scope.status != ScopeStatus::Approved {
                    self.repo.set_scope_status(scope_id, ScopeStatus::Approved, None)?;
                }
                None
            }
        };
        let PublishReport { state, baseline, added, removed, changed, .. } =
            self.repo.finish_generation(generation, &outcome.result)?;
        Ok(ScanReport { scope_id, state, baseline, added, removed, changed, problem })
    }

    pub fn scan_all(&mut self) -> Result<Vec<ScanReport>> {
        let ids: Vec<i64> =
            self.repo.scopes()?.iter().filter(|s| s.status == ScopeStatus::Approved).map(|s| s.id).collect();
        ids.into_iter().map(|id| self.scan(id)).collect()
    }

    /// Measures a managed folder or an approved scope; unknown stays unknown.
    pub fn measure_resource(&mut self, resource_id: i64) -> Result<inventory::Measurement> {
        let resource = self.repo.resource(resource_id)?;
        let identity = self.repo.resource_identity(resource_id)?;
        if resource.acquisition != Acquisition::Managed || identity.is_none() {
            return Err(Error::blocked(
                "Größen werden nur für von mogumogu angelegte Ordner oder freigegebene Suchbereiche gemessen.",
            ));
        }
        let measurement = inventory::measure(Path::new(&resource.path), identity);
        self.repo.record_size(resource_id, measurement.bytes.map(|b| b as i64))?;
        Ok(measurement)
    }

    pub fn measure_scope(&mut self, scope_id: i64) -> Result<inventory::Measurement> {
        let scope = self.repo.scope(scope_id)?;
        if scope.status != ScopeStatus::Approved || scope.kind == ScopeKind::Winget {
            return Err(Error::blocked("Nur freigegebene Ordner können gemessen werden."));
        }
        let measurement = inventory::measure(Path::new(&scope.path), self.repo.scope_identity(scope_id)?);
        if let Some(project) = scope.project_id {
            self.repo.record_project_size(project, measurement.bytes.map(|b| b as i64))?;
        }
        Ok(measurement)
    }

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

    // ---------------------------------------------------------- scratchpads

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

    // ------------------------------------------------------------- sessions

    /// Starts a managed session and tells the starter where to route TEMP.
    pub fn session_start(&mut self, scratch_id: i64, purpose: &str) -> Result<StartInfo> {
        let children = self.repo.children(scratch_id)?;
        let folder = |kind| {
            children
                .iter()
                .find(|c| c.kind == kind && c.acquisition == Acquisition::Managed)
                .map(|c| c.path.clone())
                .ok_or_else(|| Error::invalid("Verwaltete Läufe benötigen einen von mogumogu angelegten Scratchpad."))
        };
        let temp_dir = folder(ResourceKind::Temporary)?;
        let results_dir = folder(ResourceKind::Results)?;
        let prefix = self.config.job_prefix()?;
        let session_id = self.repo.create_session(scratch_id, purpose, SessionOrigin::Managed, Some(&prefix))?;
        Ok(StartInfo { session_id, temp_dir, results_dir, job_name: format!("{prefix}{session_id}") })
    }

    pub fn session_attach(&mut self, session_id: i64, process: ProcessIdentity) -> Result<Session> {
        let session = self.repo.session(session_id)?;
        if let Some(name) = &session.job_name
            && let Some(job) = Job::open(name)?
        {
            self.jobs.insert(session_id, job);
        }
        self.repo.transition_session(
            session_id,
            SessionState::Running,
            SessionUpdate { process: Some(process), ..Default::default() },
        )
    }

    pub fn session_heartbeat(&mut self, session_id: i64) -> Result<()> {
        self.repo.session_heartbeat(session_id)
    }

    /// The starter reports completion. This is a claim, not proof.
    pub fn session_report(&mut self, session_id: i64, exit_code: i64, remaining: i64) -> Result<Session> {
        let update =
            SessionUpdate { exit_code: Some(exit_code), remaining: Some(remaining), ended: true, ..Default::default() };
        self.repo.transition_session(session_id, SessionState::CompletionRequested, update)?;
        self.reconcile_sessions()?;
        self.repo.session(session_id)
    }

    /// Cooperative session registration by an integration (weaker evidence).
    pub fn session_register(&mut self, scratch_id: i64, purpose: &str, pid: Option<u32>) -> Result<Session> {
        let id = self.repo.create_session(scratch_id, purpose, SessionOrigin::Cooperative, None)?;
        let process = match pid {
            Some(pid) => {
                Some(platform::process_identity(pid)?.ok_or_else(|| Error::invalid("Prozess nicht gefunden."))?)
            }
            None => None,
        };
        self.repo.transition_session(id, SessionState::Running, SessionUpdate { process, ..Default::default() })
    }

    pub fn session_complete(&mut self, session_id: i64) -> Result<Session> {
        let update = SessionUpdate { ended: true, ..Default::default() };
        self.repo.transition_session(session_id, SessionState::CompletionRequested, update)?;
        self.reconcile_sessions()?;
        self.repo.session(session_id)
    }

    /// Resolves an unclear lock after a person reviewed it. No cleanup
    /// approval follows from this; a new plan is still required.
    pub fn session_release(&mut self, session_id: i64, reason: &str) -> Result<Session> {
        let reason = reason.trim();
        if reason.chars().count() < 5 {
            return Err(Error::invalid("Bitte eine Begründung angeben (mindestens 5 Zeichen)."));
        }
        let update = SessionUpdate { note: Some(format!("Nach Prüfung freigegeben: {reason}")), ..Default::default() };
        self.repo.transition_session(session_id, SessionState::ReleasedAfterReview, update)
    }

    /// Applies the decision table to every open session.
    pub fn reconcile_sessions(&mut self) -> Result<usize> {
        if self.is_demo() {
            return Ok(0);
        }
        let mut changed = 0;
        for session in self.repo.open_sessions()? {
            let evidence = sessions::evidence(&session, self.jobs.get(&session.id));
            if let Some(decision) = sessions::decide(&session, evidence, self.repo.now()) {
                let update = SessionUpdate { note: Some(decision.note), ..Default::default() };
                self.repo.transition_session(session.id, decision.next, update)?;
                if !decision.next.locks_resources() || decision.next == SessionState::InterruptedUnknown {
                    self.jobs.remove(&session.id);
                }
                changed += 1;
            }
        }
        self.changed |= changed > 0;
        Ok(changed)
    }

    // --------------------------------------------------------------- review

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

    // -------------------------------------------------------------- cleanup

    pub fn cleanup_plan(&mut self, resource_ids: &[i64]) -> Result<PlanSummary> {
        self.reconcile_sessions()?;
        let id = cleanup::draft(&mut self.repo, resource_ids)?;
        cleanup::summary(&mut self.repo, id)
    }

    pub fn cleanup_approve(&mut self, plan_id: i64, confirmation: &str) -> Result<PlanSummary> {
        cleanup::approve(&mut self.repo, plan_id, confirmation)?;
        cleanup::summary(&mut self.repo, plan_id)
    }

    pub fn cleanup_apply(&mut self, plan_id: i64) -> Result<PlanSummary> {
        self.reconcile_sessions()?;
        let applied = cleanup::apply(&mut self.repo, plan_id, &mut NoHooks);
        let summary = cleanup::summary(&mut self.repo, plan_id)?;
        applied.map(|_| summary)
    }

    pub fn cleanup_cancel(&mut self, plan_id: i64) -> Result<PlanSummary> {
        let plan = cleanup::summary(&mut self.repo, plan_id)?;
        if !matches!(plan.state, PlanState::Draft | PlanState::Approved) {
            return Err(Error::conflict(
                "Nur Entwürfe und bestätigte, noch nicht gestartete Pläne können abgebrochen werden.",
            ));
        }
        self.repo.transition_plan(plan_id, plan.state, PlanState::Cancelled, Some("Lokal abgebrochen"))?;
        cleanup::summary(&mut self.repo, plan_id)
    }

    pub fn cleanup_reconcile(&mut self, plan_id: i64) -> Result<PlanSummary> {
        cleanup::reconcile(&mut self.repo, plan_id)?;
        cleanup::summary(&mut self.repo, plan_id)
    }

    /// Registers an explicitly disposable test root. It must carry the
    /// marker file and must not contain or lie inside a registered project.
    pub fn register_disposable_root(&mut self, path: &str) -> Result<i64> {
        self.repo.require_local()?;
        let path = validation::windows_path(path)?;
        let marker = std::fs::read_to_string(Path::new(&path).join(DISPOSABLE_MARKER)).unwrap_or_default();
        if marker.trim() != DISPOSABLE_MARKER_TEXT {
            return Err(Error::blocked(format!(
                "Die Datei {DISPOSABLE_MARKER} mit dem Inhalt „{DISPOSABLE_MARKER_TEXT}“ fehlt. Nur ausdrücklich \
                 markierte Wegwerf-Ordner sind zulässig."
            )));
        }
        let lower = path.to_lowercase();
        for project in self.repo.projects()? {
            let project_path = project.path.to_lowercase();
            if project_path.starts_with(&format!("{lower}\\"))
                || lower.starts_with(&format!("{project_path}\\"))
                || project_path == lower
            {
                return Err(Error::blocked(
                    "Eine Testwurzel darf kein registriertes Projekt enthalten oder in einem liegen.",
                ));
            }
        }
        let identity = platform::path_identity(Path::new(&path))?;
        self.repo.register_disposable_root(&path, identity)
    }

    // ------------------------------------------------------ settings & misc

    pub fn settings(&mut self) -> Result<Settings> {
        if self.is_demo() {
            return Ok(Settings::default());
        }
        self.repo.settings()
    }

    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        self.repo.save_settings(settings)
    }

    pub fn check_updates(&mut self) -> Result<UpdateSummary> {
        self.repo.require_local()?;
        let settings = self.repo.settings()?;
        if !settings.network_updates_enabled {
            return Err(Error::blocked("Updatehinweise sind ausgeschaltet. In den Einstellungen bewusst aktivieren."));
        }
        updates::run(&mut self.repo, self.registry.as_ref(), &settings)
    }

    pub fn update_records(&mut self) -> Result<Vec<crate::storage::UpdateRecord>> {
        let now = self.repo.now();
        let mut records = self.repo.update_records()?;
        for record in &mut records {
            record.status = updates::effective_status(record, now);
        }
        Ok(records)
    }

    pub fn export(&mut self, include_paths: bool) -> Result<serde_json::Value> {
        let snapshot = self.snapshot()?;
        let packages = self.repo.published_items_all(Some(ItemCategory::Package), 100_000)?;
        Ok(crate::export::redacted(&snapshot, &packages, include_paths))
    }

    pub fn backup(&mut self, path: &str) -> Result<String> {
        let target = PathBuf::from(path);
        if !target.is_absolute() {
            return Err(Error::invalid("Sicherungspfad muss absolut sein."));
        }
        self.repo.backup_to(&target)?;
        self.repo.log("backup", "Sicherung erstellt", "Konsistente Kopie über die SQLite-Backup-API.")?;
        Ok(target.to_string_lossy().into_owned())
    }
}

fn default_system_root(kind: ScopeKind) -> Result<String> {
    let from_env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let path = match kind {
        ScopeKind::Scoop => from_env("SCOOP").or_else(|| from_env("USERPROFILE").map(|home| format!(r"{home}\scoop"))),
        ScopeKind::Chocolatey => from_env("ChocolateyInstall").or_else(|| Some(r"C:\ProgramData\chocolatey".into())),
        _ => None,
    };
    let path = path.ok_or_else(|| Error::invalid("Standardpfad nicht ermittelbar; bitte --path angeben."))?;
    validation::windows_path(&path)
}
