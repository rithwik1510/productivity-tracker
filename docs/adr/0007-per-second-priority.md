# 0007: Every second counts once; a minimized Productive app waits in for a foreground that isn't Productive

Status: accepted (2026-10-22), decided by the owner. Supersedes 0006.

## Context

ADR-0006 let a minimized Productive app earn time in parallel with the foreground, for up to 30 minutes per minimize. With VS Code in front and Claude minimized, both earned. The owner rejected this after using it, because it inflates hours beyond wall-clock time.

The problem 0006 solved is still real. The owner starts a task in a coding agent (Claude, Codex, T3 Code) or a video editor, minimizes it, and waits. While waiting the owner may watch YouTube, sit on the desktop or read in an untracked app. Counting only the foreground loses that work.

## Decision

Every second counts once: for the app in front if it's Productive; otherwise for a Productive app minimized in the last 30 minutes; otherwise for whatever's in front.

On each Active tick the engine picks one target, in this order:

1. A Productive foreground, app or site.
2. A minimized Productive app with clock left ("waiting credit").
3. An Entertainment foreground, app or site.
4. Nothing.

- **The 30-minute clock** is unchanged from 0006, with one amendment (2026-10-22, issue 15). It starts on the first tick the app is seen minimized and not in front, but only if the tracker has seen the app not minimized before (in front, or restored). It runs on wall time: while Away, and while another Productive app is in front. It resets when the app is restored or focused, or no longer has a minimized window. It is a window since the minimize, not 30 minutes of earning.
- **Several minimized apps** with clock left: the one with the most left (the most recent minimize) waits in. A tie goes to the lowest app id. One app per second.
- **Presence is unchanged.** Away trims everything. A minimized app's sound isn't evidence. Foreground sound is, so a minimized agent can cover a sound-only YouTube video, up to its clock. Accepted.
- **Apps only**, as in 0006. Entertainment and untracked apps never wait in; sites never do. Private windows aren't listed as minimized.
- **Stored** as ordinary rows with the existing `minimized` flag (schema v4). The flag is part of the row's key, so "Claude waiting" and "Claude in front" are separate rows. A Category change closes the row (ADR-0005).
- At most one row is open at a time, and nothing is rewritten after the fact.

## Rejected options

- **Parallel credit (ADR-0006).** The owner rejected it: it inflates hours beyond wall-clock time. A Day's Productive total could exceed the hours the owner was at the computer.
- **Credit the minimized app when the owner comes back to productive work.** This was the owner's first idea: hold the minimized time as pending and grant it only once the owner returns to a Productive app. Our reasoning for not doing it:
  - The confirmation adds no information. Coming back to VS Code says nothing about whether the agent's run was worth counting.
  - It needs retroactive rewriting: pending time has to become Productive (or be dropped) after the fact, which the store and recovery avoid today.
  - The live Day view would shrink or jump. Pending time either shows and then disappears, or doesn't show and then appears in a lump.
  - It raises questions with no good answer. Which app gets the time if two were minimized? How long may pending time wait for the confirmation? Does a lock in between cancel it?

## Consequences

- Productive + Entertainment never exceeds wall-clock Active time. The Day's hours are a share of the day again.
- Time spent working in front of the agent counts once, for the foreground. The agent earns only while the owner is waiting on something that isn't Productive.
- A YouTube break right after minimizing an agent counts as Productive for up to 30 minutes. That's the rule doing its job, and also a way to inflate Productive time; 30 minutes per minimize bounds it.
- A tracker start isn't a minimize. Before the amendment, every app found minimized on the first tick got a full clock, so each start (a login with windows restored minimized, the reopen after a panic, Delete all) could add up to 30 Productive minutes with no user action. Now an app minimized before the start earns nothing until the user brings it back and minimizes it again, and a restart mid-credit drops the rest of that clock. Missing a few minutes is safer than inventing them.
- The status line (`get_status`, issue 16; `get_now` before it) names what the tick counts, so during waiting credit it names the minimized app, flagged `waiting` (issue 15).
- The engine, writer and recovery are back to one open row. ADR-0006's per-app lanes are gone.
- The Day view shows the waiting part of an item: "incl. 18m waiting".
