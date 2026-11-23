//! Owner runtime shared by the tray app and headless mode: exactly one
//! worker thread owns [`Core`] (the only writer), the IPC server forwards
//! requests to it.
//!
//! UI-independent by design; the desktop supplies an [`OwnerEvents`]
//! implementation that posts results into its event loop.
use crate::config::Config;
use crate::domain::{Snapshot, TraySummary};
use crate::fsread::ScanControl;
use crate::ipc::Server;
use crate::limits::{DISK_IDLE, EVENT_QUEUE};
use crate::service::{Core, Request};
use crate::{Error, Result};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, RecvError, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

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
        Self::start_with(Core::open(config)?, events)
    }

    /// Serves an already opened core, e.g. one whose recycle bin a test
    /// replaced.
    pub fn start_with(core: Core, events: impl OwnerEvents) -> Result<Self> {
        let pipe = core.config().pipe_name()?;
        let (jobs, receiver) = mpsc::sync_channel::<Job>(EVENT_QUEUE);
        let forward = jobs.clone();
        let scan_control = core.scan_control();
        let events = Arc::new(Mutex::new(events));
        let ipc_events = Arc::clone(&events);
        let ipc_control = Arc::clone(&scan_control);
        let server = Server::start(&pipe, move |request| {
            // Host-only commands do not touch Core or SQLite; handle them
            // without waiting behind a long scan or disk analysis.
            match request {
                Request::ShowDashboard => {
                    with_events(&ipc_events, |e| e.show_dashboard());
                    return Ok(Value::Bool(true));
                }
                Request::HideDashboard => {
                    with_events(&ipc_events, |e| e.hide_dashboard());
                    return Ok(Value::Bool(true));
                }
                Request::Shutdown => {
                    ipc_control.cancel();
                    with_events(&ipc_events, |e| e.shutdown());
                    return Ok(Value::Bool(true));
                }
                _ => {}
            }
            let (reply, answer) = mpsc::sync_channel(1);
            let wait = request.reply_within();
            forward
                .send(Job::Request(request, Some(reply)))
                .map_err(|_| Error::conflict("Besitzerprozess wird beendet."))?;
            answer.recv_timeout(wait).map_err(|_| Error::conflict("Zeitüberschreitung im Besitzerprozess."))?
        })
        .map_err(|e| {
            Error::conflict(format!("IPC-Kanal belegt – läuft bereits ein mogumogu-Besitzerprozess? ({e})"))
        })?;
        let worker =
            std::thread::Builder::new().name("mogumogu-core".into()).spawn(move || run(core, receiver, events))?;
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
        // Cancel any running scan/analysis first so neither IPC nor worker
        // waits out a long pass during shutdown.
        self.scan_control.cancel();
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

fn with_events<E: OwnerEvents, R>(events: &Mutex<E>, f: impl FnOnce(&E) -> R) -> R {
    let guard = events.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&*guard)
}

/// Coalesces redundant UI refresh/read jobs sitting behind the same state,
/// without ever dropping writes, IPC replies or reordering across writes.
fn push_coalesced(pending: &mut VecDeque<Job>, job: Job) {
    match &job {
        Job::Refresh => {
            if let Some(pos) = pending.iter().rposition(|j| matches!(j, Job::Refresh)) {
                pending.remove(pos);
            }
        }
        Job::Request(req, None) => {
            let tag = match req {
                Request::Search { .. } => Some(0_u8),
                Request::Packages { .. } => Some(1_u8),
                Request::StoragePage { .. } => Some(2_u8),
                _ => None,
            };
            if let Some(tag) = tag {
                // Only supersede an older UI read after the last queued write/stop.
                let barrier = pending
                    .iter()
                    .rposition(|j| matches!(j, Job::Stop) || matches!(j, Job::Request(r, _) if r.is_write()))
                    .map_or(0, |i| i + 1);
                for index in (barrier..pending.len()).rev() {
                    let same = matches!(
                        &pending[index],
                        Job::Request(Request::Search { .. }, None) if tag == 0
                    ) || matches!(
                        &pending[index],
                        Job::Request(Request::Packages { .. }, None) if tag == 1
                    ) || matches!(
                        &pending[index],
                        Job::Request(Request::StoragePage { .. }, None) if tag == 2
                    );
                    if same {
                        pending.remove(index);
                    }
                }
            }
        }
        _ => {}
    }
    pending.push_back(job);
}

fn run(mut core: Core, receiver: Receiver<Job>, events: Arc<Mutex<impl OwnerEvents>>) {
    publish(&mut core, &events);
    let mut pending = VecDeque::new();
    loop {
        // Drain already-queued jobs so rapid UI typing/refreshes coalesce.
        loop {
            match receiver.try_recv() {
                Ok(job) => push_coalesced(&mut pending, job),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if pending.is_empty() {
                        return;
                    }
                    break;
                }
            }
        }
        let job = if let Some(next) = pending.pop_front() {
            Ok(next)
        } else if core.holds_storage_analysis() {
            // Blocks without a timer: the owner only acts on requests. A held disk
            // analysis is the exception: without a dashboard, an unused one is
            // released instead of staying in the tray.
            match receiver.recv_timeout(DISK_IDLE) {
                Err(RecvTimeoutError::Timeout) => {
                    if !with_events(&events, OwnerEvents::wants_snapshot) {
                        core.clear_storage_analysis();
                    }
                    continue;
                }
                job => job.map_err(|_| RecvError),
            }
        } else {
            receiver.recv()
        };
        match job {
            Ok(Job::Request(request, reply)) => {
                let from_ui = reply.is_none();
                match request {
                    Request::ShowDashboard => {
                        with_events(&events, OwnerEvents::show_dashboard);
                        reply_ok(reply);
                    }
                    Request::HideDashboard => {
                        with_events(&events, OwnerEvents::hide_dashboard);
                        reply_ok(reply);
                    }
                    Request::Shutdown => {
                        core.scan_control().cancel();
                        reply_ok(reply);
                        with_events(&events, OwnerEvents::shutdown);
                    }
                    // Accepting runs is the person's decision in the dashboard; an
                    // agent on the pipe could otherwise switch it on for itself.
                    Request::RunsAllow { allowed: true } if !from_ui => {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(crate::Error::blocked(
                                "KI-Läufe lassen sich nur im Dashboard einschalten (Speicher → Aufräumen → Läufe & Docker …).",
                            )));
                        }
                    }
                    // Ending a hold is the person's decision in the dashboard too:
                    // releasing a run, ending a monitoring, giving up a kept folder,
                    // or revoking a Docker context (its binds would stop holding).
                    Request::RunRelease { .. }
                    | Request::DbUnregister { .. }
                    | Request::KeepFolder { keep: false, .. }
                    | Request::DockerRevoke { .. }
                        if !from_ui =>
                    {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(crate::Error::blocked(
                                "Schutz aufheben geht nur im Dashboard (Speicher → Aufräumen).",
                            )));
                        }
                    }
                    // Moving folders is the person's decision: over the pipe a plan
                    // can be made, cancelled and reconciled, but never bound to its
                    // fingerprint or run, even with the right one.
                    Request::CleanupApprove { .. } | Request::CleanupApply { .. } if !from_ui => {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(crate::Error::blocked(
                                "Einen Papierkorb-Plan bestätigen und ausführen geht nur im Dashboard (Übersicht → Bereinigungspläne).",
                            )));
                        }
                    }
                    // Finishing a project is the person's decision (docs/LEBENSZYKLUS.md,
                    // E2): an agent may read the report and reactivate, never finish.
                    Request::FinishProject { .. } if !from_ui => {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(crate::Error::blocked(
                                "Ein Projekt abschließen geht nur im Dashboard (Projektseite → Abschlussbericht …).",
                            )));
                        }
                    }
                    request => {
                        // Reads (search, filters, findings) stay quiet so typing
                        // never blocks the dashboard's buttons.
                        let write = request.is_write();
                        if write {
                            with_events(&events, |e| e.busy(true));
                        }
                        let result = core.dispatch(request.clone());
                        if from_ui {
                            with_events(&events, |e| e.outcome(&request, &result));
                        }
                        if let Some(reply) = reply {
                            let _ = reply.send(result);
                        }
                        if write {
                            publish(&mut core, &events);
                            with_events(&events, |e| e.busy(false));
                        } else if core.take_changed() {
                            publish(&mut core, &events);
                        }
                    }
                }
            }
            Ok(Job::Refresh) => publish(&mut core, &events),
            Ok(Job::Stop) | Err(_) => break,
        }
    }
}

fn reply_ok(reply: Option<SyncSender<Result<Value>>>) {
    if let Some(reply) = reply {
        let _ = reply.send(Ok(Value::Bool(true)));
    }
}

fn publish(core: &mut Core, events: &Mutex<impl OwnerEvents>) {
    let wants = with_events(events, OwnerEvents::wants_snapshot);
    let result = if wants {
        core.snapshot().map(|snapshot| with_events(events, |e| e.snapshot(snapshot)))
    } else {
        core.tray_summary().map(|summary| with_events(events, |e| e.summary(summary)))
    };
    if let Err(e) = result {
        with_events(events, |ev| ev.outcome(&Request::Snapshot, &Err(e)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_reads_and_refreshes_coalesce_without_crossing_writes_or_ipc() {
        let mut q = VecDeque::new();
        push_coalesced(&mut q, Job::Request(Request::Search { query: "a".into() }, None));
        push_coalesced(&mut q, Job::Request(Request::Search { query: "ab".into() }, None));
        push_coalesced(&mut q, Job::Refresh);
        push_coalesced(&mut q, Job::Refresh);
        assert_eq!(q.len(), 2);
        assert!(matches!(&q[0], Job::Request(Request::Search { query }, None) if query == "ab"));
        assert!(matches!(&q[1], Job::Refresh));

        // A queued write acts as a barrier: reads before it stay in place.
        let (tx, _rx) = mpsc::sync_channel(1);
        push_coalesced(&mut q, Job::Request(Request::Scan { scope_id: None }, None));
        push_coalesced(&mut q, Job::Request(Request::Search { query: "ipc".into() }, Some(tx)));
        push_coalesced(&mut q, Job::Request(Request::Search { query: "abc".into() }, None));
        assert_eq!(q.len(), 5, "older read before write and IPC replies are never dropped");
    }
}
