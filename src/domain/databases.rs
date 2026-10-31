//! Databases the person registers for monitoring: where they are, how large
//! they are and how they grow. Measured from metadata only – mogumogu never
//! connects to a database server or reads a database file. A monitored
//! database is held and never enters a cleanup plan.
use super::fold_case;
use crate::clock::Timestamp;
use serde::{Deserialize, Serialize};

string_enum! {
    pub enum DatabaseEngine {
        Sqlite => "sqlite",
        DuckDb => "duckdb",
        Postgres => "postgres",
        MySql => "mysql",
        MongoDb => "mongodb",
        Redis => "redis",
        Other => "other",
    }
}

impl DatabaseEngine {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "SQLite",
            Self::DuckDb => "DuckDB",
            Self::Postgres => "PostgreSQL",
            Self::MySql => "MySQL/MariaDB",
            Self::MongoDb => "MongoDB",
            Self::Redis => "Redis",
            Self::Other => "Datenbank",
        }
    }

    /// The engine of a database file, from its extension.
    pub fn of_file(name: &str) -> Option<Self> {
        let name = fold_case(name);
        if [".sqlite", ".sqlite3", ".db", ".db3", ".s3db"].iter().any(|e| name.ends_with(e)) {
            Some(Self::Sqlite)
        } else if name.ends_with(".duckdb") {
            Some(Self::DuckDb)
        } else {
            None
        }
    }

    /// The engine of a data folder, from the names in it.
    pub fn of_folder<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let names: Vec<String> = names.into_iter().map(fold_case).collect();
        let has = |wanted: &str| names.iter().any(|n| n == wanted);
        if has("pg_version") && has("base") {
            Some(Self::Postgres)
        } else if has("ibdata1")
            || (has("mysql") && names.iter().any(|n| n.starts_with("ib_logfile") || n == "undo_001"))
        {
            Some(Self::MySql)
        } else if has("wiredtiger") || has("storage.bson") {
            Some(Self::MongoDb)
        } else if has("dump.rdb") || has("appendonly.aof") || has("appendonlydir") {
            Some(Self::Redis)
        } else {
            None
        }
    }

    /// A guess from a Docker volume's name, `None` when nothing hints.
    pub fn of_volume(name: &str) -> Option<Self> {
        let name = fold_case(name);
        let hints: &[(&str, Self)] = &[
            ("postgres", Self::Postgres),
            ("pgdata", Self::Postgres),
            ("mysql", Self::MySql),
            ("maria", Self::MySql),
            ("mongo", Self::MongoDb),
            ("redis", Self::Redis),
        ];
        hints.iter().find(|(hint, _)| name.contains(hint)).map(|(_, engine)| *engine)
    }
}

/// Where a monitored database lives.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DatabasePlace {
    /// A data folder or a database file, as registered.
    Path(String),
    /// A Docker volume of an approved local context.
    Volume { context: String, volume: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseSize {
    pub measured_at: Timestamp,
    /// Unknown when the measurement was incomplete or not allowed.
    pub bytes: Option<u64>,
    pub files: Option<u64>,
    pub modified: Option<Timestamp>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseGrowth {
    pub since: Timestamp,
    pub until: Timestamp,
    pub measurements: usize,
    pub bytes: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitoredDatabase {
    pub id: i64,
    pub name: String,
    pub engine: DatabaseEngine,
    pub place: DatabasePlace,
    pub project_id: Option<i64>,
    pub project: Option<String>,
    pub registered_at: Timestamp,
    pub last: Option<DatabaseSize>,
    pub growth: Option<DatabaseGrowth>,
}

/// Something that looks like a database and could be monitored.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseSuggestion {
    pub name: String,
    pub engine: DatabaseEngine,
    pub place: DatabasePlace,
    /// Why it is suggested: a known data folder, a file in a project, a volume.
    pub source: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseOverview {
    pub databases: Vec<MonitoredDatabase>,
    pub suggestions: Vec<DatabaseSuggestion>,
}

/// How far back growth is measured.
pub const DATABASE_WINDOW: Timestamp = 90 * 86_400;

/// Growth between the oldest and newest known size of the last 90 days.
pub fn database_growth(sizes: &[DatabaseSize], now: Timestamp) -> Option<DatabaseGrowth> {
    let known: Vec<(Timestamp, u64)> = sizes
        .iter()
        .filter(|s| s.measured_at <= now && now - s.measured_at <= DATABASE_WINDOW)
        .filter_map(|s| Some((s.measured_at, s.bytes?)))
        .collect();
    let first = known.iter().min_by_key(|(at, _)| *at)?;
    let last = known.iter().max_by_key(|(at, _)| *at)?;
    if first.0 == last.0 {
        return None;
    }
    let size = |b: u64| i64::try_from(b).unwrap_or(i64::MAX);
    Some(DatabaseGrowth {
        since: first.0,
        until: last.0,
        measurements: known.len(),
        bytes: size(last.1).saturating_sub(size(first.1)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines_are_told_from_names_alone() {
        assert_eq!(DatabaseEngine::of_file("App.SQLite3"), Some(DatabaseEngine::Sqlite));
        assert_eq!(DatabaseEngine::of_file("warehouse.duckdb"), Some(DatabaseEngine::DuckDb));
        assert_eq!(DatabaseEngine::of_file("notes.txt"), None);
        assert_eq!(DatabaseEngine::of_folder(["PG_VERSION", "base", "global"]), Some(DatabaseEngine::Postgres));
        assert_eq!(DatabaseEngine::of_folder(["ibdata1", "mysql"]), Some(DatabaseEngine::MySql));
        assert_eq!(DatabaseEngine::of_folder(["WiredTiger", "collection-0.wt"]), Some(DatabaseEngine::MongoDb));
        assert_eq!(DatabaseEngine::of_folder(["dump.rdb"]), Some(DatabaseEngine::Redis));
        assert_eq!(DatabaseEngine::of_folder(["base"]), None, "one name is not enough for PostgreSQL");
        assert_eq!(DatabaseEngine::of_volume("web_pgdata"), Some(DatabaseEngine::Postgres));
        assert_eq!(DatabaseEngine::of_volume("node_cache"), None);
    }

    #[test]
    fn growth_uses_known_sizes_of_the_window() {
        let day = 86_400;
        let size = |at: i64, bytes: Option<u64>| DatabaseSize {
            measured_at: at,
            bytes,
            files: None,
            modified: None,
            note: String::new(),
        };
        let now = 200 * day;
        let sizes = [
            size(now - 100 * day, Some(1)),
            size(now - 20 * day, Some(1_000)),
            size(now - 5 * day, None),
            size(now, Some(4_000)),
        ];
        let growth = database_growth(&sizes, now).unwrap();
        assert_eq!((growth.bytes, growth.measurements, growth.since), (3_000, 2, now - 20 * day));
        assert_eq!(database_growth(&sizes[3..], now), None);
    }
}
