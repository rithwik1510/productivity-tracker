-- Schema v4: minimized = 1 marks waiting credit (ADR-0007; added for ADR-0006, which it
-- replaced): a tracked Productive app counted while minimized, because the foreground wasn't
-- Productive. Apps only. Rows never overlap, whatever the flag.

ALTER TABLE interval
  ADD COLUMN minimized INTEGER NOT NULL DEFAULT 0
  CHECK (minimized IN (0, 1) AND (minimized = 0 OR app_id IS NOT NULL));
