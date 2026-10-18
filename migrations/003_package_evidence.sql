-- Version-specific source-bound update evidence. Old latest-only results
-- cannot establish lifecycle status and must be checked again.
ALTER TABLE update_checks RENAME TO update_checks_v2;
CREATE TABLE update_checks (
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('public','private','path','git','unknown')),
    source_host TEXT NOT NULL DEFAULT '',
    installed_version TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('current','update_available','offline','stale','unknown',
        'unsupported','private_skipped','not_approved')),
    latest_version TEXT,
    checked_at INTEGER NOT NULL,
    failures INTEGER NOT NULL DEFAULT 0,
    next_allowed_at INTEGER NOT NULL DEFAULT 0,
    lifecycle TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (ecosystem, name, source, source_host, installed_version)
) WITHOUT ROWID;
-- The pre-migration database is backed up through SQLite's backup API.
DROP TABLE update_checks_v2;

-- Only selected, bounded metadata is stored; never raw configuration.
ALTER TABLE inventory_items ADD COLUMN evidence TEXT NOT NULL DEFAULT '{}';
CREATE TABLE package_decisions (
    package_key TEXT PRIMARY KEY NOT NULL CHECK(length(package_key)=64 AND package_key NOT GLOB '*[^0-9a-f]*'),
    retained INTEGER NOT NULL CHECK(retained IN (0,1)),
    updated_at INTEGER NOT NULL
) WITHOUT ROWID;

-- New scopes use the same identity-bound, handle-relative read boundary.
-- The original kind column stays intact; profiles refine the approved
-- selectors without rebuilding the referenced scope table.
ALTER TABLE scopes ADD COLUMN profile TEXT NOT NULL DEFAULT ''
    CHECK(profile IN ('','projects','npm-global','uv-tools','python','cargo-tools','nuget-cache'));
DROP INDEX scopes_active_path;
CREATE UNIQUE INDEX scopes_active_path ON scopes(kind, profile, path) WHERE status <> 'revoked';
