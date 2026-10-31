//! Databases the person registers for monitoring. Registering approves
//! measuring that one place from metadata, bound to its identity; mogumogu
//! never connects to a server or reads a database file. A monitored database
//! is held and never enters a cleanup plan. Measured on request and with
//! every full tidy-up report; sizes are kept as history.
use super::Core;
use crate::domain::*;
use crate::fsread::{RootError, ScopeRoot};
use crate::inventory::databases as measure;
use crate::storage::{NewDatabase, StoredDatabase};
use crate::{Error, Result, docker, platform, validation};
use std::path::Path;

/// Suggestions shown at most.
const SUGGESTIONS: usize = 30;

/// How long a full tidy-up report measures monitored databases at most.
const AUTOMATIC_BUDGET: std::time::Duration = std::time::Duration::from_secs(20);

fn within(path: &str, root: &str) -> bool {
    let (path, root) = (fold_case(path), fold_case(root.trim_end_matches('\\')));
    path == root || path.starts_with(&format!("{root}\\"))
}

impl Core {
    /// Monitored databases with their last size and growth, and what else
    /// looks like a database: known data folders, database files the last
    /// tidy-up report found, volumes of approved Docker contexts.
    pub fn database_overview(&mut self) -> Result<DatabaseOverview> {
        let now = self.repo.now();
        let mut databases = Vec::new();
        for stored in self.repo.databases()? {
            let sizes = self.repo.database_sizes(stored.database.id)?;
            databases.push(MonitoredDatabase {
                last: sizes.last().cloned(),
                growth: database_growth(&sizes, now),
                ..stored.database
            });
        }
        let mut suggestions = measure::known_places(&*self.env);
        for project in self.tidy.iter().flat_map(|t| &t.projects) {
            for item in project.items.iter().filter(|i| i.label == "Datenbank") {
                let path = format!("{}\\{}", project.path.trim_end_matches('\\'), item.rel_path.replace('/', "\\"));
                let engine = DatabaseEngine::of_file(&item.rel_path).unwrap_or(DatabaseEngine::Other);
                let name = item.rel_path.rsplit('/').next().unwrap_or(&item.rel_path).to_string();
                let source = format!("Datenbankdatei in {}", project.name);
                suggestions.push(DatabaseSuggestion { name, engine, place: DatabasePlace::Path(path), source });
            }
        }
        let contexts = docker::contexts(&*self.env);
        for (context, pipe, _) in self.repo.docker_approvals()? {
            if docker::still_approved(&contexts, &context, &pipe).is_err() {
                continue;
            }
            for volume in docker::volume_names(&pipe).unwrap_or_default() {
                if let Some(engine) = DatabaseEngine::of_volume(&volume) {
                    let source = format!("Docker-Volume in „{context}“");
                    let place = DatabasePlace::Volume { context: context.clone(), volume: volume.clone() };
                    suggestions.push(DatabaseSuggestion { name: volume, engine, place, source });
                }
            }
        }
        let monitored = |place: &DatabasePlace| {
            databases.iter().any(|d| match (&d.place, place) {
                (DatabasePlace::Path(a), DatabasePlace::Path(b)) => same_location(a, b),
                (a, b) => a == b,
            })
        };
        suggestions.retain(|s| !monitored(&s.place));
        suggestions.truncate(SUGGESTIONS);
        Ok(DatabaseOverview { databases, suggestions })
    }

    /// Registers a database for monitoring and measures it once.
    pub fn register_database(
        &mut self,
        place: &DatabasePlace,
        name: Option<&str>,
        engine: Option<DatabaseEngine>,
    ) -> Result<MonitoredDatabase> {
        self.repo.require_local()?;
        let new = match place {
            DatabasePlace::Path(path) => self.path_database(path, name, engine)?,
            DatabasePlace::Volume { context, volume } => self.volume_database(context, volume, name, engine)?,
        };
        let id = self.repo.register_database(&new)?;
        self.measure_databases(Some(id))?;
        self.database_overview()?
            .databases
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| Error::not_found("Datenbank nicht gefunden."))
    }

    fn path_database(&mut self, path: &str, name: Option<&str>, engine: Option<DatabaseEngine>) -> Result<NewDatabase> {
        let path = validation::windows_path(path)?;
        let place = Path::new(&path);
        if !platform::local_fixed_path(place) {
            return Err(Error::blocked("Nur Datenbanken auf festen lokalen Laufwerken lassen sich überwachen."));
        }
        // Every folder on the way is opened by handle and a link refuses, so
        // neither this nor any later measurement reaches a share.
        let component = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (identity, file_name, engine, real) = match ScopeRoot::open_local(place, None) {
            Ok(root) => {
                let engine = engine.or_else(|| measure::folder_engine(&root));
                (root.identity(), None, engine, root.handle().final_path().ok())
            }
            Err(_) => {
                let folder = place.parent().ok_or_else(|| Error::invalid("Datei ohne Ordner."))?;
                let root = ScopeRoot::open_local(folder, None).map_err(|e| match e {
                    RootError::Unavailable(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        Error::not_found("Diesen Ort gibt es nicht.")
                    }
                    e => Error::blocked(format!(
                        "Ort nicht prüfbar ({e}); gib einen Ordner oder eine Datei ohne Verknüpfung auf dem Weg an."
                    )),
                })?;
                let file = component(place);
                if !measure::plain_file(&root, &file).map_err(Error::blocked)? {
                    return Err(Error::not_found("Diesen Ort gibt es nicht."));
                }
                let engine = engine.or_else(|| DatabaseEngine::of_file(&file));
                let real = root.handle().final_path().ok().map(|f| format!("{}\\{file}", f.trim_end_matches('\\')));
                (root.identity(), Some(file), engine, real)
            }
        };
        // A data folder is often called `data`; its parent and engine name it better.
        let fallback = match component(place).to_ascii_lowercase().as_str() {
            "data" | "db" => {
                let parent = place.parent().map(component).unwrap_or_default();
                format!("{} {parent}", engine.unwrap_or(DatabaseEngine::Other).label())
            }
            _ => component(place),
        };
        let project_id = self
            .repo
            .projects()?
            .into_iter()
            .filter(|p| within(&path, &p.path))
            .max_by_key(|p| p.path.len())
            .map(|p| p.id);
        Ok(NewDatabase {
            name: validation::name(name.unwrap_or(&fallback))?,
            engine: engine.unwrap_or(DatabaseEngine::Other),
            place: DatabasePlace::Path(path),
            real_location: real,
            identity: Some(identity),
            file_name,
            project_id,
        })
    }

    fn volume_database(
        &mut self,
        context: &str,
        volume: &str,
        name: Option<&str>,
        engine: Option<DatabaseEngine>,
    ) -> Result<NewDatabase> {
        let plain = |c: char| c.is_ascii_alphanumeric() || "_.-".contains(c);
        if volume.is_empty() || volume.len() > 255 || !volume.chars().all(plain) {
            return Err(Error::invalid("Volume-Name aus Buchstaben, Ziffern und _.-"));
        }
        let (context, pipe, _) = self
            .repo
            .docker_approvals()?
            .into_iter()
            .find(|(n, ..)| n == context)
            .ok_or_else(|| Error::blocked("Zuerst den Docker-Kontext freigeben."))?;
        docker::still_approved(&docker::contexts(&*self.env), &context, &pipe).map_err(Error::blocked)?;
        if !docker::volume_names(&pipe).map_err(Error::blocked)?.iter().any(|v| v == volume) {
            return Err(Error::not_found("Dieses Volume kennt die Docker-Engine nicht."));
        }
        Ok(NewDatabase {
            name: validation::name(name.unwrap_or(volume))?,
            engine: engine.or_else(|| DatabaseEngine::of_volume(volume)).unwrap_or(DatabaseEngine::Other),
            place: DatabasePlace::Volume { context, volume: volume.to_string() },
            real_location: None,
            identity: None,
            file_name: None,
            project_id: None,
        })
    }

    /// Measures one monitored database, or all.
    pub fn measure_databases(&mut self, id: Option<i64>) -> Result<DatabaseOverview> {
        self.repo.require_local()?;
        let stored: Vec<StoredDatabase> =
            self.repo.databases()?.into_iter().filter(|d| id.is_none_or(|id| d.database.id == id)).collect();
        if id.is_some() && stored.is_empty() {
            return Err(Error::not_found("Diese Datenbank wird nicht überwacht."));
        }
        for database in stored {
            let size = self.measure_one(&database);
            self.repo.record_database_size(database.database.id, &size)?;
        }
        self.database_overview()
    }

    /// Measures the monitored databases in folders and files; volumes only
    /// on request, because Docker's accounting can take a while. A full
    /// report waits for this, so it stops after [`AUTOMATIC_BUDGET`]; the
    /// longest unmeasured go first, the rest follow next time.
    pub(super) fn measure_path_databases(&mut self) -> Result<()> {
        let started = std::time::Instant::now();
        let mut due = Vec::new();
        for database in self.repo.databases()? {
            if matches!(database.database.place, DatabasePlace::Path(_)) {
                let last = self.repo.database_sizes(database.database.id)?.last().map(|s| s.measured_at);
                due.push((last.unwrap_or(i64::MIN), database));
            }
        }
        due.sort_by_key(|(last, _)| *last);
        for (_, database) in due {
            if started.elapsed() >= AUTOMATIC_BUDGET {
                break;
            }
            let size = self.measure_one(&database);
            self.repo.record_database_size(database.database.id, &size)?;
        }
        Ok(())
    }

    fn measure_one(&mut self, stored: &StoredDatabase) -> DatabaseSize {
        let now = self.repo.now();
        match (&stored.database.place, &stored.file_name) {
            (DatabasePlace::Path(path), Some(file)) => match Path::new(path).parent() {
                Some(folder) => measure::measure_file(folder, stored.identity, file, now),
                None => DatabaseSize {
                    measured_at: now,
                    bytes: None,
                    files: None,
                    modified: None,
                    note: "Kein Ordner".into(),
                },
            },
            (DatabasePlace::Path(path), None) => measure::measure_folder(Path::new(path), stored.identity, now),
            (DatabasePlace::Volume { context, volume }, _) => {
                let unknown =
                    |note: String| DatabaseSize { measured_at: now, bytes: None, files: None, modified: None, note };
                let approvals = match self.repo.docker_approvals() {
                    Ok(approvals) => approvals,
                    Err(e) => return unknown(e.to_string()),
                };
                let Some((_, pipe, _)) = approvals.into_iter().find(|(n, ..)| n == context) else {
                    return unknown("Docker-Kontext nicht mehr freigegeben".into());
                };
                if let Err(reason) = docker::still_approved(&docker::contexts(&*self.env), context, &pipe) {
                    return unknown(reason);
                }
                match docker::volume_size(&pipe, volume) {
                    Ok(bytes) => {
                        DatabaseSize { measured_at: now, bytes, files: None, modified: None, note: String::new() }
                    }
                    Err(reason) => unknown(reason),
                }
            }
        }
    }

    pub fn database_history(&mut self, id: i64) -> Result<(MonitoredDatabase, Vec<DatabaseSize>)> {
        let overview = self.database_overview()?;
        let database = overview
            .databases
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| Error::not_found("Diese Datenbank wird nicht überwacht."))?;
        Ok((database, self.repo.database_sizes(id)?))
    }

    /// Ends the monitoring; the owner accepts this only from the dashboard,
    /// because it ends the database's hold.
    pub fn unregister_database(&mut self, id: i64) -> Result<DatabaseOverview> {
        self.repo.unregister_database(id)?;
        self.database_overview()
    }
}
