# 0004: Tauri v2 shell around a standalone Rust tracker crate

Status: accepted (2026-10-21), after independent Fable review

## Context

The app runs from login in the tray 10+ hours a day, mostly with no window open. Its hardest parts are Win32/COM calls: foreground window + AUMID, last input, UI Automation address-bar read, per-process audio sessions (WASAPI), lock/sleep/RDP events, Start Menu enumeration with icons. Electron was the main alternative.

## Decision

- Tauri v2. Frontend: React + TypeScript + Tailwind + Motion, built with Bun.
- The tracker is plain Rust with no Tauri dependency, split into three crates: `tracker-api` (frozen shared types and the `Sampler` trait), `tracker` (engine, store, Day math, runtime; platform-free), and `tracker-win` (Win32 adapters and a headless probe CLI used by the ADR-0002 spike). `tracker` and `tracker-win` depend only on `tracker-api`, never on each other, so the core stays portable and both can be built in isolation. A thin Tauri shell wires them together. (Amended 2026-10-21 after plan review; the owner delegated implementation structure.)
- Closing the window destroys it (and its WebView2 processes); tracking continues; the tray recreates it. At login the app starts with no window. The frontend is never required for writes.

## Consequences

- Small footprint while closed (Rust process only) and a ~12 MB installer vs ~90 MB+ for Electron.
- The Win32/COM work uses Microsoft's `windows` crate directly; Electron would have needed C++ addons for UIA, WASAPI, and AppsFolder anyway.
- Some Rust in the codebase (~1-2k lines of thin Win32 calls). Electron's first-party powerMonitor/tray/updater conveniences are given up.
- Reopening the window has a cold-start cost (budget ~0.3-1 s, unverified).
- A future macOS port adds a macOS adapter crate alongside `tracker-win`; `tracker`, `tracker-api` and the UI are unchanged.
