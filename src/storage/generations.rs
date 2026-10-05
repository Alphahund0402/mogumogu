//! Scan generations (F-08). A pass writes into a new generation; only a
//! *complete* generation is published. Removal events are derived only
//! between two complete generations with the same catalog version, so a
//! partial or failed pass can never report a false removal.
use super::Repository;
use super::activity::log_in;
use crate::domain::{EcosystemCount, GenerationState, InventoryEvent, InventoryEventKind, InventoryItem, ItemCategory};
use crate::{Error, Result, limits, privacy};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use std::collections::HashMap;

/// Outcome of one discovery pass, ready to be stored.
#[derive(Clone, Debug, Default)]
pub struct GenerationResult {
    pub items: Vec<InventoryItem>,
    pub entries_seen: u64,
    pub files_read: u64,
    pub bytes_read: u64,
    pub limit_reason: Option<String>,
    /// Problems that make the pass incomplete (unreadable folders, files).
    pub errors: Vec<String>,
    /// Expected, safe omissions (links not followed, cloud placeholders).
    pub notes: Vec<String>,
    /// Fatal problem (root unavailable, identity changed): nothing usable.
    pub failed: bool,
}

impl GenerationResult {
    pub fn state(&self) -> GenerationState {
        if self.failed {
            GenerationState::Failed
        } else if self.limit_reason.is_some() || !self.errors.is_empty() {
            GenerationState::Partial
        } else {
            GenerationState::Complete
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublishReport {
    pub generation_id: i64,
    pub state: Option<GenerationState>,
    pub baseline: bool,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
}

const ITEM_COLUMNS: &str =
    "category, ecosystem, name, version, install_state, source, source_host, rel_path, detail, bytes";

fn map_item(row: &Row<'_>) -> rusqlite::Result<InventoryItem> {
    Ok(InventoryItem {
        category: row.get(0)?,
        ecosystem: row.get(1)?,
        name: row.get(2)?,
        version: row.get(3)?,
        install_state: row.get(4)?,
        source: row.get(5)?,
        source_host: row.get(6)?,
        rel_path: row.get(7)?,
        detail: row.get(8)?,
        bytes: row.get(9)?,
    })
}

/// Published item with its scope and project context.
#[derive(Clone, Debug)]
pub struct PublishedItem {
    pub scope_id: i64,
    pub project_id: Option<i64>,
    pub item: InventoryItem,
}

impl Repository {
    /// Persists the start of a pass *before* reading, so a crash leaves a
    /// visible `running` generation that is marked failed on restart.
    pub fn begin_generation(&mut self, scope_id: i64, catalog_version: &str) -> Result<i64> {
        self.require_local()?;
        let now = self.now();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO generations(scope_id, state, catalog_version, started_at) VALUES(?1, 'running', ?2, ?3)",
                params![scope_id, catalog_version, now],
            )?;
            Ok(tx.last_insert_rowid())
        })
    }

    pub fn finish_generation(&mut self, generation_id: i64, result: &GenerationResult) -> Result<PublishReport> {
        self.require_local()?;
        let now = self.now();
        let state = result.state();
        self.write(|tx| {
            let (scope_id, current, catalog): (i64, GenerationState, String) = tx
                .query_row(
                    "SELECT scope_id, state, catalog_version FROM generations WHERE id = ?1",
                    [generation_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| Error::not_found("Erfassungsgeneration nicht gefunden."))?;
            if !current.can_become(state) {
                return Err(Error::conflict("Erfassungsgeneration ist bereits abgeschlossen."));
            }
            let inserted = insert_items(tx, generation_id, &result.items)?;
            let summary: Vec<String> =
                result.errors.iter().take(5).chain(result.notes.iter().take(5)).cloned().collect();
            let errors = privacy::label(&summary.join(" | "));
            tx.execute(
                "UPDATE generations SET state = ?1, finished_at = ?2, entries_seen = ?3, files_read = ?4,
                   bytes_read = ?5, item_count = ?6, limit_reason = ?7, error_count = ?8, error_summary = ?9
                 WHERE id = ?10",
                params![
                    state,
                    now,
                    result.entries_seen as i64,
                    result.files_read as i64,
                    result.bytes_read as i64,
                    inserted as i64,
                    result.limit_reason.as_deref().map(privacy::label),
                    result.errors.len() as i64,
                    (!errors.is_empty()).then_some(errors),
                    generation_id
                ],
            )?;
            let mut report = PublishReport { generation_id, state: Some(state), ..Default::default() };
            if state == GenerationState::Complete {
                publish(tx, scope_id, generation_id, &catalog, now, &mut report)?;
            } else {
                let title =
                    if state == GenerationState::Partial { "Erfassung teilweise" } else { "Erfassung fehlgeschlagen" };
                log_in(
                    tx,
                    now,
                    "coverage",
                    title,
                    "Letzter vollständiger Stand bleibt sichtbar; keine Entfernungsmeldungen.",
                )?;
            }
            prune(tx, scope_id)?;
            Ok(report)
        })
    }

    pub fn published_items(&mut self, scope_id: i64) -> Result<Vec<InventoryItem>> {
        self.read(|tx| {
            let mut statement = tx.prepare(&format!(
                "SELECT {ITEM_COLUMNS} FROM inventory_items WHERE generation_id =
                   (SELECT id FROM generations WHERE scope_id = ?1 AND state = 'complete' ORDER BY id DESC LIMIT 1)
                 ORDER BY item_key"
            ))?;
            let rows = statement.query_map([scope_id], map_item)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Published items of all approved scopes, optionally by category.
    pub fn published_items_all(&mut self, category: Option<ItemCategory>, limit: i64) -> Result<Vec<PublishedItem>> {
        self.read(|tx| {
            let mut statement = tx.prepare(&format!(
                "SELECT s.id, s.project_id, {} FROM inventory_items i
                 JOIN generations g ON g.id = i.generation_id JOIN scopes s ON s.id = g.scope_id
                 WHERE g.state = 'complete' AND s.status = 'approved' AND (?1 IS NULL OR i.category = ?1)
                 ORDER BY s.id, i.item_key LIMIT ?2",
                ITEM_COLUMNS.split(", ").map(|c| format!("i.{c}")).collect::<Vec<_>>().join(", ")
            ))?;
            let rows = statement.query_map(params![category, limit], |r| {
                Ok(PublishedItem {
                    scope_id: r.get(0)?,
                    project_id: r.get(1)?,
                    item: InventoryItem {
                        category: r.get(2)?,
                        ecosystem: r.get(3)?,
                        name: r.get(4)?,
                        version: r.get(5)?,
                        install_state: r.get(6)?,
                        source: r.get(7)?,
                        source_host: r.get(8)?,
                        rel_path: r.get(9)?,
                        detail: r.get(10)?,
                        bytes: r.get(11)?,
                    },
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn ecosystem_counts(&mut self) -> Result<Vec<EcosystemCount>> {
        self.read(|tx| {
            let mut statement = tx.prepare(
                "SELECT i.ecosystem,
                    SUM(i.install_state = 'declared'), SUM(i.install_state = 'resolved'), SUM(i.install_state = 'installed'),
                    SUM(EXISTS(SELECT 1 FROM update_checks u WHERE u.ecosystem = i.ecosystem AND u.name = i.name
                               AND u.status = 'update_available'))
                 FROM inventory_items i JOIN generations g ON g.id = i.generation_id JOIN scopes s ON s.id = g.scope_id
                 WHERE g.state = 'complete' AND s.status = 'approved' AND i.category IN ('package', 'software')
                 GROUP BY i.ecosystem ORDER BY i.ecosystem",
            )?;
            let rows = statement.query_map([], |r| {
                Ok(EcosystemCount {
                    ecosystem: r.get(0)?,
                    declared: r.get(1)?,
                    resolved: r.get(2)?,
                    installed: r.get(3)?,
                    updates_available: r.get(4)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn inventory_events(&mut self, limit: i64) -> Result<Vec<InventoryEvent>> {
        self.read(|tx| {
            let mut statement = tx.prepare(
                "SELECT id, scope_id, kind, summary, created_at FROM inventory_events ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = statement.query_map([limit], |r| {
                Ok(InventoryEvent {
                    id: r.get(0)?,
                    scope_id: r.get(1)?,
                    kind: r.get(2)?,
                    summary: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }
}

fn insert_items(tx: &Transaction<'_>, generation_id: i64, items: &[InventoryItem]) -> Result<usize> {
    let mut statement = tx.prepare(
        "INSERT OR IGNORE INTO inventory_items(generation_id, item_key, category, ecosystem, name, version,
            install_state, source, source_host, rel_path, detail, bytes)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;
    let mut inserted = 0;
    for item in items.iter().map(InventoryItem::sanitized) {
        inserted += statement.execute(params![
            generation_id,
            item.key(),
            item.category,
            item.ecosystem,
            item.name,
            item.version,
            item.install_state,
            item.source,
            item.source_host,
            item.rel_path,
            item.detail,
            item.bytes
        ])?;
    }
    Ok(inserted)
}

fn keyed(tx: &Transaction<'_>, generation_id: i64) -> Result<HashMap<String, (String, Option<String>)>> {
    let mut statement = tx.prepare("SELECT item_key, name, version FROM inventory_items WHERE generation_id = ?1")?;
    let rows = statement.query_map([generation_id], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?;
    Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
}

fn sample(names: &[&String]) -> String {
    let shown: Vec<&str> = names.iter().take(5).map(|s| s.as_str()).collect();
    let more = names.len().saturating_sub(shown.len());
    if more > 0 { format!("{} (+{more})", shown.join(", ")) } else { shown.join(", ") }
}

fn publish(
    tx: &Transaction<'_>,
    scope_id: i64,
    generation_id: i64,
    catalog: &str,
    now: i64,
    report: &mut PublishReport,
) -> Result<()> {
    let previous: Option<(i64, String)> = tx
        .query_row(
            "SELECT id, catalog_version FROM generations WHERE scope_id = ?1 AND state = 'complete' AND id <> ?2
             ORDER BY id DESC LIMIT 1",
            params![scope_id, generation_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let event = |kind: InventoryEventKind, summary: String, from: Option<i64>| -> Result<()> {
        tx.execute(
            "INSERT INTO inventory_events(scope_id, kind, summary, from_generation, to_generation, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![scope_id, kind, privacy::label(&summary), from, generation_id, now],
        )?;
        Ok(())
    };
    match previous {
        Some((previous_id, previous_catalog)) if previous_catalog == catalog => {
            let old = keyed(tx, previous_id)?;
            let new = keyed(tx, generation_id)?;
            let added: Vec<&String> = new.iter().filter(|(k, _)| !old.contains_key(*k)).map(|(_, v)| &v.0).collect();
            let removed: Vec<&String> = old.iter().filter(|(k, _)| !new.contains_key(*k)).map(|(_, v)| &v.0).collect();
            let changed: Vec<&String> =
                new.iter().filter(|(k, v)| old.get(*k).is_some_and(|o| o.1 != v.1)).map(|(_, v)| &v.0).collect();
            report.added = added.len();
            report.removed = removed.len();
            report.changed = changed.len();
            for (kind, names, verb) in [
                (InventoryEventKind::Added, &added, "neu"),
                (InventoryEventKind::Removed, &removed, "nicht mehr vorhanden"),
                (InventoryEventKind::Changed, &changed, "geändert"),
            ] {
                if !names.is_empty() {
                    event(kind, format!("{} {verb}: {}", names.len(), sample(names)), Some(previous_id))?;
                }
            }
            if report.added + report.removed + report.changed > 0 {
                log_in(
                    tx,
                    now,
                    "inventory",
                    "Inventar geändert",
                    &format!("{} neu · {} entfernt · {} geändert", report.added, report.removed, report.changed),
                )?;
            }
        }
        previous => {
            report.baseline = true;
            let reason = if previous.is_some() { "Profilversion geändert" } else { "Erste vollständige Erfassung" };
            let count: i64 =
                tx.query_row("SELECT COUNT(*) FROM inventory_items WHERE generation_id = ?1", [generation_id], |r| {
                    r.get(0)
                })?;
            event(InventoryEventKind::Baseline, format!("{reason}: neue Vergleichsbasis mit {count} Einträgen"), None)?;
            log_in(
                tx,
                now,
                "inventory",
                "Vergleichsbasis erstellt",
                &format!("{count} Einträge · keine Einzelmeldungen"),
            )?;
        }
    }
    tx.execute(
        "UPDATE generations SET state = 'superseded' WHERE scope_id = ?1 AND state = 'complete' AND id <> ?2",
        params![scope_id, generation_id],
    )?;
    Ok(())
}

/// Bounded history: keep the newest generations and events per scope.
fn prune(tx: &Transaction<'_>, scope_id: i64) -> Result<()> {
    tx.execute(
        "DELETE FROM generations WHERE scope_id = ?1 AND state <> 'complete' AND state <> 'running'
           AND id NOT IN (SELECT id FROM generations WHERE scope_id = ?1 ORDER BY id DESC LIMIT ?2)",
        params![scope_id, limits::GENERATIONS_KEEP],
    )?;
    tx.execute(
        "DELETE FROM inventory_events WHERE scope_id = ?1
           AND id NOT IN (SELECT id FROM inventory_events WHERE scope_id = ?1 ORDER BY id DESC LIMIT ?2)",
        params![scope_id, limits::EVENTS_KEEP],
    )?;
    Ok(())
}
