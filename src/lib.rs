//! mogumogu core: UI-independent inventory, safety contracts and use cases.
//!
//! The desktop (`src/main.rs`) and the CLI (`src/bin/cli`) are thin shells
//! around [`service::Core`]; both call the same validated use cases.
//!
//! Start at [`owner::Owner`]: it serializes [`service::Request`] values onto
//! the one thread holding the core. [`domain::Snapshot`] is the bounded read
//! model returned to presentation; [`service::Status`] is a separate summary.
//!
//! - **Rules and records:** [`domain`], [`review`], [`validation`], [`privacy`].
//! - **Use cases and coordination:** [`service`], [`owner`], [`ipc`].
//! - **Static inventory:** [`inventory`] reads through [`fsread`], then hands
//!   bounded data to [`adapters`] and [`profiles`], using [`parsers`].
//! - **Effects:** [`storage`] owns SQL, [`platform`] owns Windows FFI, and
//!   [`cleanup`] owns journaled moves to the recycle bin (production disabled).
//!
//! See `docs/ARCHITEKTUR.md` for process/data-flow diagrams and a reading path;
//! `docs/ENTWICKLUNG.md` maps common changes to their implementation and checks.
pub mod adapters;
pub mod cleanup;
pub mod clock;
pub mod config;
pub mod domain;
pub mod error;
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
pub mod storage;
pub mod validation;

pub use error::{Error, Result};
