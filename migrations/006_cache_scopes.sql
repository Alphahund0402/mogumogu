-- Schema v6: package manager caches as their own approval realm. Additive
-- only, like v4: the scopes table is not rebuilt, existing kinds, profiles
-- and realms keep their meaning. A cache is only measured, never listed or
-- changed, and its approval never covers anything else at the same path.
ALTER TABLE scopes ADD COLUMN cache TEXT NOT NULL DEFAULT ''
    CHECK(cache IN ('','npm-cache','pip-cache','uv-cache','cargo-registry','gradle-caches','maven-repository',
                    'pnpm-store','yarn-cache'));
DROP INDEX scopes_active_path;
CREATE UNIQUE INDEX scopes_active_path ON scopes(kind, profile, realm, cache, path) WHERE status <> 'revoked';
