//! Windows adapters (plan section 5): the `Sampler` implementation, the session watcher and the
//! app catalog. Depends on `tracker-api` only.
//!
//! The `probe` feature adds the `tracker-probe` CLI's extras (`spike`, `WinSampler::watching`);
//! the app never enables it.

mod audio;
mod browser;
pub mod catalog;
mod com;
mod foreground;
mod icons;
mod input;
mod process;
mod session;
#[cfg(feature = "probe")]
pub mod spike;
mod version;

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tracker_api::{AppIdentity, Foreground, Sample, Sampler, is_browser, uia_browser};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::COINIT_MULTITHREADED;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::HSTRING;

pub use session::SessionWatcher;

use crate::audio::AudioMonitor;
use crate::browser::BrowserReader;
use crate::com::Apartment;
use crate::input::Idle;

/// Built on the runtime's poll thread: it enters the MTA there and creates its COM objects
/// lazily on first use, so it is deliberately `!Send`.
pub struct WinSampler {
    watcher: Arc<SessionWatcher>,
    /// The probe can read one browser without it being in front; the app reads the foreground.
    #[cfg(feature = "probe")]
    watch: spike::Watch,
    started: Instant,
    own_pid: u32,
    idle: Idle,
    audio: AudioMonitor,
    browser: BrowserReader,
    // Declared last so the COM objects above are released before COM is uninitialized.
    _com: Apartment,
}

impl WinSampler {
    pub fn new(watcher: Arc<SessionWatcher>) -> Self {
        Self {
            _com: Apartment::enter(COINIT_MULTITHREADED),
            watcher,
            #[cfg(feature = "probe")]
            watch: spike::Watch::Foreground,
            started: Instant::now(),
            own_pid: std::process::id(),
            idle: Idle::new(),
            audio: AudioMonitor::new(),
            browser: BrowserReader::new(),
        }
    }

    #[cfg(not(feature = "probe"))]
    fn window(&self) -> Option<HWND> {
        foreground::foreground_window()
    }

    #[cfg(feature = "probe")]
    fn window(&self) -> Option<HWND> {
        self.watch.window()
    }

    fn foreground(&mut self) -> Option<Foreground> {
        let hwnd = self.window()?;
        // The desktop, taskbar and task switcher belong to explorer.exe, as File Explorer does;
        // they are no app, so they must not count as File Explorer if the user tracks it.
        if foreground::is_shell_surface(hwnd) {
            return None;
        }
        let pid = foreground::app_pid(hwnd)?;
        let identity = process::identity(pid)?;
        if pid == self.own_pid {
            // Our own window never counts, so skip the title, audio and browser reads. The title
            // matters most: reading our own window's text is a SendMessage to our UI thread,
            // which can be blocked waiting for this thread (tray Quit's shutdown_and_wait).
            return Some(Foreground {
                identity,
                own_process: true,
                audible: false,
                browser: None,
                title: String::new(),
            });
        }
        let title = foreground::title(hwnd);
        let audible = self.audio.audible(pid, &identity);
        let browser = if uia_browser(&identity) {
            Some(self.browser.read(hwnd, &title))
        } else if is_browser(&identity) {
            Some(browser::title_view(&title))
        } else {
            None
        };
        Some(Foreground {
            identity,
            own_process: false,
            audible,
            browser,
            title,
        })
    }

    /// Apps with a minimized app window right now, each once (ADR-0007). The window filter is the
    /// foreground's and the catalog's. Our own windows and private browser windows are left out:
    /// private by title marker (Edge, Firefox), and for UIA browsers (Chrome, Brave) unless UIA
    /// read the window as normal while it was in front.
    fn minimized(&self) -> Vec<AppIdentity> {
        let mut apps = Vec::new();
        for hwnd in foreground::top_level_windows() {
            // Cheap tests first; most windows aren't minimized. Our own windows are skipped before
            // `is_app_window` reads a title: that read is a SendMessage to our UI thread, which can
            // be blocked waiting for this thread (see `foreground`).
            if !foreground::is_minimized(hwnd)
                || foreground::window_pid(hwnd) == Some(self.own_pid)
                || !foreground::is_app_window(hwnd)
            {
                continue;
            }
            let Some(identity) = minimized_identity(hwnd) else {
                continue;
            };
            // A UIA browser window counts as private until UIA has read it as normal.
            if is_browser(&identity)
                && ((uia_browser(&identity) && !self.browser.known_normal(hwnd))
                    || browser::title_is_private(&foreground::title(hwnd)))
            {
                continue;
            }
            if !apps.contains(&identity) {
                apps.push(identity);
            }
        }
        apps
    }
}

/// A minimized window's app identity, as the foreground would resolve it. A minimized UWP frame
/// usually has lost its `CoreWindow` child, so `app_pid` falls back to the frame host; the frame's
/// own AUMID names the app instead. Without one, it isn't an app of its own.
fn minimized_identity(hwnd: HWND) -> Option<AppIdentity> {
    let identity = process::identity(foreground::app_pid(hwnd)?)?;
    if matches!(&identity, AppIdentity::Exe { key } if key == "applicationframehost.exe") {
        return foreground::window_aumid(hwnd).map(|key| AppIdentity::Aumid { key });
    }
    Some(identity)
}

impl Sampler for WinSampler {
    fn sample(&mut self) -> Sample {
        let foreground = self.foreground();
        Sample {
            now_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)),
            mono_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            away_signal: self.watcher.away_signal(),
            idle_ms: self.idle.idle_ms(),
            input_event: self.watcher.take_input_event(),
            foreground,
            minimized: self.minimized(),
        }
    }
}

/// A modal error dialog with no owner window, for failures the user must see even though the app
/// has no console and maybe no window yet (startup). Blocks until dismissed.
pub fn error_box(title: &str, message: &str) {
    // SAFETY: plain FFI call; both strings outlive it.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from(title),
            MB_OK | MB_ICONERROR,
        )
    };
}
