//! One error type for the core. Messages are German because they reach the
//! dashboard and CLI directly; `code()` gives a stable machine-readable class.
use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    /// Input or contract violation the caller can correct.
    Invalid(String),
    /// A safety rule prevents the operation. Never retried automatically.
    Blocked(String),
    NotFound(String),
    /// State-machine or concurrency conflict (e.g. plan no longer approved).
    Conflict(String),
    /// Capability not implemented or not available on this platform.
    Unsupported(String),
}

impl Error {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
    pub fn blocked(message: impl Into<String>) -> Self {
        Self::Blocked(message.into())
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    /// Stable error class for JSON/IPC consumers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Sql(_) => "database",
            Self::Json(_) => "json",
            Self::Invalid(_) => "invalid",
            Self::Blocked(_) => "blocked",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Unsupported(_) => "unsupported",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Dateizugriff fehlgeschlagen: {e}"),
            Self::Sql(e) => write!(f, "Datenbankoperation fehlgeschlagen: {e}"),
            Self::Json(e) => write!(f, "JSON konnte nicht verarbeitet werden: {e}"),
            Self::Invalid(m) | Self::Blocked(m) | Self::NotFound(m) | Self::Conflict(m) | Self::Unsupported(m) => {
                f.write_str(m)
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Sql(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
