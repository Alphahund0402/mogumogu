//! Owner runtime shared by the tray app and headless mode: exactly one
//! worker thread owns [`Core`] (the only writer), the IPC server forwards
//! requests to it, and the watcher only marks scopes dirty.
//!
//! UI-independent by design; the desktop supplies an [`OwnerEvents`]
//! implementation that posts results into its event loop.
use crate::config::Config;
use crate::domain::{ScopeKind, ScopeStatus, Snapshot, TraySummary};
use crate::fsread::ScanControl;
use crate::ipc::Server;
use crate::limits::EVENT_QUEUE;
use crate::service::{Core, Request};
use crate::watcher::WatchHub;
use crate::{Error, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

/// Re-check interval for sessions waiting for process evidence.
const SESSION_RECHECK: Duration = Duration::from_secs(3);

/// Callbacks from the worker thread. Implementations must not block.
pub trait OwnerEvents: Send + 'static {
    /// Whether anyone shows the full read model right now. Without a visible
    /// dashboard the owner publishes only a [`TraySummary`] after changes.
    fn wants_snapshot(&self) -> bool {
        true
    }
    fn snapshot(&self, snapshot: Snapshot);
    fn summary(&self, _summary: TraySummary) {}
    /// Result of a request that came from the local UI.
    fn outcome(&self, request: &Request, result: &Result<Value>);
    fn busy(&self, busy: bool);
    fn show_dashboard(&self);
    /// Closes the window; the tray and the core keep running.
    fn hide_dashboard(&self);
    fn shutdown(&self);
}

enum Job {
    Request(Request, Option<SyncSender<Result<Value>>>),
    Refresh,
    Wake,
    Stop,
}

#[derive(Debug)]
pub struct Owner {
    jobs: SyncSender<Job>,
    worker: Option<JoinHandle<()>>,
    server: Option<Server>,
    scan_control: Arc<ScanControl>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Job")
    }
}

impl Owner {
    /// Opens the core, binds the pipe and starts the worker. Fails if the
    /// database is unusable or another owner already serves this data
    /// directory.
    pub fn start(config: Config, events: impl OwnerEvents) -> Result<Self> {
        let pipe = config.pipe_name()?;
        let core = Core::open(config)?;
        let (jobs, receiver) = mpsc::sync_channel::<Job>(EVENT_QUEUE);
        let forward = jobs.clone();
        let server = Server::start(&pipe, move |request| {
            let (reply, answer) = mpsc::sync_channel(1);
            forward
                .send(Job::Request(request, Some(reply)))
                .map_err(|_| Error::conflict("Besitzerprozess wird beendet."))?;
            answer
                .recv_timeout(crate::limits::IPC_CLIENT_TIMEOUT)
                .map_err(|_| Error::conflict("Zeitüberschreitung im Besitzerprozess."))?
        })
        .map_err(|e| {
            Error::conflict(format!("IPC-Kanal belegt – läuft bereits ein mogumogu-Besitzerprozess? ({e})"))
        })?;
        let wake = jobs.clone();
        let scan_control = core.scan_control();
        let worker = std::thread::Builder::new()
            .name("mogumogu-core".into())
            .spawn(move || run(core, receiver, events, wake))?;
        Ok(Self { jobs, worker: Some(worker), server: Some(server), scan_control })
    }

    /// Progress of a running pass and its cancel request, for the dashboard.
    pub fn scan_control(&self) -> Arc<ScanControl> {
        Arc::clone(&self.scan_control)
    }

    /// Queues a request from the local UI. Returns false if the worker is
    /// gone or the queue is full (the UI then shows a busy message).
    pub fn submit(&self, request: Request) -> bool {
        self.jobs.try_send(Job::Request(request, None)).is_ok()
    }

    pub fn refresh(&self) {
        let _ = self.jobs.try_send(Job::Refresh);
    }

    pub fn stop(&mut self) {
        if let Some(mut server) = self.server.take() {
            server.stop();
        }
        let _ = self.jobs.send(Job::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(mut core: Core, receiver: Receiver<Job>, events: impl OwnerEvents, wake: SyncSender<Job>) {
    let mut hub = WatchHub::default();
    configure_watchers(&mut core, &mut hub, &wake);
    publish(&mut core, &events);
    loop {
        // Block without timers unless something waits: dirty scopes,
        // sessions whose completion still needs process evidence, or the
        // end of a monitoring pause.
        let pausing = pause_end(&mut core);
        let mut wait = None;
        if hub.has_dirty() {
            wait = Some(Duration::from_millis(500));
        } else if core.has_open_sessions() {
            wait = Some(SESSION_RECHECK);
        }
        if let Some(seconds) = pausing {
            let until_end = Duration::from_secs(seconds.clamp(1, 3_600) as u64);
            wait = Some(wait.map_or(until_end, |w| w.min(until_end)));
        }
        let job = match wait {
            Some(wait) => receiver.recv_timeout(wait),
            None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match job {
            Ok(Job::Request(request, reply)) => {
                let from_ui = reply.is_none();
                match request {
                    Request::ShowDashboard => {
                        events.show_dashboard();
                        reply_ok(reply);
                    }
                    Request::HideDashboard => {
                        events.hide_dashboard();
                        reply_ok(reply);
                    }
                    Request::Shutdown => {
                        reply_ok(reply);
                        events.shutdown();
                    }
                    request => {
                        // Reads (search, filters, findings) stay quiet so typing
                        // never blocks the dashboard's buttons.
                        let write = request.is_write();
                        if write {
                            events.busy(true);
                        }
                        let result = core.dispatch(request.clone());
                        if from_ui {
                            events.outcome(&request, &result);
                        }
                        if let Some(reply) = reply {
                            let _ = reply.send(result);
                        }
                        if write {
                            configure_watchers(&mut core, &mut hub, &wake);
                            publish(&mut core, &events);
                            events.busy(false);
                        } else if core.take_changed() {
                            publish(&mut core, &events);
                        }
                    }
                }
            }
            Ok(Job::Refresh) => publish(&mut core, &events),
            Ok(Job::Wake) | Err(RecvTimeoutError::Timeout) => {
                if core.reconcile_sessions().unwrap_or(0) > 0 {
                    core.take_changed();
                    publish(&mut core, &events);
                }
                if pausing.is_some() && pause_end(&mut core).is_none() {
                    // The pause ran out: watchers resume, tray shows it.
                    configure_watchers(&mut core, &mut hub, &wake);
                    publish(&mut core, &events);
                }
                let due = hub.take_due();
                if !due.is_empty() {
                    events.busy(true);
                    for scope in due {
                        let _ = core.scan(scope);
                    }
                    publish(&mut core, &events);
                    events.busy(false);
                }
            }
            Ok(Job::Stop) | Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    hub.stop();
}

fn reply_ok(reply: Option<SyncSender<Result<Value>>>) {
    if let Some(reply) = reply {
        let _ = reply.send(Ok(Value::Bool(true)));
    }
}

fn publish(core: &mut Core, events: &impl OwnerEvents) {
    let result = if events.wants_snapshot() {
        core.snapshot().map(|snapshot| events.snapshot(snapshot))
    } else {
        core.tray_summary().map(|summary| events.summary(summary))
    };
    if let Err(e) = result {
        events.outcome(&Request::Snapshot, &Err(e));
    }
}

/// Seconds until a pending monitoring pause ends, if one is pending.
fn pause_end(core: &mut Core) -> Option<i64> {
    let now = core.repository().now();
    core.settings().ok()?.pause_pending(now).map(|until| until - now)
}

/// Watchers follow the monitoring setting, its pause and the approved project scopes.
fn configure_watchers(core: &mut Core, hub: &mut WatchHub, wake: &SyncSender<Job>) {
    let now = core.repository().now();
    let enabled = core.settings().map(|s| s.monitoring_active(now)).unwrap_or(false);
    let scopes: Vec<(i64, PathBuf)> = if enabled && !core.is_demo() {
        core.repository()
            .scopes()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.kind == ScopeKind::Project && s.status == ScopeStatus::Approved)
            .map(|s| (s.id, PathBuf::from(s.path)))
            .collect()
    } else {
        Vec::new()
    };
    if scopes.iter().map(|(id, _)| *id).collect::<Vec<_>>() == hub.watched() {
        return;
    }
    let wake = wake.clone();
    let watched = hub.configure(&scopes, move || {
        let _ = wake.try_send(Job::Wake);
    });
    core.set_watched(watched);
}
