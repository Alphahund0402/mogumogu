-- Schema v12: a protected scratchpad may let generated folders inside it into
-- a recycle-bin plan (docs/LEBENSZYKLUS.md, decision E4, option B). Off by
-- default and after the upgrade, switched on only in the dashboard, and only
-- for a protected scratchpad. Only recoverable folders strictly inside
-- qualify; the scratchpad itself, folders holding it and everything under
-- "Prüfen" stay protected.
ALTER TABLE resources ADD COLUMN plans_inside INTEGER NOT NULL DEFAULT 0
    CHECK(plans_inside IN (0,1) AND (plans_inside = 0 OR (kind = 'scratchpad' AND protected = 1)));
