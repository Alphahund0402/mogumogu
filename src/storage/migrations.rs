//! Ordered, append-only schema migrations. A newer unknown schema is refused
//! before this runs; an existing database is backed up before it changes.
use crate::clock::Timestamp;
use crate::{Error, Result};
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;
use std::time::Duration;

const MIGRATIONS: &[(i64, &str)] =
    &[(1, include_str!("../../migrations/001_initial.sql")), (2, include_str!("../../migrations/002_inventory.sql"))];

pub const SCHEMA_VERSION: i64 = MIGRATIONS[MIGRATIONS.len() - 1].0;

pub(super) fn migrate(connection: &mut Connection, current: i64, backups: Option<&Path>, now: Timestamp) -> Result<()> {
    if current == SCHEMA_VERSION {
        return Ok(());
    }
    if current > 0
        && let Some(directory) = backups
    {
        backup_before_migration(connection, directory, current, now)?;
    }
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(sql)
            .map_err(|e| Error::conflict(format!("Migration auf Schema {version} fehlgeschlagen: {e}")))?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

fn backup_before_migration(connection: &Connection, directory: &Path, version: i64, now: Timestamp) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    let target = directory.join(format!("before-schema-{}-{now}.sqlite3", version + 1));
    if target.exists() {
        return Err(Error::conflict("Sicherung vor Migration existiert bereits; Migration angehalten."));
    }
    let mut destination = Connection::open(&target)?;
    {
        let backup = rusqlite::backup::Backup::new(connection, &mut destination)?;
        backup.run_to_completion(256, Duration::from_millis(5), None)?;
    }
    let check: String = destination.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(Error::conflict("Sicherung vor Migration ist nicht konsistent; Migration angehalten."));
    }
    Ok(())
}
