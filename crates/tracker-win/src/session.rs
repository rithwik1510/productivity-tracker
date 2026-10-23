//! Lock, disconnect, display-off and end-session notifications (plan section 5 "Session"), plus
//! the light/dark theme broadcast the tray icon follows.
//!
//! A hidden top-level window on its own thread receives them. It is a real top-level window,
//! not `HWND_MESSAGE`, because message-only windows don't get the `WM_ENDSESSION` and
//! `WM_SETTINGCHANGE` broadcasts.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{POWERBROADCAST_SETTING, RegisterPowerSettingNotification};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTSActive, WTSFreeMemory,
    WTSINFOEXW, WTSQuerySessionInformationW, WTSRegisterSessionNotification, WTSSessionInfoEx,
};
use windows::Win32::System::SystemServices::{
    GUID_CONSOLE_DISPLAY_STATE, GUID_SESSION_DISPLAY_STATUS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DispatchMessageW, GetMessageW,
    GetSystemMetrics, KillTimer, MSG, PBT_POWERSETTINGCHANGE, RegisterClassExW, SM_REMOTESESSION,
    SetTimer, TranslateMessage, WINDOW_EX_STYLE, WM_ENDSESSION, WM_POWERBROADCAST,
    WM_SETTINGCHANGE, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_OVERLAPPED,
    WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT,
    WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::core::{PCWSTR, PWSTR, Result, w};

type Hook = Box<dyn Fn() + Send + Sync>;

/// The timer that retries a failed `WTSRegisterSessionNotification`, and how often.
const REGISTER_TIMER_ID: usize = 1;
const REGISTER_RETRY_MS: u32 = 5_000;

/// Owns its own thread and hidden window; everything else reads its atomics.
pub struct SessionWatcher {
    locked: AtomicBool,
    disconnected: AtomicBool,
    /// The physical console's display (`GUID_CONSOLE_DISPLAY_STATE`).
    console_display_off: AtomicBool,
    /// The display of our session (`GUID_SESSION_DISPLAY_STATUS`). Over Remote Desktop it follows
    /// the remote client, while the console's display can be off with the user at work.
    session_display_off: AtomicBool,
    input_event: AtomicBool,
    end_session_hook: Mutex<Option<Hook>>,
    theme_hook: Mutex<Option<Hook>>,
}

thread_local! {
    /// The watcher served by this thread's window procedure (set once on the watcher thread).
    static WATCHER: RefCell<Option<Arc<SessionWatcher>>> = const { RefCell::new(None) };
}

impl SessionWatcher {
    /// Starts the watcher thread and returns once the initial session state is known. If the
    /// window can't be created the watcher still works, it just never reports away.
    pub fn start() -> Arc<SessionWatcher> {
        let watcher = Arc::new(SessionWatcher {
            locked: AtomicBool::new(false),
            disconnected: AtomicBool::new(false),
            console_display_off: AtomicBool::new(false),
            session_display_off: AtomicBool::new(false),
            input_event: AtomicBool::new(false),
            end_session_hook: Mutex::new(None),
            theme_hook: Mutex::new(None),
        });
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread_watcher = Arc::clone(&watcher);
        let spawned = std::thread::Builder::new()
            .name("session-watcher".into())
            .spawn(move || run(thread_watcher, ready_tx));
        match spawned {
            Ok(_) => {
                let _ = ready_rx.recv_timeout(Duration::from_secs(2));
            }
            Err(err) => tracing::error!("session watcher thread failed to start: {err}"),
        }
        watcher
    }

    /// Runs on `WM_ENDSESSION(TRUE)` before the window procedure returns, so Windows waits for it.
    /// The shell wires it to `shutdown_and_wait(2 s)`.
    pub fn set_end_session_hook(&self, hook: Hook) {
        set_hook(&self.end_session_hook, hook);
    }

    /// Runs on `WM_SETTINGCHANGE("ImmersiveColorSet")`, which Windows broadcasts when the light or
    /// dark theme (or the accent colour) changes. Runs on the watcher thread.
    pub fn set_theme_hook(&self, hook: Hook) {
        set_hook(&self.theme_hook, hook);
    }

    /// Locked, disconnected, or display off.
    pub(crate) fn away_signal(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
            || self.disconnected.load(Ordering::Relaxed)
            || self.display_off()
    }

    /// The session's display in a Remote Desktop session, the console's otherwise. Asked every
    /// time, because a session can move between the console and a remote client.
    fn display_off(&self) -> bool {
        // SAFETY: no pointers; just reads a system metric.
        let remote = unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0;
        if remote {
            self.session_display_off.load(Ordering::Relaxed)
        } else {
            self.console_display_off.load(Ordering::Relaxed)
        }
    }

    /// True once after an unlock.
    pub(crate) fn take_input_event(&self) -> bool {
        self.input_event.swap(false, Ordering::Relaxed)
    }

    fn on_session_change(&self, event: u32) {
        match event {
            WTS_SESSION_LOCK => self.locked.store(true, Ordering::Relaxed),
            WTS_SESSION_UNLOCK => {
                self.locked.store(false, Ordering::Relaxed);
                // Unlocking counts as input (GLOSSARY "Active / Away").
                self.input_event.store(true, Ordering::Relaxed);
            }
            WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => {
                self.disconnected.store(true, Ordering::Relaxed);
            }
            WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => {
                self.disconnected.store(false, Ordering::Relaxed);
            }
            _ => {}
        }
        tracing::debug!(event, away = self.away_signal(), "session change");
    }

    /// Both display GUIDs report 0 off, 1 on, 2 dimmed (dimmed still counts as present).
    fn on_display_state(&self, slot: &AtomicBool, state: u32) {
        slot.store(state == 0, Ordering::Relaxed);
        tracing::debug!(state, away = self.away_signal(), "display state");
    }
}

// A poisoned lock only means a previous hook panicked; replacing or running it is still correct.
fn set_hook(slot: &Mutex<Option<Hook>>, hook: Hook) {
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
}

fn run_hook(slot: &Mutex<Option<Hook>>) {
    let hook = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(hook) = hook.as_ref() {
        hook();
    }
}

fn run(watcher: Arc<SessionWatcher>, ready: mpsc::Sender<()>) {
    WATCHER.with(|slot| *slot.borrow_mut() = Some(Arc::clone(&watcher)));
    match create_window() {
        Ok(hwnd) => {
            // Register before reading the initial state, so a change in between is queued.
            // At login this can fail until the session service is up: retry on a timer.
            if let Err(err) = register_session_notification(hwnd) {
                tracing::warn!(
                    "WTSRegisterSessionNotification failed, retrying every {REGISTER_RETRY_MS} ms: {err}"
                );
                // SAFETY: `hwnd` is a live window owned by this thread; WM_TIMER arrives through
                // `pump` below.
                unsafe { SetTimer(Some(hwnd), REGISTER_TIMER_ID, REGISTER_RETRY_MS, None) };
            }
            // Windows posts each display state right after registering. The handles are never
            // unregistered: the window lives as long as the process. If one fails, its display
            // just never reads as off.
            for guid in [&GUID_CONSOLE_DISPLAY_STATE, &GUID_SESSION_DISPLAY_STATUS] {
                // SAFETY: as above; the GUIDs are statics.
                if let Err(err) = unsafe {
                    RegisterPowerSettingNotification(
                        HANDLE(hwnd.0),
                        guid,
                        DEVICE_NOTIFY_WINDOW_HANDLE,
                    )
                } {
                    tracing::warn!("RegisterPowerSettingNotification({guid:?}) failed: {err}");
                }
            }
            read_initial_state(&watcher);
            let _ = ready.send(());
            pump();
        }
        Err(err) => {
            tracing::error!("session watcher window failed: {err}");
            read_initial_state(&watcher);
            let _ = ready.send(());
        }
    }
}

fn register_session_notification(hwnd: HWND) -> Result<()> {
    // SAFETY: `hwnd` is a live window owned by the calling (watcher) thread.
    unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) }
}

fn create_window() -> Result<HWND> {
    // SAFETY: registers a class whose window procedure is a plain `extern "system"` fn; failing
    // with "class already exists" (a second watcher in this process) is fine.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("ProductivityTrackerSessionWatcher"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        // Never shown: no WS_VISIBLE, zero size.
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("ProductivityTrackerSessionWatcher"),
            w!(""),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}

fn pump() {
    let mut msg = MSG::default();
    // SAFETY: standard message loop on the thread that owns the window; `msg` is a valid out
    // pointer. GetMessageW returns 0 on WM_QUIT and -1 on error, both of which end the loop.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Locked and disconnected state at startup from `WTSQuerySessionInformation(WTSSessionInfoEx)`.
fn read_initial_state(watcher: &SessionWatcher) {
    let mut buffer = PWSTR::null();
    let mut bytes = 0u32;
    // SAFETY: out pointers are valid; on success the buffer is freed below with WTSFreeMemory.
    let queried = unsafe {
        WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        )
    };
    if let Err(err) = queried {
        tracing::warn!("WTSQuerySessionInformation failed: {err}");
        return;
    }
    if bytes as usize >= size_of::<WTSINFOEXW>() {
        // SAFETY: the API returned at least a whole WTSINFOEXW; level 1 is the only union member.
        let info = unsafe { std::ptr::read_unaligned(buffer.0.cast::<WTSINFOEXW>()) };
        if info.Level == 1 {
            // SAFETY: `Level == 1` means the level-1 member is the initialized one.
            let level1 = unsafe { info.Data.WTSInfoExLevel1 };
            let locked = level1.SessionFlags == WTS_SESSIONSTATE_LOCK as i32;
            watcher.locked.store(locked, Ordering::Relaxed);
            watcher
                .disconnected
                .store(level1.SessionState != WTSActive, Ordering::Relaxed);
        }
    }
    // SAFETY: `buffer` came from a successful WTSQuerySessionInformationW.
    unsafe { WTSFreeMemory(buffer.0.cast()) };
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handled = WATCHER.with(|slot| {
        let slot = slot.borrow();
        let watcher = slot.as_ref()?;
        match msg {
            WM_WTSSESSION_CHANGE => {
                watcher.on_session_change(wparam.0 as u32);
                Some(LRESULT(0))
            }
            WM_POWERBROADCAST if wparam.0 as u32 == PBT_POWERSETTINGCHANGE => {
                // SAFETY: for PBT_POWERSETTINGCHANGE, lParam points to a POWERBROADCAST_SETTING
                // whose `Data` holds `DataLength` bytes (a DWORD for either display state).
                let setting = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };
                let slot = match setting.PowerSetting {
                    GUID_CONSOLE_DISPLAY_STATE => Some(&watcher.console_display_off),
                    GUID_SESSION_DISPLAY_STATUS => Some(&watcher.session_display_off),
                    _ => None,
                };
                if let Some(slot) = slot
                    && setting.DataLength >= 4
                {
                    // SAFETY: as above; `Data` is not 4-byte aligned, hence read_unaligned.
                    let state =
                        unsafe { std::ptr::read_unaligned(setting.Data.as_ptr().cast::<u32>()) };
                    watcher.on_display_state(slot, state);
                }
                Some(LRESULT(1))
            }
            WM_ENDSESSION if wparam.0 != 0 => {
                run_hook(&watcher.end_session_hook);
                Some(LRESULT(0))
            }
            WM_SETTINGCHANGE => {
                // SAFETY: for WM_SETTINGCHANGE, lParam is null or a NUL-terminated string naming
                // the changed area, valid for the duration of the call.
                let area = PCWSTR(lparam.0 as *const u16);
                if !area.is_null()
                    && unsafe { area.to_string() }.is_ok_and(|area| area == "ImmersiveColorSet")
                {
                    run_hook(&watcher.theme_hook);
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == REGISTER_TIMER_ID => {
                // Failures were logged once when the timer started; stay quiet until it works.
                if register_session_notification(hwnd).is_ok() {
                    // SAFETY: our own timer on our own window.
                    let _ = unsafe { KillTimer(Some(hwnd), REGISTER_TIMER_ID) };
                    // Lock changes before registering were never delivered: re-read the state.
                    read_initial_state(watcher);
                    tracing::info!("WTSRegisterSessionNotification succeeded on retry");
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    });
    // SAFETY: forwarding the unhandled message with its original arguments.
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) })
}
