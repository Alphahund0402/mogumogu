-- Metadata-only disk analysis has its own approval. Existing project, search,
-- client and cache approvals retain their meaning, including at the same path.
ALTER TABLE scopes ADD COLUMN analysis INTEGER NOT NULL DEFAULT 0 CHECK(analysis IN (0,1));
DROP INDEX scopes_active_path;
CREATE UNIQUE INDEX scopes_active_path ON scopes(kind, profile, realm, cache, analysis, path)
    WHERE status <> 'revoked';
