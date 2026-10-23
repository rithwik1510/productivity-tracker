# 0003: Identify apps by AUMID if packaged, else exe file name

Status: accepted (2026-10-21), corrected after Fable review the same day

## Context

Every recorded row references an app, so the identity scheme is baked into stored data. Full exe paths break for self-updating apps that install into versioned folders (Discord: `app-1.0.9163\Discord.exe`). UWP apps all run under `ApplicationFrameHost.exe`, so process names alone merge them. Packaged Win32 apps (Store Spotify, WhatsApp, Windows Terminal) run their own exe but also carry package identity.

## Decision

If the foreground process has package identity (`GetApplicationUserModelId` succeeds), identify it by its AppUserModelID (AUMID). For UWP windows hosted by `ApplicationFrameHost.exe`, resolve the hosted `CoreWindow` process first. Otherwise identify by exe file name, case-insensitive.

The app picker must emit the same key: packaged entries from `shell:AppsFolder` use their AUMID; Win32 entries resolve the shortcut's target to an exe name (ignoring any explicit AUMID on the shortcut, e.g. Chrome's).

## Consequences

- History survives app updates and reinstalls to new paths.
- Store apps are tracked individually.
- Different programs sharing a generic runtime exe (`javaw.exe`, `python.exe`) share one identity. Accepted; revisit only if users hit it.
- The same app installed both from the Store and as a normal installer gets two identities. Accepted.
- Changing this scheme later requires migrating stored data.
