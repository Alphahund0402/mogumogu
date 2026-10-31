//! A read-only view of a local Docker engine. Docker manages its own
//! storage; mogumogu lists what is there and changes nothing: no object is
//! stopped, removed or pruned, and Docker's data folders are never opened.
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

/// A Docker context as the client configures it. Only an endpoint on a
/// named pipe of this machine can be approved.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerContext {
    pub name: String,
    /// The endpoint as configured, e.g. `npipe:////./pipe/docker_engine`.
    pub host: String,
    /// The local pipe name, or `None` for a remote or unknown endpoint.
    pub pipe: Option<String>,
    pub approved: bool,
    /// Approved, but it no longer names the approved local pipe (another
    /// pipe, not local any more, or gone): never asked until approved again.
    #[serde(default)]
    pub changed: bool,
    /// Why it cannot be approved, or what changed since approval.
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerMount {
    /// `bind`, `volume`, `tmpfs` or `npipe`.
    pub kind: String,
    /// What Docker reports: a host path, a volume name or a VM path.
    pub source: String,
    pub destination: String,
    /// A bind source as a Windows path, where it can be told.
    pub host: Option<String>,
    /// The registered project the bind source lies in.
    pub project: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerContainer {
    /// First twelve hex digits of the id.
    pub id: String,
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `paused` …
    pub state: String,
    pub status: String,
    pub compose: Option<String>,
    /// The registered run this container belongs to (label or registration).
    pub run: Option<i64>,
    pub mounts: Vec<DockerMount>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerImage {
    pub id: String,
    pub tags: Vec<String>,
    /// Size as Docker reports it; layers shared with other images count in each.
    pub bytes: u64,
    /// Containers using it, when Docker counted them.
    pub containers: Option<i64>,
    pub run: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerVolume {
    pub name: String,
    pub driver: String,
    pub compose: Option<String>,
    /// From the usage query only.
    pub bytes: Option<u64>,
    pub containers: Option<i64>,
    pub run: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerNetwork {
    pub name: String,
    pub driver: String,
    /// `bridge`, `host` and `none` come with the engine.
    pub builtin: bool,
    pub run: Option<i64>,
}

/// Docker's own accounting (`/system/df`). Bytes inside the engine: on
/// Docker Desktop they live in a virtual disk, which does not shrink on the
/// host just because the engine frees space.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerUsage {
    pub layers: u64,
    pub containers: u64,
    pub volumes: u64,
    pub build_cache: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerInventory {
    pub context: String,
    /// `Docker Engine 29.4.3 · linux/x86_64 · API 1.52`
    pub engine: String,
    pub containers: Vec<DockerContainer>,
    pub images: Vec<DockerImage>,
    pub volumes: Vec<DockerVolume>,
    pub networks: Vec<DockerNetwork>,
    pub usage: Option<DockerUsage>,
    /// A list was longer than the bound and is cut.
    pub truncated: bool,
    pub checked_at: Timestamp,
}

impl DockerInventory {
    pub fn running(&self) -> usize {
        self.containers.iter().filter(|c| c.state == "running").count()
    }
}

/// The Docker part of the overview.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerOverview {
    pub contexts: Vec<DockerContext>,
}
