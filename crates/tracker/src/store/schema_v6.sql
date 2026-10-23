-- Schema v6 (issue 18): "Counts while I watch" per app (ADR-0008). Only read while the app is
-- Productive, and never for a browser.

ALTER TABLE app ADD COLUMN watch INTEGER NOT NULL DEFAULT 0 CHECK (watch IN (0, 1));

-- On for the tracked agent apps that tracking them now would turn it on for: the same preset list
-- as `tracker_api::watched_by_default` (keep the two in step). GLOB is case-sensitive and treats
-- `_` literally; exe keys are lowercase.
UPDATE app SET watch = 1
WHERE category IS NOT NULL
  AND ((kind = 'aumid' AND (key GLOB 'Claude_*' OR key GLOB 'OpenAI.Codex_*' OR key GLOB 'OpenAI.ChatGPT*'))
    OR (kind = 'exe' AND key GLOB 't3 code*'));
