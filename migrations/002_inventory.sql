-- Schema v2: read approvals, scan generations, inventory, ownership, sessions,
-- cleanup plans with a durable action journal, local settings and update hints.
-- Executed in one immediate transaction after a consistent backup of v1 data.

ALTER TABLE resources ADD COLUMN acquisition TEXT NOT NULL DEFAULT 'registered'
    CHECK(acquisition IN ('registered','adopted','managed','discovered'));
ALTER TABLE resources ADD COLUMN parent_id INTEGER REFERENCES resources(id) ON DELETE RESTRICT;
ALTER TABLE resources ADD COLUMN purpose TEXT;
ALTER TABLE resources ADD COLUMN review_at INTEGER;
ALTER TABLE resources ADD COLUMN expendable INTEGER NOT NULL DEFAULT 0 CHECK(expendable IN (0,1));
ALTER TABLE resources ADD COLUMN last_activity_at INTEGER;
ALTER TABLE resources ADD COLUMN created_at INTEGER;
-- Identity of a directory mogumogu created itself; a new directory at the
-- same location never inherits decisions made for the old one.
ALTER TABLE resources ADD COLUMN identity TEXT;
ALTER TABLE resources ADD COLUMN measured_at INTEGER;
ALTER TABLE activity ADD COLUMN kind TEXT NOT NULL DEFAULT 'info';

CREATE TABLE IF NOT EXISTS scopes (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK(kind IN ('project','scoop','chocolatey','winget')),
    path TEXT NOT NULL,
    identity TEXT,
    status TEXT NOT NULL DEFAULT 'approved'
        CHECK(status IN ('approved','revoked','identity_changed','unavailable')),
    approved_at INTEGER NOT NULL,
    last_error TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS scopes_active_path ON scopes(kind, path) WHERE status <> 'revoked';

CREATE TABLE IF NOT EXISTS generations (
    id INTEGER PRIMARY KEY,
    scope_id INTEGER NOT NULL REFERENCES scopes(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK(state IN ('running','partial','complete','failed','superseded')),
    catalog_version TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    entries_seen INTEGER NOT NULL DEFAULT 0,
    files_read INTEGER NOT NULL DEFAULT 0,
    bytes_read INTEGER NOT NULL DEFAULT 0,
    item_count INTEGER NOT NULL DEFAULT 0,
    limit_reason TEXT,
    error_count INTEGER NOT NULL DEFAULT 0,
    error_summary TEXT
);
CREATE INDEX IF NOT EXISTS generations_scope_state ON generations(scope_id, state, id);

CREATE TABLE IF NOT EXISTS inventory_items (
    generation_id INTEGER NOT NULL REFERENCES generations(id) ON DELETE CASCADE,
    item_key TEXT NOT NULL,
    category TEXT NOT NULL
        CHECK(category IN ('package','manifest','environment','output','cache','software','ai','ai_reference')),
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT,
    install_state TEXT CHECK(install_state IS NULL OR install_state IN ('declared','resolved','installed')),
    source TEXT NOT NULL CHECK(source IN ('public','private','path','git','unknown')),
    source_host TEXT,
    rel_path TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    bytes INTEGER CHECK(bytes IS NULL OR bytes >= 0),
    PRIMARY KEY (generation_id, item_key)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS inventory_items_package ON inventory_items(ecosystem, name);

CREATE TABLE IF NOT EXISTS inventory_events (
    id INTEGER PRIMARY KEY,
    scope_id INTEGER NOT NULL REFERENCES scopes(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('baseline','added','removed','changed')),
    item_key TEXT,
    summary TEXT NOT NULL,
    from_generation INTEGER,
    to_generation INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS inventory_events_scope ON inventory_events(scope_id, id);

CREATE TABLE IF NOT EXISTS owner_links (
    resource_id INTEGER NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('explicit','inferred')),
    evidence TEXT NOT NULL,
    PRIMARY KEY (resource_id, project_id, kind)
);

CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY,
    scratch_id INTEGER NOT NULL REFERENCES resources(id) ON DELETE RESTRICT,
    purpose TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('starting','running','completion_requested',
        'completed_verified','interrupted_unknown','released_after_review')),
    origin TEXT NOT NULL CHECK(origin IN ('managed','registered','cooperative')),
    pid INTEGER,
    process_created INTEGER,
    job_name TEXT,
    started_at INTEGER NOT NULL,
    heartbeat_at INTEGER,
    ended_at INTEGER,
    exit_code INTEGER,
    remaining_processes INTEGER,
    note TEXT
);
CREATE INDEX IF NOT EXISTS sessions_scratch ON sessions(scratch_id);

-- Explicit, identity-bound test roots. Until the independent review (G5)
-- cleanup can only run inside such a root.
CREATE TABLE IF NOT EXISTS disposable_roots (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    identity TEXT NOT NULL,
    registered_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS cleanup_plans (
    id INTEGER PRIMARY KEY,
    state TEXT NOT NULL CHECK(state IN ('draft','approved','revalidating','executing','completed',
        'partial','failed','cancelled','invalidated','recovery_required')),
    created_at INTEGER NOT NULL,
    approved_at INTEGER,
    finished_at INTEGER,
    rule_version TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    note TEXT
);

CREATE TABLE IF NOT EXISTS cleanup_targets (
    id INTEGER PRIMARY KEY,
    plan_id INTEGER NOT NULL REFERENCES cleanup_plans(id) ON DELETE RESTRICT,
    resource_id INTEGER NOT NULL REFERENCES resources(id) ON DELETE RESTRICT,
    path TEXT NOT NULL,
    identity TEXT NOT NULL,
    entries INTEGER NOT NULL CHECK(entries >= 0),
    bytes INTEGER NOT NULL CHECK(bytes >= 0),
    blockers TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS cleanup_manifest (
    target_id INTEGER NOT NULL REFERENCES cleanup_targets(id) ON DELETE RESTRICT,
    rel_path TEXT NOT NULL,
    identity TEXT NOT NULL,
    is_dir INTEGER NOT NULL CHECK(is_dir IN (0,1)),
    size INTEGER NOT NULL,
    modified INTEGER NOT NULL,
    depth INTEGER NOT NULL,
    PRIMARY KEY (target_id, rel_path)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS action_journal (
    id INTEGER PRIMARY KEY,
    plan_id INTEGER NOT NULL REFERENCES cleanup_plans(id) ON DELETE RESTRICT,
    target_id INTEGER NOT NULL REFERENCES cleanup_targets(id) ON DELETE RESTRICT,
    rel_path TEXT NOT NULL,
    identity TEXT NOT NULL,
    is_dir INTEGER NOT NULL CHECK(is_dir IN (0,1)),
    depth INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('planned','started','done','failed','reconciled_missing','pending','blocked')),
    error TEXT,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS action_journal_plan ON action_journal(plan_id, state);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS update_checks (
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('current','update_available','offline','stale','unknown',
        'unsupported','private_skipped','not_approved')),
    installed_version TEXT,
    latest_version TEXT,
    source_host TEXT,
    checked_at INTEGER NOT NULL,
    failures INTEGER NOT NULL DEFAULT 0,
    next_allowed_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (ecosystem, name)
);
