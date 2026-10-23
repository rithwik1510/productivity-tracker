# 0009: Updates come from GitHub Releases through Tauri's signed updater, with an opt-out switch

Status: accepted (2026-10-23), decided by the owner (issue 19)

## Context

Up to v0.1.1 the app made no network connections at all, so an installed copy stayed on its version until the owner downloaded the next installer by hand. The app runs in the tray for days and starts at login, so a fix shipped on GitHub could take a long time to reach anyone. The releases already live on GitHub as one NSIS installer each.

## Decision

- **Tauri's updater plugin, driven from Rust.** The check and the download run in the shell (`src-tauri/src/update.rs`). The UI calls app commands only, so the window needs no updater permission, and its CSP (`connect-src ipc:`) still keeps the page off the network.
- **GitHub Releases as the server.** One static file per release, `latest.json` (version, notes, date, and the installer's URL and signature), fetched from `releases/latest/download/latest.json`. No update server of our own.
- **Signed.** Every installer is signed with the updater's private key, and the app accepts only installers that verify against the public key in `tauri.conf.json`. The signature's trusted comment carries the version it was signed for, and the app requires it to match the version `latest.json` announces (`requireSignedVersion`), so a tampered `latest.json` can't pass an older signed installer off as a newer version. The private key lives outside the repo on the owner's PC; `bun run release` signs with it and writes `latest.json`. Losing it means installed copies can't update to a build signed with a new key.
- **On by default, with a switch.** Settings → About → "Check for updates automatically" (schema v7, on for new and existing installs). When on: one check a few seconds after start, then every 6 hours of wall-clock time while running. When off: no request at all. "Check for updates" checks once on demand either way.
- **Downloaded quietly, installed only on request.** An update found is downloaded and verified in the background, then the Day view's titlebar says "Update ready · v0.1.3 · Restart", Settings says the same, and the tray gains "Restart to update". Nothing restarts on its own.
- **No counted time lost.** Restart ends the tracker's stretch and flushes it through the same `shutdown_and_wait` path as tray Quit, just before the installer starts. The installer runs passive (a progress bar, no wizard) and relaunches the app with its window.
- **Quiet failures.** Offline, GitHub down, a missing `latest.json` or a bad signature: a background check logs it and waits for its next turn; only a manual check says "Couldn't check (offline?)". Nothing retries in a loop.

## Rejected options

- **The JS updater API in the window.** It needs updater permissions and a CSP open to GitHub, and the window is closed most of the time.
- **Installing as soon as the download finishes.** An unasked restart would close the window under the owner and cut into counted time.
- **Off by default.** Most installs would then never update. The request carries nothing about usage, and the switch turns it off.

## Consequences

- The app now makes one kind of network request: an HTTPS GET for `latest.json` from GitHub, and the installer when there is a newer version. Like any HTTPS request, GitHub sees the PC's IP address and the updater's user agent. No usage data is sent. README's privacy section says so.
- Cutting a release means `bun run release`, then uploading the renamed installer and `latest.json` to the `v<version>` release. A release without `latest.json` reads as "no update" to installed copies, and the latest release must carry one for the check to find it.
- Copies older than v0.1.2 have no updater; they need one manual install of v0.1.2 or later.
- The installer still isn't Authenticode-signed, so a first manual install still meets SmartScreen. The updater's signature is a separate key that proves the installer came from the owner's release build.
