//! Read-only inventory of local Docker engines. A context is approved on its
//! own; its endpoint must be a named pipe of this machine. The client config
//! is read only as far as needed: the `meta.json` of each context, never
//! `config.json` (it can hold registry credentials) and never the TLS keys.
//! The engine is asked through [`http::get`] only, which knows no method but
//! `GET` and no path outside its fixed list. Nothing is started, stopped,
//! removed or pruned, and Docker's data folders are never opened.
pub mod http;

use crate::clock::Timestamp;
use crate::domain::*;
use crate::fsread::{self, Budget, ScopeRoot};
use crate::limits::ScanLimits;
use http::Query;
use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Each question to the engine; `/system/df` gets longer.
const ENGINE_TIMEOUT: Duration = Duration::from_secs(10);
const USAGE_TIMEOUT: Duration = Duration::from_secs(60);
/// Objects listed per kind; the inventory says when a list is cut. The
/// answer to the dashboard stays below the IPC limit.
const MAX_OBJECTS: usize = 500;
/// Containers and volumes checked for bind mounts; more is unknown, not cut.
const MAX_HOLD_OBJECTS: usize = 5_000;
const META_MAX: u64 = 64 << 10;
/// The label a cooperating harness can set on objects it creates.
pub const RUN_LABEL: &str = "io.mogumogu.run";
const COMPOSE_LABEL: &str = "com.docker.compose.project";

/// The local pipe name of an endpoint such as `npipe:////./pipe/docker_engine`;
/// `None` for TCP, SSH, sockets and pipes of other machines.
pub fn local_pipe(host: &str) -> Option<String> {
    let rest = host.strip_prefix("npipe:")?.replace('\\', "/");
    let rest = rest.trim_start_matches('/');
    let (machine, rest) = rest.split_once('/')?;
    let name = rest.strip_prefix("pipe/").or_else(|| rest.strip_prefix("PIPE/"))?;
    let plain = |c: char| c.is_ascii_alphanumeric() || "_.-".contains(c);
    let named = name.chars().any(|c| c.is_ascii_alphanumeric());
    (machine == "." && named && name.len() <= 128 && name.chars().all(plain)).then(|| name.to_string())
}

/// The contexts the Docker client knows: `default` (from `DOCKER_HOST` or the
/// engine's standard pipe) and one per `contexts/meta/*/meta.json` in the
/// client's config folder (`DOCKER_CONFIG` or `%USERPROFILE%\.docker`).
pub fn contexts(env: &dyn Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    let host = env("DOCKER_HOST").filter(|h| !h.is_empty()).unwrap_or_else(|| "npipe:////./pipe/docker_engine".into());
    let mut found = vec![("default".to_string(), host)];
    let folder = env("DOCKER_CONFIG")
        .map(PathBuf::from)
        .or_else(|| env("USERPROFILE").map(|home| Path::new(&home).join(".docker")));
    if let Some(folder) = folder {
        found.extend(meta_contexts(&folder.join("contexts").join("meta")));
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.dedup_by(|a, b| a.0 == b.0);
    found
}

/// Name and endpoint from each context's metadata, read without links.
fn meta_contexts(meta: &Path) -> Vec<(String, String)> {
    let Ok(root) = ScopeRoot::open_local(meta, None) else { return Vec::new() };
    let limits = ScanLimits { max_depth: 1, max_entries: 1_000, max_total_bytes: 4 << 20, time_budget: ENGINE_TIMEOUT };
    let mut budget = Budget::new(limits);
    let Ok((entries, _)) = fsread::list(root.handle(), &mut budget) else { return Vec::new() };
    let mut found = Vec::new();
    for entry in entries.iter().filter(|e| e.is_dir() && !e.is_reparse()).take(100) {
        let Ok(folder) = root.handle().open_dir(&entry.name) else { continue };
        let Ok(text) = fsread::read_text(&folder, OsStr::new("meta.json"), META_MAX, &mut budget) else { continue };
        let Ok(meta) = serde_json::from_str::<Value>(&text) else { continue };
        let name = meta["Name"].as_str().unwrap_or_default();
        let host = meta["Endpoints"]["docker"]["Host"].as_str().unwrap_or_default();
        if !name.is_empty() && name.chars().count() <= 100 && !name.chars().any(char::is_control) {
            found.push((name.to_string(), host.chars().take(300).collect()));
        }
    }
    found
}

/// Whether the approval of `name` for `pipe` still holds: the context must
/// still name exactly that local pipe. Otherwise it is not asked.
pub fn still_approved(contexts: &[(String, String)], name: &str, pipe: &str) -> Result<(), String> {
    match contexts.iter().find(|(n, _)| n == name).map(|(_, host)| local_pipe(host)) {
        Some(Some(current)) if current == pipe => Ok(()),
        Some(Some(_)) => {
            Err(format!("Docker-Kontext „{name}“ zeigt seit der Freigabe auf eine andere Pipe; erneut freigeben"))
        }
        Some(None) => Err(format!("Docker-Kontext „{name}“ ist nicht mehr lokal; erneut freigeben")),
        None => Err(format!("Docker-Kontext „{name}“ ist nicht mehr konfiguriert; erneut freigeben")),
    }
}

/// Registered things the inventory links objects to.
#[derive(Debug, Default)]
pub struct Links {
    /// Registered projects: name and root.
    pub projects: Vec<(String, String)>,
    /// Docker objects registered by runs: kind, id or name, run.
    pub artifacts: Vec<(DockerObject, String, i64)>,
}

impl Links {
    fn run(&self, kind: DockerObject, ids: &[&str], labels: &Value) -> Option<i64> {
        let labelled = labels.get(RUN_LABEL).and_then(Value::as_str).and_then(|v| v.trim().parse().ok());
        labelled.or_else(|| {
            self.artifacts.iter().find_map(|(k, id, run)| {
                let id = id.trim_start_matches("sha256:");
                let matches = |own: &&str| {
                    let own = own.trim_start_matches("sha256:").trim_start_matches('/');
                    own == id || (id.len() >= 12 && own.starts_with(id))
                };
                (*k == kind && ids.iter().any(matches)).then_some(*run)
            })
        })
    }

    fn project(&self, host: &str) -> Option<String> {
        self.projects.iter().find(|(_, root)| starts_within(host, root)).map(|(name, _)| name.clone())
    }
}

/// `path` lies at or below `root`.
fn starts_within(path: &str, root: &str) -> bool {
    let (path, root) = (fold_case(path), fold_case(root.trim_end_matches('\\')));
    path == root || path.starts_with(&format!("{root}\\"))
}

/// A bind source as a Windows path. Docker Desktop names host folders inside
/// its VM (`/run/desktop/mnt/host/c/…`, `/mnt/host/c/…`, older
/// `/host_mnt/c/…`), an engine in WSL as `/mnt/c/…` or, with
/// `automount root=/`, `/c/…`; Windows engines keep `C:\…`. A one-letter
/// top folder is read as a drive: that may hold too much, never too little.
pub fn host_path(source: &str) -> Option<String> {
    let source = source.strip_prefix(r"\\?\").unwrap_or(source);
    let drive = |rest: &str| {
        let mut parts = rest.splitn(2, '/');
        let letter = parts.next().filter(|l| l.len() == 1 && l.as_bytes()[0].is_ascii_alphabetic())?;
        let tail = parts.next().unwrap_or_default().replace('/', "\\");
        Some(format!("{}:\\{}", letter.to_ascii_uppercase(), tail).trim_end_matches('\\').to_string())
    };
    for prefix in ["/run/desktop/mnt/host/", "/mnt/host/", "/host_mnt/", "/mnt/", "/"] {
        if let Some(rest) = source.strip_prefix(prefix)
            && let Some(path) = drive(rest)
        {
            return Some(path);
        }
    }
    let bytes = source.as_bytes();
    (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'))
        .then(|| format!("{}{}", source[..1].to_ascii_uppercase(), source[1..].replace('/', "\\")))
}

/// The engine of an approved context: what it runs and stores.
pub fn inventory(
    context: &str,
    pipe: &str,
    usage: bool,
    links: &Links,
    now: Timestamp,
) -> Result<DockerInventory, String> {
    let version = http::get(pipe, Query::Version, ENGINE_TIMEOUT)?;
    let text = |v: &Value, key: &str| v[key].as_str().unwrap_or_default().to_string();
    let engine = format!(
        "{} {} · {}/{} · API {}",
        version["Platform"]["Name"].as_str().unwrap_or("Docker Engine"),
        text(&version, "Version"),
        text(&version, "Os"),
        text(&version, "Arch"),
        text(&version, "ApiVersion")
    );
    let mut truncated = false;
    let mut list = |value: Value| -> Vec<Value> {
        let items = match value {
            Value::Array(items) => items,
            _ => Vec::new(),
        };
        truncated |= items.len() > MAX_OBJECTS;
        items.into_iter().take(MAX_OBJECTS).collect()
    };
    let containers =
        list(http::get(pipe, Query::Containers, ENGINE_TIMEOUT)?).iter().map(|c| container(c, links)).collect();
    let images = list(http::get(pipe, Query::Images, ENGINE_TIMEOUT)?).iter().map(|i| image(i, links)).collect();
    let volumes =
        list(http::get(pipe, Query::Volumes, ENGINE_TIMEOUT)?.get_mut("Volumes").map_or(Value::Null, Value::take));
    let networks = list(http::get(pipe, Query::Networks, ENGINE_TIMEOUT)?).iter().map(|n| network(n, links)).collect();
    let mut inventory = DockerInventory {
        context: context.to_string(),
        engine,
        containers,
        images,
        volumes: volumes.iter().map(|v| volume(v, links)).collect(),
        networks,
        usage: None,
        truncated,
        checked_at: now,
    };
    if usage {
        let df = http::get(pipe, Query::DiskUsage, USAGE_TIMEOUT)?;
        apply_usage(&mut inventory, &df);
    }
    Ok(inventory)
}

fn short_id(id: &str) -> String {
    id.trim_start_matches("sha256:").chars().take(12).collect()
}

fn bytes(value: &Value) -> u64 {
    value.as_i64().filter(|b| *b >= 0).map_or(0, |b| b as u64)
}

fn label(labels: &Value, key: &str) -> Option<String> {
    labels.get(key).and_then(Value::as_str).map(|v| v.chars().take(100).collect())
}

fn container(c: &Value, links: &Links) -> DockerContainer {
    let id = c["Id"].as_str().unwrap_or_default();
    let name = c["Names"][0].as_str().unwrap_or_default().trim_start_matches('/').to_string();
    let mounts = c["Mounts"]
        .as_array()
        .map(|mounts| {
            mounts
                .iter()
                .take(100)
                .map(|m| {
                    let kind = m["Type"].as_str().unwrap_or_default().to_string();
                    let source = if kind == "volume" { m["Name"].as_str() } else { m["Source"].as_str() };
                    let source = source.unwrap_or_default().to_string();
                    let host = (kind == "bind").then(|| host_path(&source)).flatten();
                    DockerMount {
                        project: host.as_deref().and_then(|h| links.project(h)),
                        kind,
                        source,
                        destination: m["Destination"].as_str().unwrap_or_default().to_string(),
                        host,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    DockerContainer {
        id: short_id(id),
        image: c["Image"].as_str().unwrap_or_default().chars().take(200).collect(),
        state: c["State"].as_str().unwrap_or_default().to_string(),
        status: c["Status"].as_str().unwrap_or_default().to_string(),
        compose: label(&c["Labels"], COMPOSE_LABEL),
        run: links.run(DockerObject::Container, &[id, &name], &c["Labels"]),
        name,
        mounts,
    }
}

fn image(i: &Value, links: &Links) -> DockerImage {
    let id = i["Id"].as_str().unwrap_or_default();
    let tags: Vec<String> = i["RepoTags"]
        .as_array()
        .map(|t| {
            t.iter().filter_map(Value::as_str).filter(|t| *t != "<none>:<none>").take(10).map(String::from).collect()
        })
        .unwrap_or_default();
    let names: Vec<&str> = std::iter::once(id).chain(tags.iter().map(String::as_str)).collect();
    DockerImage {
        id: short_id(id),
        bytes: bytes(&i["Size"]),
        containers: i["Containers"].as_i64().filter(|n| *n >= 0),
        run: links.run(DockerObject::Image, &names, &i["Labels"]),
        tags,
    }
}

fn volume(v: &Value, links: &Links) -> DockerVolume {
    let name = v["Name"].as_str().unwrap_or_default().to_string();
    DockerVolume {
        driver: v["Driver"].as_str().unwrap_or_default().to_string(),
        compose: label(&v["Labels"], COMPOSE_LABEL),
        bytes: v["UsageData"]["Size"].as_i64().filter(|b| *b >= 0).map(|b| b as u64),
        containers: v["UsageData"]["RefCount"].as_i64().filter(|n| *n >= 0),
        run: links.run(DockerObject::Volume, &[&name], &v["Labels"]),
        name,
    }
}

fn network(n: &Value, links: &Links) -> DockerNetwork {
    let name = n["Name"].as_str().unwrap_or_default().to_string();
    let id = n["Id"].as_str().unwrap_or_default();
    DockerNetwork {
        driver: n["Driver"].as_str().unwrap_or_default().to_string(),
        builtin: matches!(name.as_str(), "bridge" | "host" | "none" | "nat"),
        run: links.run(DockerObject::Network, &[id, &name], &n["Labels"]),
        name,
    }
}

/// Docker's accounting, and the sizes of volumes, from `/system/df`.
fn apply_usage(inventory: &mut DockerInventory, df: &Value) {
    let sum = |key: &str, field: &dyn Fn(&Value) -> u64| {
        df[key].as_array().map_or(0, |a| a.iter().map(field).fold(0, u64::saturating_add))
    };
    inventory.usage = Some(DockerUsage {
        layers: bytes(&df["LayersSize"]),
        containers: sum("Containers", &|c| bytes(&c["SizeRw"])),
        volumes: sum("Volumes", &|v| bytes(&v["UsageData"]["Size"])),
        build_cache: sum("BuildCache", &|b| bytes(&b["Size"])),
    });
    for listed in df["Volumes"].as_array().into_iter().flatten() {
        let name = listed["Name"].as_str().unwrap_or_default();
        if let Some(volume) = inventory.volumes.iter_mut().find(|v| v.name == name) {
            volume.bytes = listed["UsageData"]["Size"].as_i64().filter(|b| *b >= 0).map(|b| b as u64);
            volume.containers = listed["UsageData"]["RefCount"].as_i64().filter(|n| *n >= 0);
        }
    }
}

/// Names of the volumes of an approved engine (for suggestions and to check
/// that a monitored volume exists).
pub fn volume_names(pipe: &str) -> Result<Vec<String>, String> {
    let volumes = http::get(pipe, Query::Volumes, ENGINE_TIMEOUT)?;
    Ok(volumes["Volumes"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_HOLD_OBJECTS)
        .filter_map(|v| v["Name"].as_str().map(String::from))
        .collect())
}

/// Size of one volume from Docker's own accounting (`/system/df`); `None`
/// when Docker did not count it.
pub fn volume_size(pipe: &str, volume: &str) -> Result<Option<u64>, String> {
    let df = http::get(pipe, Query::DiskUsage, USAGE_TIMEOUT)?;
    Ok(df["Volumes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["Name"].as_str() == Some(volume))
        .and_then(|v| v["UsageData"]["Size"].as_i64())
        .filter(|b| *b >= 0)
        .map(|b| b as u64))
}

/// Folders the approved engine binds in: through containers, running or
/// not (a stopped stack starts again), and through volumes whose driver
/// binds a host folder. Each counts under the spelling Docker reports and,
/// where it exists, under its real place. An engine that cannot be asked, a
/// list longer than the bound or an answer of an unexpected shape leaves
/// this open: the caller must treat the error as a blocker.
pub fn mount_holds(context: &str, pipe: &str) -> Result<Vec<Hold>, String> {
    let unknown = |what: &str| format!("Docker-Kontext „{context}“: {what}");
    let containers = http::get(pipe, Query::Containers, ENGINE_TIMEOUT).map_err(|e| unknown(&e))?;
    let containers = containers.as_array().ok_or_else(|| unknown("unerwartete Antwort auf die Containerliste"))?;
    let volumes = http::get(pipe, Query::Volumes, ENGINE_TIMEOUT).map_err(|e| unknown(&e))?;
    // Older engines send `null` for none; a missing field is unknown.
    let volumes = match volumes.get("Volumes") {
        Some(Value::Array(volumes)) => volumes.as_slice(),
        Some(Value::Null) => &[],
        _ => return Err(unknown("unerwartete Antwort auf die Volume-Liste")),
    };
    if containers.len() > MAX_HOLD_OBJECTS || volumes.len() > MAX_HOLD_OBJECTS {
        return Err(unknown(&format!(
            "mehr als {MAX_HOLD_OBJECTS} Container oder Volumes; Einbindungen nicht vollständig prüfbar"
        )));
    }
    let mut holds = Vec::new();
    let mut hold = |source: &str, reason: String| {
        let Some(host) = host_path(source) else { return };
        let real = crate::platform::real_place(Path::new(&host)).filter(|real| !same_location(real, &host));
        for location in real.into_iter().chain([host]) {
            holds.push(Hold { kind: HoldKind::Docker, location, reason: reason.clone() });
        }
    };
    for c in containers {
        let name = c["Names"][0].as_str().unwrap_or_default().trim_start_matches('/');
        let state = c["State"].as_str().unwrap_or_default();
        let mounts = match c.get("Mounts") {
            Some(Value::Array(mounts)) => mounts.as_slice(),
            Some(Value::Null) => &[],
            _ => return Err(unknown(&format!("Container „{name}“ ohne Mount-Liste"))),
        };
        for m in mounts.iter().filter(|m| m["Type"] == "bind") {
            hold(m["Source"].as_str().unwrap_or_default(), format!("Container „{name}“ ({state}) bindet ihn ein"));
        }
    }
    // Any device that names a host folder counts, whatever `o` says (`bind`,
    // `rbind` …): holding too much is safe, too little is not.
    for v in volumes {
        if let Some(device) = v["Options"]["device"].as_str() {
            let name = v["Name"].as_str().unwrap_or_default();
            hold(device, format!("Volume „{name}“ bindet ihn ein"));
        }
    }
    Ok(holds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_pipes_of_this_machine_are_local() {
        assert_eq!(local_pipe("npipe:////./pipe/docker_engine").as_deref(), Some("docker_engine"));
        assert_eq!(local_pipe("npipe://./pipe/dockerDesktopLinuxEngine").as_deref(), Some("dockerDesktopLinuxEngine"));
        assert_eq!(local_pipe(r"npipe:\\.\pipe\docker_engine").as_deref(), Some("docker_engine"));
        for remote in [
            "npipe:////server/pipe/docker_engine",
            "tcp://10.0.0.5:2376",
            "ssh://me@build",
            "unix:///var/run/docker.sock",
            "npipe:////./pipe/../x",
            "npipe:////./pipe/.",
            "npipe:////./pipe/..",
            "",
        ] {
            assert_eq!(local_pipe(remote), None, "{remote}");
        }
    }

    #[test]
    fn bind_sources_become_windows_paths_where_they_can() {
        assert_eq!(host_path("/run/desktop/mnt/host/c/dev/app").as_deref(), Some(r"C:\dev\app"));
        assert_eq!(host_path("/host_mnt/d/work/x/").as_deref(), Some(r"D:\work\x"));
        assert_eq!(host_path("/mnt/c/Users/me/proj").as_deref(), Some(r"C:\Users\me\proj"));
        assert_eq!(host_path("c:/dev/app").as_deref(), Some(r"C:\dev\app"));
        assert_eq!(host_path("/mnt/host/c/dev/app").as_deref(), Some(r"C:\dev\app"));
        assert_eq!(host_path("/c/dev/app").as_deref(), Some(r"C:\dev\app"), "WSL with automount root=/");
        assert_eq!(host_path(r"\\?\C:\dev\app").as_deref(), Some(r"C:\dev\app"));
        for vm in ["/var/lib/docker/volumes/x/_data", "/home/me/app", "/mnt/wsl/x", "relative"] {
            assert_eq!(host_path(vm), None, "{vm}");
        }
    }

    #[test]
    fn objects_link_to_projects_and_runs_without_trusting_more_than_labels() {
        let links = Links {
            projects: vec![("web".into(), r"C:\dev\web".into())],
            artifacts: vec![(DockerObject::Volume, "pgdata".into(), 7)],
        };
        let c = container(
            &json!({"Id": "0123456789abcdef0123", "Names": ["/web-db-1"], "Image": "postgres:17", "State": "exited",
                "Status": "Exited (0)", "Labels": {"com.docker.compose.project": "web", "io.mogumogu.run": "3"},
                "Mounts": [{"Type": "bind", "Source": "/run/desktop/mnt/host/c/dev/web/data", "Destination": "/data"},
                           {"Type": "volume", "Name": "pgdata", "Source": "/var/lib/docker/volumes/pgdata/_data",
                            "Destination": "/var/lib/postgresql/data"}]}),
            &links,
        );
        assert_eq!(
            (c.id.as_str(), c.name.as_str(), c.run, c.compose.as_deref()),
            ("0123456789ab", "web-db-1", Some(3), Some("web"))
        );
        assert_eq!(c.mounts[0].host.as_deref(), Some(r"C:\dev\web\data"));
        assert_eq!(c.mounts[0].project.as_deref(), Some("web"));
        assert_eq!((c.mounts[1].source.as_str(), c.mounts[1].host.as_ref()), ("pgdata", None));
        let v = volume(&json!({"Name": "pgdata", "Driver": "local", "Labels": null}), &links);
        assert_eq!(v.run, Some(7), "registered by run 7");
        assert_eq!(links.project(r"C:\dev\website\x"), None, "a prefix of the name is another folder");
    }
}
