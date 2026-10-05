//! Cached update hints with backoff state.
use super::Repository;
use crate::clock::Timestamp;
use crate::domain::UpdateStatus;
use crate::{Result, limits};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateRecord {
    pub ecosystem: String,
    pub name: String,
    pub status: UpdateStatus,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub source_host: Option<String>,
    pub checked_at: Timestamp,
    pub failures: i64,
    pub next_allowed_at: Timestamp,
}

impl Repository {
    pub fn update_record(&mut self, ecosystem: &str, name: &str) -> Result<Option<UpdateRecord>> {
        self.read(|tx| {
            Ok(tx
                .query_row(
                    "SELECT ecosystem, name, status, installed_version, latest_version, source_host, checked_at,
                            failures, next_allowed_at FROM update_checks WHERE ecosystem = ?1 AND name = ?2",
                    params![ecosystem, name],
                    map,
                )
                .optional()?)
        })
    }

    pub fn update_records(&mut self) -> Result<Vec<UpdateRecord>> {
        self.read(|tx| {
            let mut statement = tx.prepare(&format!(
                "SELECT ecosystem, name, status, installed_version, latest_version, source_host, checked_at,
                        failures, next_allowed_at FROM update_checks ORDER BY ecosystem, name LIMIT {}",
                limits::UI_PAGE * 10
            ))?;
            let rows = statement.query_map([], map)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn store_update(&mut self, record: &UpdateRecord) -> Result<()> {
        self.require_local()?;
        self.write(|tx| {
            tx.execute(
                "INSERT INTO update_checks(ecosystem, name, status, installed_version, latest_version, source_host,
                                           checked_at, failures, next_allowed_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(ecosystem, name) DO UPDATE SET status = excluded.status,
                   installed_version = excluded.installed_version, latest_version = excluded.latest_version,
                   source_host = excluded.source_host, checked_at = excluded.checked_at,
                   failures = excluded.failures, next_allowed_at = excluded.next_allowed_at",
                params![
                    record.ecosystem,
                    record.name,
                    record.status,
                    record.installed_version,
                    record.latest_version,
                    record.source_host,
                    record.checked_at,
                    record.failures,
                    record.next_allowed_at
                ],
            )?;
            Ok(())
        })
    }
}

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<UpdateRecord> {
    Ok(UpdateRecord {
        ecosystem: r.get(0)?,
        name: r.get(1)?,
        status: r.get(2)?,
        installed_version: r.get(3)?,
        latest_version: r.get(4)?,
        source_host: r.get(5)?,
        checked_at: r.get(6)?,
        failures: r.get(7)?,
        next_allowed_at: r.get(8)?,
    })
}
