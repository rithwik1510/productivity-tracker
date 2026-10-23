# 0001: Track only selected apps

Status: accepted (2026-10-21)

## Context

The tracker could record every foreground app, record unselected time as an anonymous "Other" bucket, or record only the apps the user picks.

## Decision

Only Tracked Apps are recorded. Time spent in any other app is not counted, not bucketed, and not stored.

## Consequences

- Clean data: the user sees only what they chose to care about.
- The app cannot answer "how long was I on my laptop today?" Totals are sums of Tracked App time, never a share of total screen time.
- Don't add an "Other" or "untracked" bucket without revisiting this ADR.
