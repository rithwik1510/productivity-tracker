//! `tracker-probe` extras behind the `probe` feature, never built into the app: the browser
//! drivers for the ADR-0002 spike (`tracker-probe spike ...`) and watching one window.

use std::sync::Arc;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::COINIT_MULTITHREADED;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetWindowThreadProcessId, IsIconic, SW_RESTORE, SetForegroundWindow,
    ShowWindow,
};
use windows::core::BSTR;

use crate::browser::{Privacy, Uia, value_pattern};
use crate::com::Apartment;
use crate::foreground::{app_pid, foreground_window, is_app_window, is_window, top_level_windows};
use crate::{SessionWatcher, WinSampler};

/// Which window the probe's sampler reads.
#[derive(Debug, Clone, Copy)]
pub enum Watch {
    Foreground,
    /// The topmost app window of this process (UWP apps: the process behind the frame).
    Pid(u32),
    /// This top-level window handle.
    Hwnd(isize),
}

impl Watch {
    pub(crate) fn window(self) -> Option<HWND> {
        match self {
            Watch::Foreground => foreground_window(),
            Watch::Pid(pid) => main_window_of(pid),
            Watch::Hwnd(hwnd) => Some(HWND(hwnd as *mut _)).filter(|&h| is_window(h)),
        }
    }
}

impl WinSampler {
    /// A sampler that reads `watch` instead of the foreground, so the probe can follow one
    /// browser without it being in front.
    pub fn watching(watcher: Arc<SessionWatcher>, watch: Watch) -> Self {
        Self {
            watch,
            ..Self::new(watcher)
        }
    }
}

/// The topmost app window whose app process is `pid` (UWP frames included).
fn main_window_of(pid: u32) -> Option<HWND> {
    top_level_windows()
        .into_iter()
        .find(|&hwnd| is_app_window(hwnd) && app_pid(hwnd) == Some(pid))
}

#[derive(Debug, thiserror::Error)]
pub enum SpikeError {
    #[error("process {0} has no app window")]
    NoWindow(u32),
    #[error("no omnibox found in the window")]
    NoOmnibox,
    #[error("the window could not be brought to the front")]
    NotForeground,
    #[error("Windows API error: {0}")]
    Windows(#[from] windows::core::Error),
}

/// Types `text` into the omnibox of `pid`'s topmost window the way a user would start a draft:
/// UIA `SetFocus`, then `ValuePattern.SetValue`.
pub fn set_omnibox(pid: u32, text: &str) -> Result<(), SpikeError> {
    let _com = Apartment::enter(COINIT_MULTITHREADED);
    let hwnd = main_window_of(pid).ok_or(SpikeError::NoWindow(pid))?;
    let uia = Uia::new()?;
    let omnibox = uia
        .find_omnibox(hwnd, &mut Privacy::Unknown)?
        .ok_or(SpikeError::NoOmnibox)?;
    // SAFETY: COM calls on objects created on this thread, released before `_com` drops.
    unsafe {
        omnibox.SetFocus()?;
        value_pattern(&omnibox)?.SetValue(&BSTR::from(text))?;
    }
    Ok(())
}

/// Brings `pid`'s topmost window to the front despite the foreground lock: first by attaching
/// to the foreground thread's input queue, then with the ALT-key workaround.
pub fn focus(pid: u32) -> Result<(), SpikeError> {
    let hwnd = main_window_of(pid).ok_or(SpikeError::NoWindow(pid))?;
    // SAFETY: plain FFI calls on a window handle; stale handles just fail.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let ours = GetCurrentThreadId();
        let theirs = foreground_window().map_or(0, |fg| GetWindowThreadProcessId(fg, None));
        let attached =
            theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, true).as_bool();
        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);
        if attached {
            let _ = AttachThreadInput(ours, theirs, false);
        }
        if foreground_window() != Some(hwnd) {
            // A synthetic ALT press makes Windows treat us as having had the last input.
            let key = |flags| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_MENU,
                        dwFlags: flags,
                        ..Default::default()
                    },
                },
            };
            let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
            SendInput(&inputs, size_of::<INPUT>() as i32);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    if foreground_window() == Some(hwnd) {
        Ok(())
    } else {
        Err(SpikeError::NotForeground)
    }
}
