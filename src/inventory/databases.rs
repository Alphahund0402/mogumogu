//! Sizes of monitored databases from directory metadata, and places where
//! database servers keep their data. Nothing is opened: no database file is
//! read and no server is contacted. A data folder is measured like any
//! approved folder; a database file with its companions (`-wal`, `-shm`,
//! `-journal`) from the listing of its own folder, bound to that folder's
//! identity. Every folder on the way is opened by handle and a link refuses
//! (`ScopeRoot::open_local`), so a junction or symbolic link never leads a
//! measurement to a share. Suggestions list only the names in a few known
//! install folders.
use super::measure_root;
use crate::clock::Timestamp;
use crate::domain::{DatabaseEngine, DatabasePlace, DatabaseSize, DatabaseSuggestion, fold_case};
use crate::fsread::{self, Budget, ScopeRoot};
use crate::limits::ScanLimits;
use crate::platform::{self, FileIdentity};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Files that belong to a database file of this name.
const COMPANIONS: &[&str] = &["", "-wal", "-shm", "-journal"];

pub fn measure_folder(path: &Path, identity: Option<FileIdentity>, now: Timestamp) -> DatabaseSize {
    let measured = match ScopeRoot::open_local(path, identity) {
        Ok(root) => measure_root(root),
        Err(e) => {
            return DatabaseSize { measured_at: now, bytes: None, files: None, modified: None, note: e.to_string() };
        }
    };
    DatabaseSize {
        measured_at: now,
        bytes: measured.bytes,
        files: measured.bytes.map(|_| measured.entries),
        modified: None,
        note: measured.reason.unwrap_or_default(),
    }
}

pub fn measure_file(folder: &Path, identity: Option<FileIdentity>, name: &str, now: Timestamp) -> DatabaseSize {
    let unknown = |note: String| DatabaseSize { measured_at: now, bytes: None, files: None, modified: None, note };
    let root = match ScopeRoot::open_local(folder, identity) {
        Ok(root) => root,
        Err(e) => return unknown(e.to_string()),
    };
    let limits =
        ScanLimits { max_depth: 1, max_entries: 200_000, max_total_bytes: 0, time_budget: Duration::from_secs(10) };
    let entries = match fsread::list(root.handle(), &mut Budget::new(limits)) {
        Ok((entries, false)) => entries,
        Ok((_, true)) => return unknown("Ordner zu groß, um ihn vollständig zu listen".into()),
        Err(e) => return unknown(format!("Ordner nicht lesbar: {e}")),
    };
    let wanted: Vec<String> = COMPANIONS.iter().map(|suffix| fold_case(&format!("{name}{suffix}"))).collect();
    let mine: Vec<_> = entries
        .iter()
        .filter(|e| !e.is_dir() && !e.is_reparse())
        .filter(|e| e.name_str().is_some_and(|n| wanted.contains(&fold_case(n))))
        .collect();
    if !mine.iter().any(|e| e.name_str().is_some_and(|n| fold_case(n) == wanted[0])) {
        return unknown("Datenbankdatei nicht vorhanden".into());
    }
    DatabaseSize {
        measured_at: now,
        bytes: Some(mine.iter().map(|e| e.size).fold(0, u64::saturating_add)),
        files: Some(mine.len() as u64),
        modified: mine.iter().map(|e| e.modified).max(),
        note: String::new(),
    }
}

/// The engine of a data folder from its names.
pub fn folder_engine(root: &ScopeRoot) -> Option<DatabaseEngine> {
    let limits =
        ScanLimits { max_depth: 1, max_entries: 10_000, max_total_bytes: 0, time_budget: Duration::from_secs(5) };
    let (entries, _) = fsread::list(root.handle(), &mut Budget::new(limits)).ok()?;
    DatabaseEngine::of_folder(entries.iter().filter_map(|e| e.name_str()))
}

/// Whether `name` is a plain file in `folder`: not a folder, not a link.
pub fn plain_file(folder: &ScopeRoot, name: &str) -> Result<bool, String> {
    let limits =
        ScanLimits { max_depth: 1, max_entries: 200_000, max_total_bytes: 0, time_budget: Duration::from_secs(10) };
    let entries = match fsread::list(folder.handle(), &mut Budget::new(limits)) {
        Ok((entries, false)) => entries,
        Ok((_, true)) => return Err("Ordner zu groß, um ihn vollständig zu listen".into()),
        Err(e) => return Err(format!("Ordner nicht lesbar: {e}")),
    };
    let wanted = fold_case(name);
    match entries.iter().find(|e| e.name_str().is_some_and(|n| fold_case(n) == wanted)) {
        Some(entry) if entry.is_reparse() => Err("Eine Verknüpfung wird nicht überwacht.".into()),
        Some(entry) => Ok(!entry.is_dir()),
        None => Ok(false),
    }
}

/// Where database servers on Windows keep their data by default. Only the
/// names in these install folders are listed (for version folders); the
/// data folders themselves are not opened.
pub fn known_places(env: &dyn Fn(&str) -> Option<String>) -> Vec<DatabaseSuggestion> {
    let program_files = env("ProgramFiles").map(PathBuf::from);
    let program_data = env("ProgramData").map(PathBuf::from);
    let system_drive = env("SystemDrive").map(|d| PathBuf::from(format!("{}\\", d.trim_end_matches('\\'))));
    let mut found = Vec::new();
    let mut each_version = |base: Option<&PathBuf>, folder: &str, below: &[&str], engine: DatabaseEngine| {
        let Some(base) = base else { return };
        let install = base.join(folder);
        for version in names(&install).into_iter().take(20) {
            let data = below.iter().fold(install.join(&version), |path, part| path.join(part));
            if is_dir(&data) {
                found.push(suggestion(engine, &data, &format!("Standardordner von {} {version}", engine.label())));
            }
        }
    };
    each_version(program_files.as_ref(), "PostgreSQL", &["data"], DatabaseEngine::Postgres);
    each_version(program_data.as_ref(), "MySQL", &["Data"], DatabaseEngine::MySql);
    each_version(program_files.as_ref(), "MongoDB\\Server", &["data"], DatabaseEngine::MongoDb);
    if let Some(base) = &program_files {
        for name in names(base).into_iter().filter(|n| fold_case(n).starts_with("mariadb")) {
            let data = base.join(&name).join("data");
            if is_dir(&data) {
                found.push(suggestion(DatabaseEngine::MySql, &data, &format!("Standardordner von {name}")));
            }
        }
        let redis = base.join("Redis");
        if is_dir(&redis) {
            found.push(suggestion(DatabaseEngine::Redis, &redis, "Installationsordner von Redis"));
        }
    }
    if let Some(drive) = system_drive {
        let mongo = drive.join("data").join("db");
        if is_dir(&mongo) {
            found.push(suggestion(DatabaseEngine::MongoDb, &mongo, "Standardordner von mongod (\\data\\db)"));
        }
    }
    found
}

fn suggestion(engine: DatabaseEngine, path: &Path, source: &str) -> DatabaseSuggestion {
    let name = path.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    DatabaseSuggestion {
        name: format!("{} {}", engine.label(), name.unwrap_or_default()).trim().to_string(),
        engine,
        place: DatabasePlace::Path(path.to_string_lossy().into_owned()),
        source: source.into(),
    }
}

/// Names of the folders directly in `path` on a local fixed disk; nothing
/// below them is opened, links are not listed.
fn names(path: &Path) -> Vec<String> {
    if !platform::local_fixed_path(path) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(path) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .take(200)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

fn is_dir(path: &Path) -> bool {
    platform::local_fixed_path(path)
        && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
