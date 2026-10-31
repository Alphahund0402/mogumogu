//! Databases registered for monitoring and their measured sizes. Only
//! places and sizes are stored, never content. Ending the monitoring removes
//! its records, not the database.
use super::Repository;
use super::activity::log_in;
use crate::domain::{DatabaseEngine, DatabasePlace, DatabaseSize, MonitoredDatabase};
use crate::platform::FileIdentity;
use crate::{Error, Result, privacy};
use rusqlite::{OptionalExtension, params};

/// Bounds: more is refused, never silently dropped.
const MAX_DATABASES: i64 = 500;
/// Measurements kept per database.
const SIZES_KEPT: i64 = 100;

/// A database as it is registered.
#[derive(Clone, Debug)]
pub struct NewDatabase {
    pub name: String,
    pub engine: DatabaseEngine,
    pub place: DatabasePlace,
    pub real_location: Option<String>,
    /// Identity of the folder, or of the file's folder.
    pub identity: Option<FileIdentity>,
    pub file_name: Option<String>,
    pub project_id: Option<i64>,
}

/// A database row: the record, its real place, identity and file name.
type Row = (MonitoredDatabase, Option<String>, Option<String>, Option<String>);

/// What a measurement needs besides the record shown.
#[derive(Clone, Debug)]
pub struct StoredDatabase {
    pub database: MonitoredDatabase,
    pub real_location: Option<String>,
    pub identity: Option<FileIdentity>,
    pub file_name: Option<String>,
}

impl Repository {
    pub fn register_database(&mut self, new: &NewDatabase) -> Result<i64> {
        self.require_local()?;
        let now = self.now();
        let (location, context, volume) = match &new.place {
            DatabasePlace::Path(path) => (path.as_str(), None, None),
            DatabasePlace::Volume { context, volume } => ("", Some(context.as_str()), Some(volume.as_str())),
        };
        self.write(|tx| {
            let (count, same): (i64, i64) = tx.query_row(
                "SELECT COUNT(*), COALESCE(SUM(lower(location) = lower(?1) AND COALESCE(docker_volume, '') = COALESCE(?2, '')
                                            AND COALESCE(docker_context, '') = COALESCE(?3, '')), 0)
                 FROM monitored_databases",
                params![location, volume, context],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if same > 0 {
                return Err(Error::conflict("Diese Datenbank wird schon überwacht."));
            }
            if count >= MAX_DATABASES {
                return Err(Error::blocked("Limit für überwachte Datenbanken erreicht."));
            }
            tx.execute(
                "INSERT INTO monitored_databases(name, engine, location, real_location, identity, file_name,
                                                 docker_context, docker_volume, project_id, registered_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    privacy::label(&new.name),
                    new.engine,
                    location,
                    new.real_location,
                    new.identity.map(|i| i.to_string()),
                    new.file_name,
                    context,
                    volume,
                    new.project_id,
                    now
                ],
            )?;
            let id = tx.last_insert_rowid();
            log_in(tx, now, "databases", "Datenbank wird überwacht", "Nur Größe und Änderungszeit; der Inhalt wird nie gelesen.")?;
            Ok(id)
        })
    }

    pub fn databases(&mut self) -> Result<Vec<StoredDatabase>> {
        let rows: Vec<Row> = self.query(
            "SELECT d.id, d.name, d.engine, d.location, d.docker_context, d.docker_volume, d.project_id, p.name,
                    d.registered_at, d.real_location, d.identity, d.file_name
             FROM monitored_databases d LEFT JOIN projects p ON p.id = d.project_id ORDER BY d.id",
            [],
            |r| {
                let location: String = r.get(3)?;
                let place = match (r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?) {
                    (Some(context), Some(volume)) => DatabasePlace::Volume { context, volume },
                    _ => DatabasePlace::Path(location),
                };
                Ok((
                    MonitoredDatabase {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        engine: r.get(2)?,
                        place,
                        project_id: r.get(6)?,
                        project: r.get(7)?,
                        registered_at: r.get(8)?,
                        last: None,
                        growth: None,
                    },
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                ))
            },
        )?;
        rows.into_iter()
            .map(|(database, real_location, identity, file_name)| {
                let identity = identity.map(|i| i.parse()).transpose()?;
                Ok(StoredDatabase { database, real_location, identity, file_name })
            })
            .collect()
    }

    pub fn record_database_size(&mut self, id: i64, size: &DatabaseSize) -> Result<()> {
        self.require_local()?;
        let clamp = |b: Option<u64>| b.map(|b| i64::try_from(b).unwrap_or(i64::MAX));
        self.write(|tx| {
            tx.execute(
                "INSERT INTO database_sizes(database_id, measured_at, bytes, files, modified, note)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    size.measured_at,
                    clamp(size.bytes),
                    clamp(size.files),
                    size.modified,
                    privacy::label(&size.note)
                ],
            )?;
            tx.execute(
                "DELETE FROM database_sizes WHERE database_id = ?1 AND id NOT IN
                    (SELECT id FROM database_sizes WHERE database_id = ?1 ORDER BY id DESC LIMIT ?2)",
                params![id, SIZES_KEPT],
            )?;
            Ok(())
        })
    }

    /// Measurements of a database, oldest first.
    pub fn database_sizes(&mut self, id: i64) -> Result<Vec<DatabaseSize>> {
        self.query(
            "SELECT measured_at, bytes, files, modified, COALESCE(note, '') FROM database_sizes
             WHERE database_id = ?1 ORDER BY id",
            [id],
            |r| {
                let size = |i: usize| r.get::<_, Option<i64>>(i).map(|b| b.map(|b| b.max(0) as u64));
                Ok(DatabaseSize {
                    measured_at: r.get(0)?,
                    bytes: size(1)?,
                    files: size(2)?,
                    modified: r.get(3)?,
                    note: r.get(4)?,
                })
            },
        )
    }

    /// Ends the monitoring: its records go, the database stays untouched.
    pub fn unregister_database(&mut self, id: i64) -> Result<()> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            let name: Option<String> =
                tx.query_row("SELECT name FROM monitored_databases WHERE id = ?1", [id], |r| r.get(0)).optional()?;
            if name.is_none() {
                return Err(Error::not_found("Diese Datenbank wird nicht überwacht."));
            }
            tx.execute("DELETE FROM database_sizes WHERE database_id = ?1", [id])?;
            tx.execute("DELETE FROM monitored_databases WHERE id = ?1", [id])?;
            log_in(tx, now, "databases", "Überwachung beendet", "Die Datenbank selbst bleibt unverändert.")
        })
    }
}
