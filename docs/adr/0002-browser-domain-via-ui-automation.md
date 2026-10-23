# 0002: Read the browser's domain via UI Automation, fall back to window title

Status: accepted (2026-10-21), pending a feasibility spike

## Context

To split browser time into Tracked Sites, the tracker must know which site the foreground tab shows. Options were: treat the browser as one app (too coarse), match window-title keywords (fragile: many titles omit the site name), read the address bar through Windows UI Automation, or ship a browser extension (exact, but a second artifact, per-browser installs, and store publishing).

## Decision

When a browser is the foreground window, read the address bar's value through Windows UI Automation and keep only the domain. If that read fails, match the window title against a keyword for each Tracked Site.

## Consequences

- One install, no extension. Works across Chromium browsers (Chrome, Edge, Brave, ...).
- Only the domain is kept; full URLs and paths are never stored. Window titles are read in memory only and never persisted.
- Private windows (Incognito/InPrivate/Private Browsing) are detected and never tracked. The spike must confirm detection per browser (window title marker or UIA). Amended 2026-10-22 (issue 16): for a UIA browser, a window whose privacy hasn't been read yet (the read failed or timed out, or UIA is unavailable) counts as private. The spike had chosen the opposite so a broken UIA wouldn't stop Chrome tracking; the owner prefers missing time to recording an incognito window, since Chrome's title carries no marker.
- Fullscreen (F11, video fullscreen) hides the address bar, so the title fallback must be first-class: each Tracked Site has a title keyword defaulting to its domain label (`youtube`), user-editable. While the user types in the address bar, its value is a draft; ignore values that don't parse as a domain and keep the last good one.
- Locate the address bar structurally (toolbar -> Edit control), not by its localized name, and never traverse into web content: that is what would switch on Chrome's costly web-content accessibility.
- Browser UI updates can break the UI Automation lookup; the title fallback keeps tracking degraded rather than broken.
- Enabling accessibility queries may cost the browser CPU/memory. A spike must confirm reliability and overhead in Chrome and Edge before we build on this. Acceptance: after 30 minutes of 1s polling, `chrome://accessibility` shows Native API on and Web contents off, and renderer CPU is flat. If it fails, the next option is a browser extension, not title matching alone.
