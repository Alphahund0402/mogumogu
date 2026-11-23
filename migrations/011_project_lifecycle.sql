-- Schema v11: finishing a project as a decision of its own, separate from
-- cleaning up generated files (docs/LEBENSZYKLUS.md). One row per change,
-- the newest 50 per project. The newest row is the state; a project without
-- a row is active, so no project changes state with this upgrade. Finishing
-- changes no protection and nothing on disk.
CREATE TABLE project_lifecycle (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    state TEXT NOT NULL CHECK(state IN ('active','finished')),
    at INTEGER NOT NULL,
    -- The finishing report the person confirmed; none for a reactivation.
    fingerprint TEXT CHECK(fingerprint IS NULL OR length(fingerprint) = 64),
    CHECK((state = 'finished') = (fingerprint IS NOT NULL))
);
CREATE INDEX project_lifecycle_project ON project_lifecycle(project_id, id);
