//! Owner-side pipe server: sequential, one request per connection, bounded.
use super::{Envelope, PROTOCOL_VERSION, Reply, read_frame, write_frame};
use crate::platform::{PipeListener, pipe_connect};
use crate::service::Request;
use serde_json::Value;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// Running pipe server; stops when dropped or on [`Server::stop`].
#[derive(Debug)]
pub struct Server {
    name: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Binds the pipe (fails if another owner already holds it) and serves
    /// requests on a background thread through `handler`.
    pub fn start<H>(name: &str, handler: H) -> io::Result<Self>
    where
        H: FnMut(Request) -> crate::Result<Value> + Send + 'static,
    {
        let listener = PipeListener::bind(name)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread =
            std::thread::Builder::new().name("mogumogu-ipc".into()).spawn(move || serve(&listener, handler, &flag))?;
        Ok(Self { name: name.to_string(), stop, thread: Some(thread) })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept with a throw-away connection.
        let _ = pipe_connect(&self.name, Duration::from_millis(200));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn serve<H>(listener: &PipeListener, mut handler: H, stop: &AtomicBool)
where
    H: FnMut(Request) -> crate::Result<Value>,
{
    while !stop.load(Ordering::SeqCst) {
        let mut connection = match listener.accept() {
            Ok(connection) => connection,
            Err(_) => {
                // Foreign session or transient error: never crash the owner.
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(payload) = read_frame(&mut connection) else { continue };
        let reply = match serde_json::from_slice::<Envelope>(&payload) {
            Err(e) => Reply::Error {
                v: PROTOCOL_VERSION,
                id: 0,
                code: "invalid".into(),
                message: format!("Ungültige Anfrage: {e}"),
            },
            Ok(envelope) if envelope.v != PROTOCOL_VERSION => Reply::Error {
                v: PROTOCOL_VERSION,
                id: envelope.id,
                code: "unsupported".into(),
                message: format!(
                    "Protokollversion {} wird nicht unterstützt (erwartet {PROTOCOL_VERSION}).",
                    envelope.v
                ),
            },
            Ok(envelope) => match handler(envelope.request) {
                Ok(result) => Reply::Ok { v: PROTOCOL_VERSION, id: envelope.id, result },
                Err(e) => Reply::Error {
                    v: PROTOCOL_VERSION,
                    id: envelope.id,
                    code: e.code().into(),
                    message: crate::privacy::redact(&e.to_string()),
                },
            },
        };
        let mut bytes = serde_json::to_vec(&reply).unwrap_or_default();
        if bytes.len() > crate::limits::IPC_MAX_MESSAGE {
            let id = match &reply {
                Reply::Ok { id, .. } | Reply::Error { id, .. } => *id,
            };
            bytes = serde_json::to_vec(&Reply::Error {
                v: PROTOCOL_VERSION,
                id,
                code: "invalid".into(),
                message: "Antwort überschreitet das IPC-Limit; bitte einschränken.".into(),
            })
            .unwrap_or_default();
        }
        let _ = write_frame(&mut connection, &bytes);
    }
}
