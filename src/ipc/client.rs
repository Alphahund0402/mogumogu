//! CLI side: one connection per request, verified server identity.
use super::{Envelope, PROTOCOL_VERSION, Reply, error_from, read_frame, write_frame};
use crate::config::Config;
use crate::limits::IPC_CLIENT_TIMEOUT;
use crate::service::Request;
use crate::{Error, Result};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct Client {
    pipe: String,
    timeout: Duration,
}

impl Client {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self { pipe: config.pipe_name()?, timeout: IPC_CLIENT_TIMEOUT })
    }

    /// True if an owner process holds the pipe. A busy pipe counts: the
    /// owner is serving another request, and starting a second owner would
    /// only fail. Its identity is still verified when a call connects.
    pub fn owner_running(&self) -> bool {
        match crate::platform::pipe_connect(&self.pipe, Duration::from_millis(500)) {
            Ok(found) => found.is_some(),
            Err(error) => crate::platform::pipe_busy(&error),
        }
    }

    pub fn call(&self, request: Request) -> Result<Value> {
        let mut pipe = crate::platform::pipe_connect(&self.pipe, self.timeout)?
            .ok_or_else(|| Error::unsupported("Kein mogumogu-Besitzerprozess läuft für dieses Datenverzeichnis."))?;
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let envelope = Envelope { v: PROTOCOL_VERSION, id, request };
        write_frame(&mut pipe, &serde_json::to_vec(&envelope)?)?;
        let reply: Reply = serde_json::from_slice(&read_frame(&mut pipe)?)?;
        match reply {
            Reply::Ok { id: reply_id, result, .. } if reply_id == id => Ok(result),
            Reply::Error { code, message, .. } => Err(error_from(&code, message)),
            Reply::Ok { .. } => Err(Error::conflict("Antwort gehört zu einer anderen Anfrage.")),
        }
    }
}
