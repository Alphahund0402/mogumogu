//! What keeps a folder in place besides its own evidence: the person's keep
//! decisions, artifacts registered by agent runs (until the person releases
//! a finished run), monitored databases, and folders an approved Docker
//! engine binds in. The tidy-up report, the planner and the executor
//! ask the same. Holds only ever take folders out.
//!
//! A hold counts at the place it names and, where that exists, at its real
//! place (long names, links resolved; metadata only). Targets are compared
//! by their real place and by their registered spelling, so a short name,
//! a junction or a SUBST drive on either side does not slip through.
use crate::domain::{DatabasePlace, Hold, HoldKind, same_location};
use crate::storage::Repository;
use crate::{Result, docker, platform};
use std::path::Path;

/// Reads one environment variable: where the Docker client is configured.
pub type Env = dyn Fn(&str) -> Option<String>;

/// Holds, and why some could not be determined: an approved Docker engine
/// that changed, cannot be asked or answers oddly leaves its bind mounts
/// unknown, which blocks.
#[derive(Debug, Default)]
pub struct Holds {
    pub holds: Vec<Hold>,
    pub unknown: Vec<String>,
}

/// Real places already resolved during one plan, by the spelling a hold
/// names: an execution asks before each move, not before each hold again.
pub type Places = std::collections::HashMap<String, Option<String>>;

impl Holds {
    fn add(&mut self, places: &mut Places, kind: HoldKind, location: String, reason: String) {
        let real = places.entry(location.clone()).or_insert_with(|| platform::real_place(Path::new(&location)));
        if let Some(real) = real.clone()
            && !same_location(&real, &location)
        {
            self.holds.push(Hold { kind, location: real, reason: reason.clone() });
        }
        self.holds.push(Hold { kind, location, reason });
    }
}

pub fn holds(repo: &mut Repository, env: &Env, places: &mut Places) -> Result<Holds> {
    let mut found = Holds::default();
    for location in repo.kept_locations()? {
        found.add(places, HoldKind::Keep, location, "von dir behalten".into());
    }
    for (location, run, kind) in repo.artifact_locations()? {
        found.add(places, HoldKind::Run, location, format!("gehört zu Lauf {} ({})", run.title(), kind.label()));
    }
    for stored in repo.databases()? {
        if let DatabasePlace::Path(location) = stored.database.place {
            let reason = format!("überwachte Datenbank „{}“", stored.database.name);
            if let Some(real) = stored.real_location {
                found.add(places, HoldKind::Database, real, reason.clone());
            }
            found.add(places, HoldKind::Database, location, reason);
        }
    }
    let contexts = docker::contexts(env);
    for (context, pipe, _) in repo.docker_approvals()? {
        let mounts =
            docker::still_approved(&contexts, &context, &pipe).and_then(|()| docker::mount_holds(&context, &pipe));
        match mounts {
            Ok(mounts) => found.holds.extend(mounts),
            Err(reason) => found.unknown.push(format!("Docker-Einbindungen nicht prüfbar: {reason}")),
        }
    }
    Ok(found)
}
