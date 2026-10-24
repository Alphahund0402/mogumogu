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
    "category, ecosystem, name, version, install_state, source, source_host, rel_path, detail, bytes, evidence";

/// Projects and scratchpads before what lies in them: a drive search finds
/// far more environments and caches than one dashboard page holds. Members
/// of a workspace (context is the outer project) follow the outer projects.
const DISCOVERY_ORDER: &str = "CASE json_extract(evidence, '$.discovery.kind')
    WHEN 'project' THEN 0 WHEN 'scratchpad' THEN 1 WHEN 'notebook' THEN 2 WHEN 'script' THEN 3
    WHEN 'test_data' THEN 4 WHEN 'database' THEN 5 ELSE 6 END,
    json_extract(evidence, '$.discovery.confidence') DESC,
    COALESCE(json_extract(evidence, '$.context'), '') <> rel_path";

fn map_item(row: &Row<'_>, offset: usize) -> rusqlite::Result<InventoryItem> {
    Ok(InventoryItem {
        category: row.get(offset)?,
        ecosystem: row.get(offset + 1)?,
        name: row.get(offset + 2)?,
        version: row.get(offset + 3)?,
        install_state: row.get(offset + 4)?,
        source: row.get(offset + 5)?,
        source_host: row.get(offset + 6)?,
        rel_path: row.get(offset + 7)?,
        detail: row.get(offset + 8)?,
        bytes: row.get(offset + 9)?,
        evidence: serde_json::from_str(&row.get::<_, String>(offset + 10)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(offset + 10, rusqlite::types::Type::Text, Box::new(error))
        })?,
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
        self.query(
            &format!(
                "SELECT {ITEM_COLUMNS} FROM inventory_items WHERE generation_id =
                   (SELECT id FROM generations WHERE scope_id = ?1 AND state = 'complete' ORDER BY id DESC LIMIT 1)
                 ORDER BY item_key"
            ),
            [scope_id],
            |r| map_item(r, 0),
        )
    }

    /// Filename-only discoveries for a bounded dashboard page. Package and
    /// AI rows are filtered in SQLite rather than materialized and discarded.
    pub fn published_discoveries(&mut self, scope_id: i64, limit: usize) -> Result<Vec<InventoryItem>> {
        self.query(
            &format!(
                "SELECT {ITEM_COLUMNS} FROM inventory_items WHERE generation_id =
                   (SELECT id FROM generations WHERE scope_id = ?1 AND state = 'complete' ORDER BY id DESC LIMIT 1)
                   AND ecosystem = 'discovery' ORDER BY {DISCOVERY_ORDER}, item_key LIMIT ?2"
            ),
            params![scope_id, limit.min(limits::UI_PAGE as usize) as i64],
            |row| map_item(row, 0),
        )
    }

    /// Outer projects of the last complete pass that the names make certain:
    /// folders with an exact path and no project above them.
    pub fn published_root_projects(&mut self, scope_id: i64, limit: usize) -> Result<Vec<InventoryItem>> {
        self.query(
            &format!(
                "SELECT {ITEM_COLUMNS} FROM inventory_items WHERE generation_id =
                   (SELECT id FROM generations WHERE scope_id = ?1 AND state = 'complete' ORDER BY id DESC LIMIT 1)
                   AND ecosystem = 'discovery'
                   AND json_extract(evidence, '$.discovery.kind') = 'project'
                   AND json_extract(evidence, '$.discovery.confidence') = 3
                   AND json_extract(evidence, '$.discovery.is_dir') = 1
                   AND json_extract(evidence, '$.discovery.path_exact') = 1
                   AND COALESCE(json_extract(evidence, '$.context'), '') = rel_path
                 ORDER BY item_key LIMIT ?2"
            ),
            params![scope_id, limit.min(limits::UI_PAGE as usize) as i64],
            |row| map_item(row, 0),
        )
    }

    /// Exact published discovery for metadata-only registration/adoption.
    pub fn published_discovery(&mut self, scope_id: i64, rel: &str) -> Result<Option<InventoryItem>> {
        Ok(self
            .query(
                &format!(
                    "SELECT {ITEM_COLUMNS} FROM inventory_items WHERE generation_id =
                       (SELECT id FROM generations WHERE scope_id = ?1 AND state = 'complete' ORDER BY id DESC LIMIT 1)
                       AND ecosystem = 'discovery' AND rel_path = ?2 ORDER BY item_key LIMIT 1"
                ),
                params![scope_id, rel],
                |row| map_item(row, 0),
            )?
            .into_iter()
            .next())
    }

    /// Search all published development metadata, independently of the
    /// dashboard page cap, without loading unrelated package or AI rows.
    pub fn search_discoveries(&mut self, needle: &str, limit: usize) -> Result<Vec<(i64, InventoryItem)>> {
        let pattern = format!("%{}%", needle.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        self.query(
            &format!(
                "SELECT s.id, {} FROM scopes s
                 CROSS JOIN generations g ON g.scope_id = s.id
                 CROSS JOIN inventory_items i ON i.generation_id = g.id
                 WHERE g.state = 'complete' AND s.status = 'approved' AND s.kind = 'project'
                   AND s.profile IN ('', 'projects') AND s.realm = '' AND s.cache = ''
                   AND i.ecosystem = 'discovery'
                   AND (lower(i.name) LIKE ?1 ESCAPE '\\' OR lower(i.rel_path) LIKE ?1 ESCAPE '\\')
                 ORDER BY s.id, i.item_key LIMIT ?2",
                ITEM_COLUMNS.split(", ").map(|c| format!("i.{c}")).collect::<Vec<_>>().join(", ")
            ),
            params![pattern, limit.min(limits::UI_PAGE as usize) as i64],
            |row| Ok((row.get(0)?, map_item(row, 1)?)),
        )
    }

    /// One published instruction/skill candidate for an on-demand content view.
    /// Raw content is never stored here; the caller still checks catalog and
    /// approval identity before opening the file.
    pub fn published_ai_artifact(&mut self, scope_id: i64, rel_path: &str) -> Result<Option<InventoryItem>> {
        Ok(self
            .query(
                &format!(
                    "SELECT {ITEM_COLUMNS} FROM inventory_items WHERE generation_id =
                       (SELECT id FROM generations WHERE scope_id = ?1 AND state = 'complete' ORDER BY id DESC LIMIT 1)
                       AND category = ?3 AND rel_path = ?2 ORDER BY item_key LIMIT 1"
                ),
                params![scope_id, rel_path, ItemCategory::AiArtifact],
                |row| map_item(row, 0),
            )?
            .into_iter()
            .next())
    }

    /// Published items of all approved scopes, optionally by category.
    pub fn published_items_all(&mut self, category: Option<ItemCategory>, limit: i64) -> Result<Vec<PublishedItem>> {
        self.query(
            &format!(
                "SELECT s.id, s.project_id, {} FROM inventory_items i
                 JOIN generations g ON g.id = i.generation_id JOIN scopes s ON s.id = g.scope_id
                 WHERE g.state = 'complete' AND s.status = 'approved' AND (?1 IS NULL OR i.category = ?1)
                 ORDER BY s.id, i.item_key LIMIT ?2",
                ITEM_COLUMNS.split(", ").map(|c| format!("i.{c}")).collect::<Vec<_>>().join(", ")
            ),
            params![category, limit],
            |r| Ok(PublishedItem { scope_id: r.get(0)?, project_id: r.get(1)?, item: map_item(r, 2)? }),
        )
    }

    /// Package names of the published inventory containing `needle`
    /// (lower case), with their number of versions. Declarations are left out
    /// like in the package review; the search never reads files.
    pub fn search_packages(&mut self, needle: &str, limit: usize) -> Result<Vec<(String, String, i64)>> {
        let pattern = format!("%{}%", needle.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        self.query(
            "SELECT i.ecosystem, i.name, COUNT(DISTINCT IFNULL(i.version, ''))
             FROM scopes s CROSS JOIN generations g ON g.scope_id = s.id
                  CROSS JOIN inventory_items i ON i.generation_id = g.id
             WHERE g.state = 'complete' AND s.status = 'approved'
               AND (i.category = 'package' OR (i.category = 'cache' AND i.ecosystem = 'nuget'))
               AND IFNULL(i.install_state, '') <> 'declared'
               AND lower(i.name) LIKE ?1 ESCAPE '\\'
             GROUP BY i.ecosystem, i.name ORDER BY i.name LIMIT ?2",
            params![pattern, limit as i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    }

    /// `CROSS JOIN` keeps the loop on published generations; otherwise SQLite
    /// walks the package index over every kept generation.
    pub fn ecosystem_counts(&mut self) -> Result<Vec<EcosystemCount>> {
        self.query(
            "SELECT i.ecosystem,
                    SUM(i.install_state = 'declared'), SUM(i.install_state = 'resolved'), SUM(i.install_state = 'installed')
                 FROM scopes s CROSS JOIN generations g ON g.scope_id = s.id
                      CROSS JOIN inventory_items i ON i.generation_id = g.id
                 WHERE g.state = 'complete' AND s.status = 'approved' AND i.category IN ('package', 'software')
                 GROUP BY i.ecosystem ORDER BY i.ecosystem",
            [],
            |r| {
                Ok(EcosystemCount {
                    ecosystem: r.get(0)?,
                    declared: r.get(1)?,
                    resolved: r.get(2)?,
                    installed: r.get(3)?,
                })
            },
        )
    }

    pub fn inventory_events(&mut self, limit: i64) -> Result<Vec<InventoryEvent>> {
        self.query(
            "SELECT id, scope_id, kind, summary, created_at FROM inventory_events ORDER BY id DESC LIMIT ?1",
            [limit],
            |r| {
                Ok(InventoryEvent {
                    id: r.get(0)?,
                    scope_id: r.get(1)?,
                    kind: r.get(2)?,
                    summary: r.get(3)?,
                    created_at: r.get(4)?,
                })
            },
        )
    }
}

fn insert_items(tx: &Transaction<'_>, generation_id: i64, items: &[InventoryItem]) -> Result<usize> {
    let mut statement = tx.prepare(
        "INSERT OR IGNORE INTO inventory_items(generation_id, item_key, category, ecosystem, name, version,
            install_state, source, source_host, rel_path, detail, bytes, evidence)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
            item.bytes,
            serde_json::to_string(&item.evidence)?
        ])?;
    }
    Ok(inserted)
}

fn keyed(tx: &Transaction<'_>, generation_id: i64) -> Result<HashMap<String, (String, Option<String>)>> {
    let mut statement = tx.prepare(
        "SELECT item_key, name,
                CASE WHEN ecosystem = 'discovery' THEN 'metadata:' || IFNULL(bytes, -1) || ':' || evidence
                     ELSE version END
         FROM inventory_items WHERE generation_id = ?1",
    )?;
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
