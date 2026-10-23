//! Top-level windows: the foreground window, its owning app process, and app-window enumeration.

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToBSTR};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow,
    GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;

pub(crate) fn foreground_window() -> Option<HWND> {
    // SAFETY: no preconditions; a null HWND means no foreground window right now.
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_invalid()).then_some(hwnd)
}

pub(crate) fn is_window(hwnd: HWND) -> bool {
    // SAFETY: IsWindow accepts any value, including handles of destroyed windows.
    unsafe { IsWindow(Some(hwnd)) }.as_bool()
}

pub(crate) fn window_pid(hwnd: HWND) -> Option<u32> {
    let mut pid = 0;
    // SAFETY: `pid` is a valid out pointer; an invalid HWND just returns 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

pub(crate) fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: the API writes at most `buf.len()` u16s and returns the count written.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)])
}

pub(crate) fn title(hwnd: HWND) -> String {
    // SAFETY: plain FFI call on any HWND.
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    let Ok(len) = usize::try_from(len) else {
        return String::new();
    };
    if len == 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len + 1];
    // SAFETY: the API writes at most `buf.len()` u16s, NUL included, and returns the count.
    let copied = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(copied).unwrap_or(0)])
}

/// Window classes of explorer.exe's shell surfaces: the desktop (also what "show desktop" and
/// minimize-all leave in front), the taskbars, Alt+Tab and Task View, and taskbar flyouts. File
/// Explorer windows (`CabinetWClass`) are not among them. The Start menu, search and the
/// notification center are packaged apps of their own, which no catalog entry tracks.
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "MultitaskingViewFrame",
    "XamlExplorerHostIslandWindow",
    "ForegroundStaging",
    "TaskListThumbnailWnd",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
];

pub(crate) fn is_shell_surface(hwnd: HWND) -> bool {
    SHELL_CLASSES.contains(&class_name(hwnd).as_str())
}

/// The PID of the app behind a top-level window. UWP apps are hosted in an
/// `ApplicationFrameHost.exe` frame, so for those the hosted `CoreWindow`'s process is the app.
pub(crate) fn app_pid(hwnd: HWND) -> Option<u32> {
    let frame_pid = window_pid(hwnd)?;
    if class_name(hwnd) != "ApplicationFrameWindow" {
        return Some(frame_pid);
    }
    let core = children(hwnd).into_iter().find_map(|child| {
        let pid = window_pid(child)?;
        (pid != frame_pid && class_name(child) == "Windows.UI.Core.CoreWindow").then_some(pid)
    });
    // A suspended or minimized UWP app can lose its CoreWindow; fall back to the frame host.
    Some(core.unwrap_or(frame_pid))
}

/// Minimized to the taskbar. A window hidden to the tray isn't iconic, it's invisible.
pub(crate) fn is_minimized(hwnd: HWND) -> bool {
    // SAFETY: IsIconic accepts any value, including handles of destroyed windows.
    unsafe { IsIconic(hwnd) }.as_bool()
}

/// The AppUserModelID the shell keeps on a top-level window for taskbar grouping, if any. A
/// minimized UWP frame usually has no `CoreWindow` child to resolve, but it still carries its
/// app's AUMID, the same one `GetApplicationUserModelId` gives for the app's process.
pub(crate) fn window_aumid(hwnd: HWND) -> Option<String> {
    // SAFETY: COM is initialized on the calling thread (the sampler's MTA). The PROPVARIANT is
    // read once and cleared exactly once.
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let mut value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let text = PropVariantToBSTR(&value).ok().map(|b| b.to_string());
        let _ = PropVariantClear(&mut value);
        text.filter(|t| !t.is_empty())
    }
}

/// Top-level windows in Z order (topmost first).
pub(crate) fn top_level_windows() -> Vec<HWND> {
    let mut windows = Vec::new();
    // SAFETY: `collect` only runs during this call, while `windows` is alive and borrowed by it.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&raw mut windows as isize)) };
    windows
}

fn children(hwnd: HWND) -> Vec<HWND> {
    let mut windows = Vec::new();
    // SAFETY: as in `top_level_windows`.
    let _ =
        unsafe { EnumChildWindows(Some(hwnd), Some(collect), LPARAM(&raw mut windows as isize)) };
    windows
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: every caller passes a `*mut Vec<HWND>` that outlives the enumeration.
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    windows.push(hwnd);
    BOOL(1)
}

/// What the taskbar would show: visible, not cloaked (other virtual desktops, suspended UWP
/// frames), unowned, not a tool window, and titled.
pub(crate) fn is_app_window(hwnd: HWND) -> bool {
    // SAFETY: plain FFI queries on a window handle; stale handles just fail.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
            return false;
        }
        if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let cloaked_ok = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        )
        .is_ok();
        if cloaked_ok && cloaked != 0 {
            return false;
        }
    }
    !title(hwnd).is_empty()
}
