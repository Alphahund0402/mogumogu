//! mogumogu core: UI-independent inventory, safety contracts and use cases.
//!
//! The desktop (`src/main.rs`) and the CLI (`src/bin/cli`) are thin shells
//! around [`service::Core`]; both call the same validated use cases.
pub mod adapters;
pub mod cleanup;
pub mod clock;
pub mod config;
pub mod domain;
pub mod error;
pub mod export;
pub mod fsread;
pub mod inventory;
pub mod ipc;
pub mod limits;
pub mod owner;
pub mod parsers;
pub mod platform;
pub mod privacy;
pub mod profiles;
pub mod review;
pub mod service;
pub mod sessions;
pub mod storage;
pub mod updates;
pub mod validation;
pub mod watcher;

pub use error::{Error, Result};
