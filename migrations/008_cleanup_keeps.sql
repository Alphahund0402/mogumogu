-- Schema v8: folders a person keeps out of every cleanup plan, and the free
-- space measured on the project's drive around an executed plan. A keep only
-- ever takes folders out; it never makes anything recoverable.
CREATE TABLE cleanup_keeps (
    id INTEGER PRIMARY KEY,
    -- The project it was set in, for display; it counts for every project.
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    -- `/`-separated below that project's root, as entered.
    rel_path TEXT NOT NULL CHECK(length(rel_path) BETWEEN 1 AND 1024),
    -- The folder's real place as the drive names it (long names, its own
    -- spelling), so it matches every registration that reaches the folder.
    -- Uniqueness is checked without case in the application: SQLite's
    -- lower() folds ASCII only.
    location TEXT NOT NULL CHECK(length(location) BETWEEN 4 AND 4096),
    created_at INTEGER NOT NULL
);
CREATE INDEX cleanup_keeps_project ON cleanup_keeps(project_id);

-- Free bytes of the drive right before the first and after the last move.
-- The recycle bin returns space only when it is emptied.
ALTER TABLE cleanup_plans ADD COLUMN free_before INTEGER CHECK(free_before >= 0);
ALTER TABLE cleanup_plans ADD COLUMN free_after INTEGER CHECK(free_after >= 0);
