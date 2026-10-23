-- Schema v3: the current stretch's trim, for crash recovery (issue 10). Every flush writes where the
-- stretch would end if tracking stopped then (the shutdown cutoff); ending the stretch deletes the
-- row. After a crash, recovery applies it, so idle minutes at the end of the stretch don't count.

CREATE TABLE pending_trim (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  first_id INTEGER NOT NULL,
  cutoff_ms INTEGER NOT NULL);
