-- Schema v4: user-level configuration folders as their own approval realm.
-- Additive only: the referenced scopes table is not rebuilt, existing kinds
-- and profiles keep their meaning. A user folder never shares an approval
-- with a project folder at the same path.
ALTER TABLE scopes ADD COLUMN realm TEXT NOT NULL DEFAULT '' CHECK(realm IN ('','user'));
DROP INDEX scopes_active_path;
CREATE UNIQUE INDEX scopes_active_path ON scopes(kind, profile, realm, path) WHERE status <> 'revoked';
