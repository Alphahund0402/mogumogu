//! Local IPC between CLI and owner process (F-13; PROJEKTPLAN §5.3).
//!
//! Transport: a named pipe restricted to the current user, local clients
//! only, first instance only (see `platform`). Framing: 4-byte little-endian
//! length + JSON, at most [`IPC_MAX_MESSAGE`] bytes. Every message carries
//! the protocol version and a request id. One request per connection.
mod client;
mod server;

pub use client::Client;
pub use server::{Server, serve};

use crate::limits::IPC_MAX_MESSAGE;
use crate::service::Request;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Write};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub v: u32,
    pub id: u64,
    pub request: Request,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Reply {
    Ok { v: u32, id: u64, result: Value },
    Error { v: u32, id: u64, code: String, message: String },
}

pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.len() > IPC_MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Nachricht über dem IPC-Limit"));
    }
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

pub fn read_frame(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > IPC_MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Nachricht über dem IPC-Limit"));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

/// Maps a reply error code back to the core error class.
pub(crate) fn error_from(code: &str, message: String) -> crate::Error {
    use crate::Error;
    match code {
        "invalid" | "json" => Error::Invalid(message),
        "blocked" => Error::Blocked(message),
        "not_found" => Error::NotFound(message),
        "conflict" => Error::Conflict(message),
        "unsupported" => Error::Unsupported(message),
        _ => Error::Io(io::Error::other(message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_roundtrip_and_oversize_is_rejected() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, b"{}").unwrap();
        assert_eq!(read_frame(&mut buffer.as_slice()).unwrap(), b"{}");
        let mut forged = (u32::MAX).to_le_bytes().to_vec();
        forged.extend_from_slice(b"x");
        assert!(read_frame(&mut forged.as_slice()).is_err());
        assert!(write_frame(&mut Vec::new(), &vec![0; IPC_MAX_MESSAGE + 1]).is_err());
    }

    #[test]
    fn envelopes_reject_unknown_fields() {
        let ok = r#"{"v":1,"id":7,"request":{"cmd":"status"}}"#;
        assert!(serde_json::from_str::<Envelope>(ok).is_ok());
        let extra = r#"{"v":1,"id":7,"request":{"cmd":"status"},"elevate":true}"#;
        assert!(serde_json::from_str::<Envelope>(extra).is_err());
    }
}
