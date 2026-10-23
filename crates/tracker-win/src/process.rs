//! Process identity (ADR-0003) and the parent-PID snapshot used for audio descendants.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use tracker_api::{AppIdentity, exe_identity};
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
use windows::Win32::Storage::Packaging::Appx::{
    APPLICATION_USER_MODEL_ID_MAX_LENGTH, GetApplicationUserModelId,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

/// An open process handle, closed on drop.
struct Process(HANDLE);

impl Process {
    fn open(pid: u32) -> Option<Process> {
        // SAFETY: plain FFI call; the returned handle is owned by `Process` and closed exactly once.
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .ok()
            .map(Process)
    }

    /// The AppUserModelID, if the process has package identity.
    fn aumid(&self) -> Option<String> {
        let mut buf = [0u16; APPLICATION_USER_MODEL_ID_MAX_LENGTH as usize];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` u16s; the API writes at most that many, NUL included.
        let status =
            unsafe { GetApplicationUserModelId(self.0, &mut len, Some(PWSTR(buf.as_mut_ptr()))) };
        (status == ERROR_SUCCESS).then(|| from_wide_nul(&buf))
    }

    fn image_path(&self) -> Option<PathBuf> {
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` u16s; on success `len` is the written length without the NUL.
        unsafe {
            QueryFullProcessImageNameW(
                self.0,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
        }
        .ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(
            &buf[..len as usize],
        )))
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenProcess and is closed only here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// AUMID if packaged, else the lowercase exe file name (with a Chromium fork's ProductName,
/// `exe_identity`). `None` if the process is gone or inaccessible.
pub(crate) fn identity(pid: u32) -> Option<AppIdentity> {
    let process = Process::open(pid)?;
    if let Some(key) = process.aumid() {
        return Some(AppIdentity::Aumid { key });
    }
    let path = process.image_path()?;
    let file_name = path.file_name()?.to_string_lossy();
    Some(exe_identity(&file_name, || product_name(&path)))
}

/// The exe's version-info ProductName, read once per path: `identity` runs every tick for the
/// foreground and each minimized window. Only `chrome.exe` paths are ever asked, so the map stays
/// a handful of entries.
pub(crate) fn product_name(path: &Path) -> Option<String> {
    static NAMES: LazyLock<Mutex<HashMap<PathBuf, Option<String>>>> = LazyLock::new(Mutex::default);
    NAMES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(path.to_owned())
        .or_insert_with(|| crate::version::string(path, "ProductName"))
        .clone()
}

pub(crate) fn image_path(pid: u32) -> Option<PathBuf> {
    Process::open(pid)?.image_path()
}

/// PID → parent PID for every running process. Parent PIDs can be stale (the parent exited and
/// the PID was reused), so walks over it must be bounded.
pub(crate) fn parent_map() -> HashMap<u32, u32> {
    let mut parents = HashMap::new();
    // SAFETY: plain FFI call; the snapshot handle is closed below.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return parents;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: `entry.dwSize` is set as the API requires; `snapshot` is a valid snapshot handle.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    // SAFETY: the snapshot handle is valid and closed exactly once.
    let _ = unsafe { CloseHandle(snapshot) };
    parents
}

/// True if `pid` descends from `ancestor` (bounded, since stale parent PIDs can form cycles).
pub(crate) fn descends_from(parents: &HashMap<u32, u32>, pid: u32, ancestor: u32) -> bool {
    let mut current = pid;
    for _ in 0..32 {
        match parents.get(&current) {
            Some(&parent) if parent == ancestor => return true,
            Some(&parent) if parent != 0 && parent != current => current = parent,
            _ => return false,
        }
    }
    false
}

/// A NUL-terminated (or full) UTF-16 buffer as a String.
pub(crate) fn from_wide_nul(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
