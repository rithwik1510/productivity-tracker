-- Schema v5 (issue 13): the Tracking settings and Pause. Away after 5 minutes without evidence and
-- not paused, for every install.

ALTER TABLE settings
  ADD COLUMN away_after_minutes INTEGER NOT NULL DEFAULT 5 CHECK (away_after_minutes IN (2, 5, 10, 15));

-- Waiting credit is off for a new settings row: a fresh install (v1 inserts the row with onboarded
-- = 0 just before this runs) and Delete all, which inserts a new row. An install that finished
-- onboarding before v5 has always had waiting credit, so it keeps it on (issue 15).
ALTER TABLE settings
  ADD COLUMN waiting_credit INTEGER NOT NULL DEFAULT 0 CHECK (waiting_credit IN (0, 1));
UPDATE settings SET waiting_credit = 1 WHERE onboarded = 1;

-- Nothing counts before this instant (UTC epoch ms), as if Away. NULL, or an instant already past,
-- means not paused, so an expired pause needs no clean-up.
ALTER TABLE settings ADD COLUMN paused_until_ms INTEGER;
