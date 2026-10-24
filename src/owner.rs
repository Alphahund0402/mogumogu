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
use crate::limits::EVENT_QUEUE;
use crate::service::{Core, Request};
use crate::{Error, Result};
use serde_json::Value;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender};
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
        let scan_control = core.scan_control();
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

fn run(mut core: Core, receiver: Receiver<Job>, events: impl OwnerEvents) {
    publish(&mut core, &events);
    loop {
        // Blocks without a timer: the owner only acts on requests.
        match receiver.recv() {
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
                            publish(&mut core, &events);
                            events.busy(false);
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
