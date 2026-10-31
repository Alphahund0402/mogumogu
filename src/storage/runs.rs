//! Agent runs and their artifacts, as cooperating harnesses register them,
//! and approvals of local Docker contexts. Nothing here removes a run or an
//! artifact: registration adds provenance and protection only.
use super::Repository;
use super::activity::log_in;
use super::resources::require_project;
use crate::clock::Timestamp;
use crate::domain::{AgentRun, ArtifactKind, DockerObject, RunArtifact, RunState};
use crate::{Error, Result, privacy};
use rusqlite::{OptionalExtension, Row, Transaction, params};

/// Bounds: more is refused, never silently dropped.
const MAX_RUNS: i64 = 5_000;
const ARTIFACTS_PER_RUN: i64 = 200;
const MAX_ARTIFACTS: i64 = 20_000;

/// A new artifact; `docker` is context, object kind and id or name.
#[derive(Clone, Debug)]
pub struct NewArtifact {
    pub kind: ArtifactKind,
    pub location: String,
    pub docker: Option<(String, DockerObject, String)>,
    pub purpose: Option<String>,
}

const RUN_COLUMNS: &str = "r.id, r.project_id, p.name, r.harness, r.purpose, r.state, r.started_at, r.updated_at,
     r.finished_at, r.note, r.released_at";

fn map_run(r: &Row<'_>) -> rusqlite::Result<AgentRun> {
    Ok(AgentRun {
        id: r.get(0)?,
        project_id: r.get(1)?,
        project: r.get(2)?,
        harness: r.get(3)?,
        purpose: r.get(4)?,
        state: r.get(5)?,
        started_at: r.get(6)?,
        updated_at: r.get(7)?,
        finished_at: r.get(8)?,
        note: r.get(9)?,
        released_at: r.get(10)?,
        artifact_count: 0,
        artifacts: Vec::new(),
    })
}

fn artifacts_of(tx: &Transaction<'_>, run_id: i64) -> Result<Vec<RunArtifact>> {
    let mut statement = tx.prepare(
        "SELECT id, kind, location, docker_context, docker_type, docker_id, purpose, created_at
         FROM run_artifacts WHERE run_id = ?1 ORDER BY id",
    )?;
    let rows = statement.query_map([run_id], |r| {
        let docker = match (r.get::<_, Option<String>>(3)?, r.get::<_, Option<DockerObject>>(4)?, r.get(5)?) {
            (Some(context), Some(kind), Some(id)) => Some((context, kind, id)),
            _ => None,
        };
        Ok(RunArtifact {
            id: r.get(0)?,
            kind: r.get(1)?,
            location: r.get(2)?,
            docker,
            purpose: r.get(6)?,
            created_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

impl Repository {
    pub fn start_run(&mut self, project_id: i64, harness: &str, purpose: Option<&str>) -> Result<i64> {
        self.require_local()?;
        let now = self.now();
        let purpose = purpose.map(privacy::label);
        self.write(|tx| {
            require_project(tx, project_id)?;
            let count: i64 = tx.query_row("SELECT COUNT(*) FROM agent_runs", [], |r| r.get(0))?;
            if count >= MAX_RUNS {
                return Err(Error::blocked("Limit für registrierte Läufe erreicht."));
            }
            tx.execute(
                "INSERT INTO agent_runs(project_id, harness, purpose, state, started_at, updated_at)
                 VALUES(?1, ?2, ?3, 'running', ?4, ?4)",
                params![project_id, privacy::label(harness), purpose, now],
            )?;
            let id = tx.last_insert_rowid();
            log_in(tx, now, "runs", &format!("Lauf #{id} registriert"), "Herkunft und Schutz; nichts wird entfernt.")?;
            Ok(id)
        })
    }

    pub fn run(&mut self, id: i64) -> Result<AgentRun> {
        let sql =
            format!("SELECT {RUN_COLUMNS} FROM agent_runs r JOIN projects p ON p.id = r.project_id WHERE r.id = ?1");
        self.read(|tx| {
            let run = tx
                .query_row(&sql, [id], map_run)
                .optional()?
                .ok_or_else(|| Error::not_found("Lauf nicht gefunden."))?;
            let artifacts = artifacts_of(tx, id)?;
            Ok(AgentRun { artifact_count: artifacts.len(), artifacts, ..run })
        })
    }

    /// The newest runs, of one project or of all, each listing its first
    /// `artifacts` artifacts (all are counted).
    pub fn runs(&mut self, project_id: Option<i64>, limit: i64, artifacts: usize) -> Result<Vec<AgentRun>> {
        let sql = format!(
            "SELECT {RUN_COLUMNS} FROM agent_runs r JOIN projects p ON p.id = r.project_id
             WHERE ?1 IS NULL OR r.project_id = ?1 ORDER BY r.id DESC LIMIT ?2"
        );
        self.read(|tx| {
            let mut statement = tx.prepare(&sql)?;
            let runs =
                statement.query_map(params![project_id, limit], map_run)?.collect::<rusqlite::Result<Vec<_>>>()?;
            runs.into_iter()
                .map(|run| {
                    let mut listed = artifacts_of(tx, run.id)?;
                    let count = listed.len();
                    listed.truncate(artifacts);
                    Ok(AgentRun { artifact_count: count, artifacts: listed, ..run })
                })
                .collect()
        })
    }

    /// Compare-and-set on the run state; finished runs stay finished.
    pub fn set_run_state(&mut self, id: i64, from: RunState, to: RunState, note: Option<&str>) -> Result<()> {
        self.require_local()?;
        if !from.can_become(to) {
            return Err(Error::conflict(format!(
                "Ein Lauf kann nicht von „{}“ zu „{}“ wechseln.",
                from.label(),
                to.label()
            )));
        }
        let now = self.now();
        let note = note.map(privacy::label);
        self.write(|tx| {
            let changed = tx.execute(
                "UPDATE agent_runs SET state = ?1, updated_at = ?2, note = COALESCE(?3, note),
                    finished_at = CASE WHEN ?1 IN ('succeeded','failed','cancelled') THEN ?2 ELSE finished_at END
                 WHERE id = ?4 AND state = ?5",
                params![to, now, note, id, from],
            )?;
            if changed == 0 {
                return Err(Error::conflict("Laufzustand wurde zwischenzeitlich geändert."));
            }
            log_in(tx, now, "runs", &format!("Lauf #{id}: {}", to.label()), "Vom Harness gemeldet.")
        })
    }

    pub fn add_artifact(&mut self, run_id: i64, artifact: &NewArtifact) -> Result<i64> {
        self.require_local()?;
        let now = self.now();
        let purpose = artifact.purpose.as_deref().map(privacy::label);
        let (context, kind, id) = match &artifact.docker {
            Some((context, kind, id)) => (Some(context.as_str()), Some(*kind), Some(id.as_str())),
            None => (None, None, None),
        };
        self.write(|tx| {
            let released: Option<Option<i64>> = tx
                .query_row("SELECT released_at FROM agent_runs WHERE id = ?1", [run_id], |r| r.get(0))
                .optional()?;
            match released {
                None => return Err(Error::not_found("Lauf nicht gefunden.")),
                // A released run holds nothing; a new artifact would look held and not be.
                Some(Some(_)) => {
                    return Err(Error::conflict("Dieser Lauf ist freigegeben; er nimmt keine Artefakte mehr an."));
                }
                Some(None) => {}
            }
            let (here, total): (i64, i64) = tx.query_row(
                "SELECT COALESCE(SUM(run_id = ?1), 0), COUNT(*) FROM run_artifacts",
                [run_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if here >= ARTIFACTS_PER_RUN || total >= MAX_ARTIFACTS {
                return Err(Error::blocked("Limit für registrierte Artefakte erreicht."));
            }
            tx.execute(
                "INSERT INTO run_artifacts(run_id, kind, location, docker_context, docker_type, docker_id, purpose, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![run_id, artifact.kind, artifact.location, context, kind, id, purpose, now],
            )?;
            let artifact_id = tx.last_insert_rowid();
            tx.execute("UPDATE agent_runs SET updated_at = ?1 WHERE id = ?2", params![now, run_id])?;
            Ok(artifact_id)
        })
    }

    /// Every registered file or folder of a run not released, with its run.
    pub fn artifact_locations(&mut self) -> Result<Vec<(String, AgentRun, ArtifactKind)>> {
        let sql = format!(
            "SELECT {RUN_COLUMNS}, a.location, a.kind FROM run_artifacts a
             JOIN agent_runs r ON r.id = a.run_id JOIN projects p ON p.id = r.project_id
             WHERE a.kind <> 'docker' AND r.released_at IS NULL ORDER BY a.id"
        );
        self.query(&sql, [], |r| Ok((r.get(11)?, map_run(r)?, r.get(12)?)))
    }

    /// The person releases a finished run's protection: its artifacts are not
    /// held any more. Nothing becomes recoverable by it; the tidy rules decide.
    pub fn release_run(&mut self, id: i64) -> Result<()> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            let changed = tx.execute(
                "UPDATE agent_runs SET released_at = ?1, updated_at = ?1
                 WHERE id = ?2 AND released_at IS NULL AND state IN ('succeeded','failed','cancelled')",
                params![now, id],
            )?;
            if changed == 0 {
                return Err(Error::conflict("Nur ein beendeter, noch geschützter Lauf lässt sich freigeben."));
            }
            log_in(tx, now, "runs", &format!("Lauf #{id} freigegeben"), "Seine Artefakte sind nicht mehr gehalten.")
        })
    }

    /// Docker objects registered by runs: kind, id or name, run.
    pub fn docker_artifacts(&mut self) -> Result<Vec<(DockerObject, String, i64)>> {
        self.query(
            "SELECT docker_type, docker_id, run_id FROM run_artifacts WHERE kind = 'docker' ORDER BY id",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    }

    /// Approved local Docker contexts with the pipe they named.
    pub fn docker_approvals(&mut self) -> Result<Vec<(String, String, Timestamp)>> {
        self.query("SELECT name, pipe, approved_at FROM docker_contexts ORDER BY name", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
    }

    pub fn approve_docker(&mut self, name: &str, pipe: &str) -> Result<()> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO docker_contexts(name, pipe, approved_at) VALUES(?1, ?2, ?3)
                 ON CONFLICT(name) DO UPDATE SET pipe = excluded.pipe, approved_at = excluded.approved_at",
                params![name, pipe, now],
            )?;
            log_in(tx, now, "docker", "Docker-Kontext freigegeben", "Nur lesende Abfragen an die lokale Engine.")
        })
    }

    pub fn revoke_docker(&mut self, name: &str) -> Result<()> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            if tx.execute("DELETE FROM docker_contexts WHERE name = ?1", [name])? == 0 {
                return Err(Error::not_found("Dieser Docker-Kontext ist nicht freigegeben."));
            }
            log_in(tx, now, "docker", "Docker-Freigabe widerrufen", "mogumogu fragt diese Engine nicht mehr.")
        })
    }
}
