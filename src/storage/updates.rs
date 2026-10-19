//! Version-specific, source-bound update/lifecycle evidence and local retention.
use super::Repository;
use crate::clock::Timestamp;
use crate::domain::{Lifecycle, PackageId, SourceClass, UpdateStatus};
use crate::{Result, limits, privacy};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateRecord {
    pub ecosystem: String,
    pub name: String,
    pub status: UpdateStatus,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub source: SourceClass,
    pub source_host: Option<String>,
    pub checked_at: Timestamp,
    pub failures: i64,
    pub next_allowed_at: Timestamp,
    pub lifecycle: Lifecycle,
}

impl UpdateRecord {
    pub fn id(&self) -> PackageId {
        PackageId {
            ecosystem: self.ecosystem.clone(),
            name: self.name.clone(),
            version: self.installed_version.clone().unwrap_or_default(),
            source: self.source,
            source_host: self.source_host.clone().unwrap_or_default(),
        }
    }
}

const COLUMNS: &str = "ecosystem, name, status, installed_version, latest_version, source_host,
    checked_at, failures, next_allowed_at, source, lifecycle";

impl Repository {
    pub fn update_record(&mut self, id: &PackageId) -> Result<Option<UpdateRecord>> {
        self.read(|tx| {
            Ok(tx
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM update_checks WHERE ecosystem=?1 AND name=?2
                AND source=?3 AND source_host=?4 AND installed_version=?5"
                    ),
                    params![id.ecosystem, id.name, id.source, id.source_host, id.version],
                    map,
                )
                .optional()?)
        })
    }

    pub fn update_records(&mut self) -> Result<Vec<UpdateRecord>> {
        self.query(
            &format!(
                "SELECT {COLUMNS} FROM update_checks ORDER BY checked_at DESC, ecosystem, name LIMIT {}",
                limits::UI_PAGE * 10
            ),
            [],
            map,
        )
    }

    pub fn update_cache(&mut self) -> Result<Vec<UpdateRecord>> {
        self.query(&format!("SELECT {COLUMNS} FROM update_checks ORDER BY checked_at DESC LIMIT 10000"), [], map)
    }

    pub fn store_update(&mut self, record: &UpdateRecord) -> Result<()> {
        self.require_local()?;
        let mut lifecycle = record.lifecycle.clone();
        lifecycle.reason = lifecycle.reason.as_deref().map(privacy::label);
        lifecycle.replacement = lifecycle.replacement.as_deref().map(privacy::label);
        lifecycle.project_status = lifecycle.project_status.as_deref().map(privacy::label);
        self.write(|tx| {
            tx.execute(
                "INSERT INTO update_checks(ecosystem,name,status,installed_version,latest_version,source_host,
                    checked_at,failures,next_allowed_at,source,lifecycle)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
                 ON CONFLICT(ecosystem,name,source,source_host,installed_version) DO UPDATE SET
                   status=excluded.status,latest_version=excluded.latest_version,checked_at=excluded.checked_at,
                   failures=excluded.failures,next_allowed_at=excluded.next_allowed_at,lifecycle=excluded.lifecycle",
                params![
                    record.ecosystem,
                    record.name,
                    record.status,
                    record.installed_version.as_deref().unwrap_or(""),
                    record.latest_version.as_deref().map(privacy::label),
                    record.source_host.as_deref().unwrap_or(""),
                    record.checked_at,
                    record.failures,
                    record.next_allowed_at,
                    record.source,
                    serde_json::to_string(&lifecycle)?
                ],
            )?;
            tx.execute(
                "DELETE FROM update_checks WHERE (ecosystem,name,source,source_host,installed_version) IN
                (SELECT ecosystem,name,source,source_host,installed_version FROM update_checks
                 ORDER BY checked_at DESC,ecosystem,name,source,source_host,installed_version LIMIT -1 OFFSET 10000)",
                [],
            )?;
            Ok(())
        })
    }

    pub fn retained_packages(&mut self) -> Result<Vec<String>> {
        self.query(
            "SELECT package_key FROM package_decisions WHERE retained=1 ORDER BY package_key LIMIT 10000",
            [],
            |r| r.get(0),
        )
    }

    pub fn retain_package(&mut self, key: &str, retained: bool) -> Result<()> {
        self.require_local()?;
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(crate::Error::invalid("Paket-Schlüssel aus dem Paketbericht verwenden."));
        }
        let key = key.to_ascii_lowercase();
        let now = self.now();
        self.write(|tx| {
            if retained {
                let count: i64 = tx.query_row("SELECT COUNT(*) FROM package_decisions", [], |r| r.get(0))?;
                if count >= 10000 {
                    return Err(crate::Error::blocked("Limit für Behalten-Entscheidungen erreicht."));
                }
                tx.execute(
                    "INSERT INTO package_decisions(package_key,retained,updated_at) VALUES(?1,1,?2)
                    ON CONFLICT(package_key) DO UPDATE SET retained=1,updated_at=excluded.updated_at",
                    params![key, now],
                )?;
            } else {
                tx.execute("DELETE FROM package_decisions WHERE package_key=?1", [key])?;
            }
            Ok(())
        })
    }
}

fn map(r: &Row<'_>) -> rusqlite::Result<UpdateRecord> {
    let lifecycle = serde_json::from_str(&r.get::<_, String>(10)?)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(e)))?;
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
        source: r.get(9)?,
        lifecycle,
    })
}
