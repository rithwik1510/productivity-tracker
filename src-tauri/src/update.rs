//! Updates from GitHub Releases (ADR-0009). The check and the download run here, in Rust, so the
//! window's CSP still keeps the page off the network. An update found is downloaded and verified
//! against the public key in `tauri.conf.json`, then waits until the owner presses Restart (Day
//! view, Settings or the tray); nothing restarts on its own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
#[cfg(not(debug_assertions))]
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
#[cfg(not(debug_assertions))]
use tracker::Reader;
use tracker_api::ApiError;

use crate::state::AppState;
use crate::tray;

/// Emitted when an update becomes ready or stops being ready, so the window re-reads it.
const UPDATE_CHANGED: &str = "update-changed";
/// For fetching `latest.json`; a stalled connection fails instead of holding "Checking…".
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// For downloading the installer (about 6 MB).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
/// How long the tracker gets to flush before the installer takes over, as for tray Quit.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(3);

/// Managed state: the update waiting for a restart, if any.
#[derive(Default)]
pub struct Updates {
    ready: Mutex<Option<Ready>>,
    /// One check at a time: a manual check during a background one waits, then finds its result.
    busy: tauri::async_runtime::Mutex<()>,
    /// Set once the tracker has been shut down for the installer.
    stopped: AtomicBool,
}

/// A downloaded installer whose signature has been verified.
struct Ready {
    update: Update,
    bytes: Vec<u8>,
}

impl Updates {
    fn ready(&self) -> MutexGuard<'_, Option<Ready>> {
        self.ready.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The version downloaded and waiting for a restart, such as "0.1.3".
pub fn ready_version(app: &AppHandle) -> Option<String> {
    let ready = app.state::<Updates>();
    ready.ready().as_ref().map(|r| r.update.version.clone())
}

/// Asks GitHub for `latest.json` and, if it names a newer version than this one, downloads and
/// verifies its installer. Returns the version now ready to install, or `None` when up to date.
/// Every failure (offline, GitHub down, a bad signature) is an error; nothing retries.
pub async fn check(app: &AppHandle) -> Result<Option<String>, tauri_plugin_updater::Error> {
    let updates = app.state::<Updates>();
    let _one_at_a_time = updates.busy.lock().await;
    let flush = app.clone();
    let found = app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        // The plugin would relaunch with this process's arguments, and a login start's
        // `--autostart` would bring the app back without its window. A bare `/R` relaunches it
        // with none, so the window opens on the new version.
        .restart_after_install(false)
        .installer_arg("/R")
        // Runs once the installer is written to disk, just before it starts and this process
        // exits: the tracker ends its stretch and flushes, as tray Quit does, so no counted time
        // is lost.
        .on_before_exit(move || {
            flush
                .state::<Updates>()
                .stopped
                .store(true, Ordering::Relaxed);
            flush
                .state::<AppState>()
                .runtime
                .shutdown_and_wait(FLUSH_TIMEOUT);
            tray::hide(&flush);
        })
        .build()?
        .check()
        .await?;
    if let Some(mut update) = found
        && ready_version(app).as_ref() != Some(&update.version)
    {
        update.timeout = Some(DOWNLOAD_TIMEOUT);
        // `download` checks the signature before returning the bytes.
        let bytes = update.download(|_, _| {}, || {}).await?;
        tracing::info!("update {} downloaded", update.version);
        *updates.ready() = Some(Ready { update, bytes });
        changed(app);
    }
    Ok(ready_version(app))
}

/// Flushes the tracker, starts the installer (passive: a progress bar, no wizard) and exits; the
/// installer relaunches the app. Returns only on failure. Blocks, so call it off the main thread.
pub fn restart(app: &AppHandle) -> Result<(), ApiError> {
    if cfg!(debug_assertions) {
        return Err(ApiError {
            message: "Dev builds don't install updates.".into(),
        });
    }
    let updates = app.state::<Updates>();
    let Some(Ready { update, bytes }) = updates.ready().take() else {
        return Err(ApiError {
            message: "No update is ready.".into(),
        });
    };
    // Exits the process once the installer has started.
    let error = match update.install(bytes) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    tracing::error!("couldn't install update {}: {error}", update.version);
    if updates.stopped.load(Ordering::Relaxed) {
        // The tracker has already stopped for the installer; a fresh start brings it back.
        app.restart();
    }
    // Dropped, so the next check downloads it again.
    changed(app);
    Err(ApiError {
        message: format!("Couldn't install the update: {error}"),
    })
}

/// Tells the window and the tray that the ready update changed.
fn changed(app: &AppHandle) {
    tray::refresh(app);
    if let Err(e) = app.emit(UPDATE_CHANGED, ()) {
        tracing::warn!("couldn't tell the window about the update: {e}");
    }
}

/// The background checks while "Check for updates automatically" is on: one shortly after start,
/// then one every 6 hours of wall-clock time, so a PC that slept overnight checks soon after it
/// wakes. A failure waits for the next turn, and nothing is checked while the setting is off.
/// Release builds only: a dev build never downloads on its own.
#[cfg(not(debug_assertions))]
pub fn start(app: &AppHandle) -> std::io::Result<()> {
    use jiff::Timestamp;

    const FIRST_CHECK_AFTER: Duration = Duration::from_secs(10);
    const CHECK_EVERY_MS: i64 = 6 * 3_600_000;
    /// How often the loop looks at the clock and the setting.
    const WAKE_EVERY: Duration = Duration::from_secs(15 * 60);

    let app = app.clone();
    thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            thread::sleep(FIRST_CHECK_AFTER);
            let mut last_check_ms: Option<i64> = None;
            loop {
                let now = Timestamp::now().as_millisecond();
                // A clock set back also counts as due.
                let due =
                    last_check_ms.is_none_or(|last| !(last..last + CHECK_EVERY_MS).contains(&now));
                let enabled = app
                    .state::<AppState>()
                    .read(Reader::settings)
                    .is_ok_and(|s| s.auto_update);
                if due && enabled {
                    last_check_ms = Some(now);
                    if let Err(e) = tauri::async_runtime::block_on(check(&app)) {
                        tracing::info!("update check failed: {e}");
                    }
                }
                thread::sleep(WAKE_EVERY);
            }
        })
        .map(drop)
}
