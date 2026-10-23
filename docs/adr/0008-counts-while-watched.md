# 0008: A watched agent app keeps the owner Active for up to 30 minutes without input

Status: accepted (2026-10-23), decided by the owner (issue 18)

## Context

The owner works with coding agents (T3 Code, Claude, ChatGPT/Codex, their own Grok Bot). Often the agent runs in front and the owner watches it work without touching the keyboard or mouse. Agents make no sound. After "Count as away after" (5 minutes by default) the tracker made the owner Away and trimmed the stretch back to the last input, so that time vanished: their data showed T3 Code until 14:47, a 15-minute gap, and T3 Code again at 15:02.

Raising "Count as away after" for everything would fix the agents and break every other app: a forgotten editor or a document left open would count for 15 minutes or more. Waiting credit (ADR-0007) covers a minimized agent, not one in front.

## Decision

A per-app setting, "Counts while I watch", for Tracked Apps whose Category is Productive. Browsers never get it (their time belongs to their sites), and neither do sites.

- **The rule.** While a watched app is in front, it is evidence of presence, like input or sound, for up to 30 minutes after the last input or sound. So the away threshold doesn't end the stretch.
- **The cap.** Once 30 minutes pass without input or sound, the owner is Away, and the stretch is trimmed back to the last evidence + 30 minutes. The 30 minutes are kept; only the time after them is removed. This matches the waiting-credit clock.
- **It holds a stretch, it never starts one.** A watched app that comes to the front while the owner is Away (or paused, or locked) counts nothing until real input. When a stretch ends, the watch ends with it.
- **Everything else is unchanged.** Lock, display off, sleep, session disconnect and clock gaps end the stretch at once, trimmed as before; the watched seconds before them count, since they were evidence. The 2-hour sound cap still trims back to the last input. Every second still counts once (ADR-0007): only presence changes, and only while the watched app is in front.
- **Switching away returns to the normal rules.** The last watched second is the last evidence, so if another window comes to the front without input, the owner is Away "Count as away after" later, trimmed back to that second.
- **Defaults.** On for a short preset list when an app is first tracked: the Claude desktop app (AUMID prefix `Claude_`), the ChatGPT/Codex desktop apps (`OpenAI.Codex_`, `OpenAI.ChatGPT`) and T3 Code (an exe name starting with `t3 code`). Off for everything else, Cursor included (an editor, not an agent). Schema v6 adds `app.watch` and turns it on for already-tracked apps matching the presets. The setting is kept when an app is untracked and tracked again.
- **UI.** A switch row in the app's popover in Apps & sites, shown only for Productive apps that aren't browsers. While the watch alone keeps a stretch going, the status line reads "Tracking · T3 Code · watching".

## Rejected options

- **A 30-minute choice for "Count as away after".** It would apply to every app, so anything left open in front would count for half an hour.
- **Trimming the whole wait when the cap hits** (back to the last input, like the sound cap). The owner chose to keep the 30 minutes. A watched agent run is real work up to the cap, and 30 minutes is the same bound waiting credit uses.
- **Detecting agent activity** (CPU, window repaints, network). Unreliable across apps and far more code; the owner knows which apps they watch.

## Consequences

- Walking away from a watched app in front can add up to 30 minutes that the owner wasn't there for. That is the trade-off for not losing watched agent runs, and the cap bounds it. Locking the PC (Win+L) ends it at once.
- Only the foreground counts this way. An agent the owner watches on a second monitor while another window is in front gets nothing.
- The setting is per app, not per window: every window of a watched app is watched.
- `NowTracking.watching` is a flag on the existing Tracking state, not a new status.
