# 0006: Minimized Productive apps earn time in parallel, for 30 minutes

Status: superseded by 0007 (2026-10-22). Accepted earlier the same day, decided by the owner.

## Context

The owner works with coding agents (Claude, Codex, T3 Code) and video editors. They start a task, minimizes the app while it runs, and works on something else or waits. Until now only the foreground window earned time, so an agent's 20-minute run or a render counted as nothing, even though it was real work they were responsible for.

Any rule that credits a window the user isn't looking at can invent time. A render left running over lunch must not count, and neither should a minimized app forgotten for the afternoon.

## Decision

A Tracked App whose Category is Productive keeps earning while it has a minimized window (`IsIconic`), for at most 30 minutes from the first tick it is seen minimized and not in front.

- It counts in parallel with the foreground. With VS Code in front and Claude minimized, both earn.
- The same app never earns twice at once. If another of its windows is in front, the foreground wins.
- Presence applies unchanged: it earns only while the user is Active, and every Away trim (no evidence, the 2 h sound cap, lock, sleep, gaps) cuts minimized rows exactly like foreground rows. The 30-minute clock keeps running while Away.
- The clock resets when the app comes to the front or no longer has a minimized window. Closing it, untracking it or making it Entertainment stops it; a Category change closes the row as in ADR-0005.
- Apps only. Entertainment apps, untracked apps and sites never earn it; a minimized browser earns only as itself, and only if it is a Productive Tracked App.
- Stored as ordinary intervals with a `minimized` flag (schema v4), so trims, crash recovery and every aggregate treat them like any row. The Day view shows the minimized part of an item ("incl. 18m minimized").

## Rejected options

- **Carry-over only when nothing tracked is in front.** The owner chose parallel credit over this. Our reasoning: the common case is an agent minimized while the owner works in another tracked app, and this rule gives that case nothing. It only covers waiting.
- **Overlap, but union the totals so Productive never exceeds wall-clock time.** The owner chose parallel credit over this, explicitly accepting that Productive can exceed wall time. Our reasoning: under a union, the minimized app's time would disappear from the totals whenever the owner works in front, which is most of the time it runs.

## Consequences

- The Productive total, the goal ring and History can exceed wall-clock time. A Day's hours are no longer a share of the day. This is accepted.
- 30 minutes bounds what one minimize can add, and requiring presence means a render over lunch adds nothing. A long agent run that the user keeps minimized past 30 minutes stops counting until it is brought to the front.
- Apps that hide to the tray when minimized (the window becomes hidden, not iconic) earn nothing. That is correct under this rule, not a bug.
- Windows that are merely behind others or on another monitor or virtual desktop earn nothing. Minimized is the user's explicit signal.
- The sampler enumerates top-level windows every tick (under 1 ms measured). A minimized UWP frame usually loses its `CoreWindow` child, so its identity comes from the frame's `PKEY_AppUserModel_ID`, which matches the foreground's AUMID.
- `get_now` stays foreground-only.
