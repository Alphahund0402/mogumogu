-- Schema v9: agent runs that cooperating harnesses register on request,
-- approvals of local Docker contexts for a read-only inventory, and a bounded
-- history of tidy-up reports. None of it can remove anything: a registered
-- run and its artifacts only add provenance and protection.
CREATE TABLE agent_runs (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    harness TEXT NOT NULL CHECK(length(harness) BETWEEN 1 AND 100),
    purpose TEXT,
    state TEXT NOT NULL CHECK(state IN ('running','paused','succeeded','failed','cancelled')),
    started_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    finished_at INTEGER,
    note TEXT
);
CREATE INDEX agent_runs_project ON agent_runs(project_id, id);

-- A file or folder a run made (real place, as for kept folders) or a Docker
-- object (`location` empty, `docker_*` set). Every artifact is protected.
CREATE TABLE run_artifacts (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES agent_runs(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL
        CHECK(kind IN ('workspace','scratch','diagnostic','test','code','state','output','docker')),
    location TEXT NOT NULL DEFAULT '',
    docker_context TEXT,
    docker_type TEXT CHECK(docker_type IS NULL OR docker_type IN ('container','image','volume','network')),
    docker_id TEXT,
    purpose TEXT,
    created_at INTEGER NOT NULL,
    CHECK((kind = 'docker') = (docker_id IS NOT NULL)),
    CHECK(kind = 'docker' OR length(location) BETWEEN 4 AND 4096)
);
CREATE INDEX run_artifacts_run ON run_artifacts(run_id);

-- A local Docker context, bound to the named pipe it named when approved.
CREATE TABLE docker_contexts (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE CHECK(length(name) BETWEEN 1 AND 100),
    pipe TEXT NOT NULL CHECK(length(pipe) BETWEEN 1 AND 128),
    approved_at INTEGER NOT NULL
);

-- One measured tidy-up report per row, the newest 30 per project.
CREATE TABLE tidy_history (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    checked_at INTEGER NOT NULL,
    complete INTEGER NOT NULL CHECK(complete IN (0,1)),
    total INTEGER CHECK(total >= 0),
    regenerable INTEGER NOT NULL CHECK(regenerable >= 0),
    review INTEGER NOT NULL CHECK(review >= 0),
    vcs INTEGER NOT NULL CHECK(vcs >= 0),
    test_data INTEGER NOT NULL CHECK(test_data >= 0),
    agent INTEGER NOT NULL CHECK(agent >= 0),
    -- The largest entries: [[rel_path, class, bytes], …], at most 20.
    items TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX tidy_history_project ON tidy_history(project_id, checked_at);
