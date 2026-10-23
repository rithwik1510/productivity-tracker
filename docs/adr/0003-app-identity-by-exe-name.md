# 0003: Identify apps by AUMID if packaged, else exe file name

Status: accepted (2026-10-21), corrected after Fable review the same day

## Context

Every recorded row references an app, so the identity scheme is baked into stored data. Full exe paths break for self-updating apps that install into versioned folders (Discord: `app-1.0.9163\Discord.exe`). UWP apps all run under `ApplicationFrameHost.exe`, so process names alone merge them. Packaged Win32 apps (Store Spotify, WhatsApp, Windows Terminal) run their own exe but also carry package identity.

## Decision

If the foreground process has package identity (`GetApplicationUserModelId` succeeds), identify it by its AppUserModelID (AUMID). For UWP windows hosted by `ApplicationFrameHost.exe`, resolve the hosted `CoreWindow` process first. Otherwise identify by exe file name, case-insensitive.

The app picker must emit the same key: packaged entries from `shell:AppsFolder` use their AUMID; Win32 entries resolve the shortcut's target to an exe name (ignoring any explicit AUMID on the shortcut, e.g. Chrome's).

**Amendment (2026-10-23, issue 18): Chromium forks that ship `chrome.exe`.** Helium, Chromium, ungoogled-chromium, Supermium and other forks name their browser exe `chrome.exe`, so they were all "Google Chrome". The owner tracks Chrome as Productive and watched YouTube in Helium, which counted as Productive Chrome. Now a `chrome.exe` whose version-info ProductName isn't Google Chrome's (any Chrome channel starts with "Google Chrome") and isn't empty gets the key `chrome.exe|<ProductName, lowercased>`, e.g. `chrome.exe|helium`. It shows under its own name and icon, and it is still a browser read like Chrome (UI Automation, `KNOWN_BROWSERS` by the part before `|`). Google Chrome, and a `chrome.exe` without a ProductName, keep `chrome.exe`, so existing data and Chrome's category stay. The sampler and the picker both build the key with `tracker_api::exe_identity`, the picker from the shortcut's target. The ProductName is read once per exe path and cached, since the sampler asks every tick. No other known browser exe is shared by forks, so the rule is `chrome.exe` only (Firefox's forks ship their own exe names; Tor Browser's `firefox.exe` is left as it is).

## Consequences

- History survives app updates and reinstalls to new paths.
- Store apps are tracked individually.
- Different programs sharing a generic runtime exe (`javaw.exe`, `python.exe`) share one identity. Accepted; revisit only if users hit it. Chromium forks' `chrome.exe` is the exception (amendment above).
- Time a fork recorded before the amendment stays under Google Chrome. It can't be told apart after the fact.
- The same app installed both from the Store and as a normal installer gets two identities. Accepted.
- Changing this scheme later requires migrating stored data.
