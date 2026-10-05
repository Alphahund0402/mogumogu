//! CLI side: one connection per request, verified server identity.
use super::{Envelope, PROTOCOL_VERSION, Reply, error_from, read_frame, write_frame};
use crate::config::Config;
use crate::domain::ProcessIdentity;
use crate::limits::IPC_CLIENT_TIMEOUT;
use crate::service::Request;
use crate::sessions::{SessionControl, StartInfo};
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

    /// True if an owner process answers on the pipe.
    pub fn owner_running(&self) -> bool {
        matches!(crate::platform::pipe_connect(&self.pipe, Duration::from_millis(500)), Ok(Some(_)))
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

impl SessionControl for Client {
    fn start(&mut self, scratch_id: i64, purpose: &str) -> Result<StartInfo> {
        let value = self.call(Request::SessionStart { scratch_id, purpose: purpose.into() })?;
        Ok(serde_json::from_value(value)?)
    }
    fn attach(&mut self, session_id: i64, process: ProcessIdentity) -> Result<()> {
        self.call(Request::SessionAttach { session_id, pid: process.pid, created: process.created }).map(drop)
    }
    fn heartbeat(&mut self, session_id: i64) -> Result<()> {
        self.call(Request::SessionHeartbeat { session_id }).map(drop)
    }
    fn report(&mut self, session_id: i64, exit_code: i64, remaining: i64) -> Result<()> {
        self.call(Request::SessionReport { session_id, exit_code, remaining }).map(drop)
    }
}
