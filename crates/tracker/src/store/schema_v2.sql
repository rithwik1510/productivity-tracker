-- Schema v2: the window Appearance (issue 07). Existing installs keep the glass look they had.

ALTER TABLE settings
  ADD COLUMN appearance TEXT NOT NULL DEFAULT 'glass' CHECK (appearance IN ('glass', 'solid'));
