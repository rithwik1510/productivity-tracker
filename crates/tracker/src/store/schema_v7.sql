-- Schema v7 (issue 19): "Check for updates automatically" (ADR-0009). On for every install,
-- existing ones included, and for the new settings row Delete all inserts.

ALTER TABLE settings
  ADD COLUMN auto_update INTEGER NOT NULL DEFAULT 1 CHECK (auto_update IN (0, 1));
