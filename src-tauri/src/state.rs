//! Shared app state (plan section 6). Commands clone the `AppHandle` into `spawn_blocking` and
//! reach this through it, because `State<'_>` isn't `'static`.

use std::path::PathBuf;
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::AppHandle;
use tracker::{Reader, RuntimeHandle, StoreError};
use tracker_api::{ApiError, Control};

/// How long a command waits for the poll thread to answer a Control message.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct AppState {
    pub runtime: RuntimeHandle,
    /// One query_only connection shared by every read command and the tray tooltip (queries take
    /// milliseconds, so one connection's memory beats a second one); `Send` but not `Sync`.
    reader: Mutex<Reader>,
    pub data_dir: PathBuf,
    /// Debug builds never touch the autostart registry, so Settings toggles this instead.
    #[cfg(debug_assertions)]
    launch_at_login_debug: AtomicBool,
}

impl AppState {
    pub fn new(runtime: RuntimeHandle, data_dir: PathBuf) -> Result<Self, StoreError> {
        Ok(Self {
            reader: Mutex::new(runtime.reader()?),
            runtime,
            data_dir,
            #[cfg(debug_assertions)]
            launch_at_login_debug: AtomicBool::new(false),
        })
    }

    /// Runs a query on the shared reader. Blocks on the lock, so call it from blocking threads only.
    pub fn read<T>(
        &self,
        query: impl FnOnce(&Reader) -> Result<T, StoreError>,
    ) -> Result<T, ApiError> {
        // Poisoning only means an earlier query panicked; the connection itself is still fine.
        let reader = self.reader.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(query(&reader)?)
    }

    /// Sends a Control message built around a fresh reply channel and waits for the poll thread's
    /// answer, which normally arrives within one tick. A hung poll thread (e.g. a stuck Win32
    /// call) gets `REPLY_TIMEOUT`, so the command fails instead of hanging its blocking thread.
    pub fn request<T>(
        &self,
        message: impl FnOnce(Sender<Result<T, ApiError>>) -> Control,
    ) -> Result<T, ApiError> {
        let (reply, answer) = mpsc::channel();
        self.runtime.send(message(reply))?;
        answer.recv_timeout(REPLY_TIMEOUT).map_err(|e| ApiError {
            message: match e {
                RecvTimeoutError::Timeout => "The tracker didn't answer in time.",
                RecvTimeoutError::Disconnected => "The tracker stopped before answering.",
            }
            .into(),
        })?
    }

    /// Release: the real Run-key state, so a change made outside the app shows up in Settings.
    #[cfg(not(debug_assertions))]
    pub fn launch_at_login(&self, app: &AppHandle) -> Result<bool, ApiError> {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().map_err(autostart_error)
    }

    #[cfg(debug_assertions)]
    pub fn launch_at_login(&self, _app: &AppHandle) -> Result<bool, ApiError> {
        Ok(self.launch_at_login_debug.load(Ordering::Relaxed))
    }

    #[cfg(not(debug_assertions))]
    pub fn set_launch_at_login(&self, app: &AppHandle, enabled: bool) -> Result<(), ApiError> {
        use tauri_plugin_autostart::ManagerExt;
        let autolaunch = app.autolaunch();
        if enabled {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        }
        .map_err(autostart_error)
    }

    #[cfg(debug_assertions)]
    pub fn set_launch_at_login(&self, _app: &AppHandle, enabled: bool) -> Result<(), ApiError> {
        self.launch_at_login_debug.store(enabled, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(not(debug_assertions))]
fn autostart_error(e: tauri_plugin_autostart::Error) -> ApiError {
    ApiError {
        message: format!("Launch at login failed: {e}"),
    }
}
