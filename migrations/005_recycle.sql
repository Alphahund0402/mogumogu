-- Schema v5: managed runs and sessions are gone; cleanup moves recoverable
-- folders of an approved project to the Windows recycle bin, one journaled
-- action per folder. The per-file deletion manifest and journal go with the
-- old executor; plans that never finished cannot run any more.
DROP TABLE action_journal;
DROP TABLE cleanup_manifest;
DROP TABLE cleanup_targets;
DROP TABLE sessions;

UPDATE cleanup_plans
SET state = 'invalidated',
    note = 'Alter Löschweg entfernt; bitte einen neuen Plan erstellen.',
    finished_at = COALESCE(finished_at, CAST(strftime('%s','now') AS INTEGER))
WHERE state IN ('draft','approved','revalidating','executing','recovery_required');

-- One folder per row, below the root of an approved project scope. `state`
-- is the durable journal: `started` is committed before the folder moves.
CREATE TABLE recycle_targets (
    id INTEGER PRIMARY KEY,
    plan_id INTEGER NOT NULL REFERENCES cleanup_plans(id) ON DELETE RESTRICT,
    scope_id INTEGER NOT NULL REFERENCES scopes(id) ON DELETE RESTRICT,
    rel_path TEXT NOT NULL CHECK(length(rel_path) > 0),
    identity TEXT NOT NULL,
    label TEXT NOT NULL,
    evidence TEXT NOT NULL,
    bytes INTEGER NOT NULL CHECK(bytes >= 0),
    blockers TEXT NOT NULL DEFAULT '[]',
    state TEXT NOT NULL DEFAULT 'planned'
        CHECK(state IN ('planned','started','done','failed','reconciled_missing','pending','blocked')),
    error TEXT,
    updated_at INTEGER NOT NULL,
    UNIQUE(plan_id, rel_path)
);
CREATE INDEX recycle_targets_plan ON recycle_targets(plan_id, state);
