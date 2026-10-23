-- Schema v1 (plan section 4 "Store"). All timestamps are UTC epoch ms.

CREATE TABLE app (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('exe', 'aumid')),
  key TEXT NOT NULL,
  name TEXT NOT NULL,
  -- NULL = not tracked; the row is kept so history keeps its name.
  category TEXT CHECK (category IN ('productive', 'entertainment')),
  UNIQUE (kind, key)
);

CREATE TABLE site (
  id INTEGER PRIMARY KEY,
  domain TEXT NOT NULL UNIQUE,
  keyword TEXT,
  category TEXT CHECK (category IN ('productive', 'entertainment'))
);

CREATE TABLE interval (
  id INTEGER PRIMARY KEY,
  app_id INTEGER REFERENCES app (id),
  site_id INTEGER REFERENCES site (id),
  -- The Category at the time (ADR-0005); aggregates read it from here, never from app/site.
  category TEXT NOT NULL CHECK (category IN ('productive', 'entertainment')),
  start_ms INTEGER NOT NULL,
  end_ms INTEGER NOT NULL,
  open INTEGER NOT NULL DEFAULT 0,
  CHECK ((app_id IS NULL) <> (site_id IS NULL)),
  CHECK (end_ms >= start_ms)
);

CREATE INDEX interval_start ON interval (start_ms);

CREATE TABLE settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  daily_goal_minutes INTEGER NOT NULL DEFAULT 360,
  onboarded INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO settings (id) VALUES (1);
