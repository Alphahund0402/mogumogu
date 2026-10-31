-- Schema v10: databases registered for monitoring (size and growth from
-- metadata only, never their content), and the person's release of a
-- finished run's protection. A monitored database is held: it never enters
-- a cleanup plan. Nothing here removes data.
ALTER TABLE agent_runs ADD COLUMN released_at INTEGER;

CREATE TABLE monitored_databases (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 100),
    engine TEXT NOT NULL
        CHECK(engine IN ('sqlite','duckdb','postgres','mysql','mongodb','redis','other')),
    -- A data folder, or a database file with `file_name` set; empty for a
    -- Docker volume.
    location TEXT NOT NULL DEFAULT '',
    -- The real place when registered, for holds under every spelling.
    real_location TEXT,
    -- Identity of the folder (of the file's folder for a file).
    identity TEXT,
    file_name TEXT,
    docker_context TEXT,
    docker_volume TEXT,
    project_id INTEGER REFERENCES projects(id) ON DELETE RESTRICT,
    registered_at INTEGER NOT NULL,
    CHECK((docker_volume IS NULL) = (length(location) > 0)),
    CHECK(docker_volume IS NULL OR docker_context IS NOT NULL)
);

-- One measurement per row, the newest 100 per database.
CREATE TABLE database_sizes (
    id INTEGER PRIMARY KEY,
    database_id INTEGER NOT NULL REFERENCES monitored_databases(id) ON DELETE CASCADE,
    measured_at INTEGER NOT NULL,
    bytes INTEGER CHECK(bytes >= 0),
    files INTEGER CHECK(files >= 0),
    modified INTEGER,
    note TEXT
);
CREATE INDEX database_sizes_database ON database_sizes(database_id, id);
