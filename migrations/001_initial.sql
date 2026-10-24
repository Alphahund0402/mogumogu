-- Executed in one immediate transaction; SQLite PRAGMA user_version is set by Rust.
CREATE TABLE IF NOT EXISTS app_meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 100),
    path TEXT NOT NULL UNIQUE,
    ecosystem TEXT NOT NULL DEFAULT 'Unbekannt',
    state TEXT NOT NULL DEFAULT 'unknown' CHECK(state IN ('unknown','observed','review','protected')),
    evidence TEXT NOT NULL DEFAULT 'Registriert; noch nicht untersucht.',
    bytes INTEGER CHECK(bytes IS NULL OR bytes >= 0),
    packages INTEGER CHECK(packages IS NULL OR packages >= 0),
    protected INTEGER NOT NULL DEFAULT 1 CHECK(protected IN (0,1)),
    origin TEXT NOT NULL CHECK(origin IN ('demo','registered')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE TABLE IF NOT EXISTS resources (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    bytes INTEGER CHECK(bytes IS NULL OR bytes >= 0),
    state TEXT NOT NULL DEFAULT 'unknown' CHECK(state IN ('unknown','observed','review','protected')),
    evidence TEXT NOT NULL,
    protected INTEGER NOT NULL DEFAULT 1 CHECK(protected IN (0,1)),
    origin TEXT NOT NULL CHECK(origin IN ('demo','registered'))
);
CREATE INDEX IF NOT EXISTS resources_project ON resources(project_id);
CREATE TABLE IF NOT EXISTS ai_configs (
    id INTEGER PRIMARY KEY,
    client TEXT NOT NULL,
    label TEXT NOT NULL,
    path TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'protected'
);
CREATE TABLE IF NOT EXISTS activity (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    detail TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
