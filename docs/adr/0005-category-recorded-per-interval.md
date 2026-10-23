# 0005: Category is recorded with each interval; history never rewrites

Status: accepted (2026-10-21)

## Context

A user can change a Tracked App's or Tracked Site's Category (e.g. Discord from Entertainment to Productive). Either history follows the current Category (stored once on the app) or history keeps the Category that applied when the time was recorded (stored on every interval).

## Decision

Each recorded interval stores its Category. Changing a Category affects future time only. History shows what was true at the time.

## Consequences

- History is an honest record: a month of Entertainment Discord stays Entertainment after the switch.
- A Category chosen by mistake stays wrong in past data. Accepted; no "apply to past" option in v1.
- All aggregates read the Category from intervals, never from the current Tracked App or Tracked Site.
