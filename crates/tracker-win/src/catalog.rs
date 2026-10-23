//! Installed (`shell:AppsFolder`) and running apps, with icons (plan section 5 "Catalog").
//!
//! Every entry gets the same identity key the sampler would report for it (ADR-0003): packaged
//! apps by AUMID, everything else by the lowercase exe file name of the shortcut's target (plus the
//! ProductName for a Chromium fork's `chrome.exe`, `tracker_api::exe_identity`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use tracker_api::{
    AppIdentity, CatalogApp, ICONS_DIR_NAME, exe_identity, is_browser, watched_by_default,
};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Storage::EnhancedStorage::{PKEY_Link_Arguments, PKEY_Link_TargetParsingPath};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoFreeUnusedLibrariesEx, CoTaskMemFree,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2, KF_FLAG_DEFAULT,
    SHCreateItemFromParsingName, SHGetKnownFolderItem, SIGDN, SIGDN_NORMALDISPLAY,
    SIGDN_PARENTRELATIVEPARSING,
};
use windows::core::{GUID, HSTRING, Interface, PWSTR};

use crate::com::Apartment;
use crate::{foreground, icons, process, version};

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("Windows API error: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("icon file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("icon encoding error: {0}")]
    Png(#[from] png::EncodingError),
    #[error("icon error: {0}")]
    Icon(&'static str),
    #[error("the catalog thread stopped unexpectedly")]
    Thread,
}

/// `System.AppUserModel.PackageFamilyName`; non-empty only for packaged apps. Missing from the
/// `windows` crate's generated keys.
const PKEY_PACKAGE_FAMILY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 17,
};

/// Shortcut targets that are never apps: browser PWA launchers, installers, uninstallers.
fn skipped(exe: &str) -> bool {
    matches!(exe, "chrome_proxy.exe" | "msedge_proxy.exe" | "msiexec.exe")
        // Covers both `unins*.exe` and `uninstall*.exe`.
        || exe.starts_with("unins")
        || !exe.ends_with(".exe")
}

/// Set while a listing's thread extracts icons, so two listings never write the same file.
static EXTRACTING: AtomicBool = AtomicBool::new(false);

/// Every installed or running app, merged by identity, with `category: None` (the shell merges in
/// the tracked apps). `is_running` comes from this call's window enumeration.
///
/// Returns before extracting icons, which takes seconds the first time (tens of ms each): an
/// app's `icon_path` is set only once its icon is on disk, at `tracker_api::icon_path`. The
/// missing ones are extracted afterwards on the same thread, then `icons_done` runs with how many
/// were written (0 when another listing was already extracting), so the caller can list again.
/// Runs its own STA thread (shell extensions need one), so it can be called from any thread.
pub fn list(
    data_dir: &Path,
    icons_done: impl FnOnce(usize) + Send + 'static,
) -> Result<Vec<CatalogApp>, CatalogError> {
    let data_dir = data_dir.to_path_buf();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("catalog".into())
        .spawn(move || {
            let _com = Apartment::enter(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            let written = match collect(&data_dir) {
                Ok((apps, missing)) => {
                    let _ = tx.send(Ok(apps));
                    extract_icons(&data_dir, missing)
                }
                Err(err) => {
                    let _ = tx.send(Err(err));
                    0
                }
            };
            // Every COM object is released by now. Unload the shell extension DLLs the listing
            // pulled in right away, rather than keeping them mapped in a process that runs all day.
            // SAFETY: COM is still initialized on this thread (`_com` drops after this).
            unsafe { CoFreeUnusedLibrariesEx(0, None) };
            icons_done(written);
        })?;
    rx.recv().map_err(|_| CatalogError::Thread)?
}

/// Extracts the icons a listing found missing; how many were written.
fn extract_icons(data_dir: &Path, missing: Vec<(AppIdentity, IShellItem)>) -> usize {
    if missing.is_empty() || EXTRACTING.swap(true, Ordering::Acquire) {
        return 0;
    }
    let written = missing
        .iter()
        .filter(|(identity, item)| icons::extract_to(data_dir, identity, item))
        .count();
    EXTRACTING.store(false, Ordering::Release);
    written
}

/// An app found in the Apps folder or among running windows, before its icon is resolved.
struct Entry {
    identity: AppIdentity,
    name: String,
    /// The shell item its icon comes from.
    icon: Option<IShellItem>,
    rank: Rank,
}

/// Which entry names an identity when several share it (lowest wins). A shortcut that passes
/// arguments is a variant of its exe ("Developer PowerShell", a cmd script, a folder opened in
/// Explorer), so a plain shortcut or the running window names the app better.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Shortcut,
    Running,
    ShortcutWithArguments,
}

/// The apps, and the shell items of those whose icon isn't on disk yet.
type Listing = (Vec<CatalogApp>, Vec<(AppIdentity, IShellItem)>);

fn collect(data_dir: &Path) -> Result<Listing, CatalogError> {
    std::fs::create_dir_all(data_dir.join(ICONS_DIR_NAME))?;
    let mut entries = installed()?;
    let running = running();
    // Dedup below keeps the best-named entry, which may be the shortcut, so remember who runs.
    let running_ids: HashSet<AppIdentity> = running.iter().map(|e| e.identity.clone()).collect();
    entries.extend(running);
    // Stable, so Apps folder order breaks ties.
    entries.sort_by_key(|entry| entry.rank);
    let mut seen = HashSet::new();
    let mut missing = Vec::new();
    let mut apps: Vec<_> = entries
        .into_iter()
        .filter(|entry| seen.insert(entry.identity.clone()))
        .map(|entry| {
            let icon_path = icons::cached(data_dir, &entry.identity);
            if icon_path.is_none()
                && let Some(item) = entry.icon
            {
                missing.push((entry.identity.clone(), item));
            }
            CatalogApp {
                icon_path,
                is_browser: is_browser(&entry.identity),
                is_running: running_ids.contains(&entry.identity),
                watch: watched_by_default(&entry.identity),
                identity: entry.identity,
                name: entry.name,
                category: None,
            }
        })
        .collect();
    apps.sort_by_cached_key(|app| app.name.to_lowercase());
    Ok((apps, missing))
}

fn installed() -> Result<Vec<Entry>, CatalogError> {
    // SAFETY: COM calls on this thread's STA.
    let items: IEnumShellItems = unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
        folder.BindToHandler(None, &BHID_EnumItems)?
    };
    let mut entries = Vec::new();
    loop {
        let mut batch = [None];
        // SAFETY: `batch` has room for one item, as its length tells the API; a null fetched
        // count is allowed when asking for one. An empty slot means the end.
        unsafe { items.Next(&mut batch, None) }?;
        let [Some(item)] = batch else { break };
        match classify(item) {
            Ok(Some(entry)) => entries.push(entry),
            Ok(None) => {}
            Err(err) => tracing::debug!("skipping Apps folder item: {err}"),
        }
    }
    Ok(entries)
}

/// The identity of an Apps folder item, or `None` for items that aren't trackable apps.
fn classify(item: IShellItem) -> Result<Option<Entry>, CatalogError> {
    let props: IShellItem2 = item.cast()?;
    let name = display_name(&item, SIGDN_NORMALDISPLAY)?;
    // For Apps folder items this is the AUMID (packaged) or a path / known-folder path (Win32).
    let parsing = display_name(&item, SIGDN_PARENTRELATIVEPARSING)?;
    if !string_property(&props, &PKEY_PACKAGE_FAMILY_NAME).is_empty() {
        return Ok(Some(Entry {
            identity: AppIdentity::Aumid { key: parsing },
            name,
            icon: Some(item),
            rank: Rank::Shortcut,
        }));
    }
    // Win32: the shortcut's target exe. Any explicit AUMID on the shortcut (Chrome sets one) is
    // ignored, because the sampler identifies the running process by exe name (and a fork of
    // Chrome by its ProductName too, read from the target here).
    let target = string_property(&props, &PKEY_Link_TargetParsingPath);
    let mut exe = if target.is_empty() {
        // Known-folder entries like `{GUID}\cmd.exe` have no shortcut target.
        file_name(&parsing)
    } else {
        file_name(&target)
    };
    let arguments = string_property(&props, &PKEY_Link_Arguments);
    let mut rank = if arguments.trim().is_empty() {
        Rank::Shortcut
    } else {
        Rank::ShortcutWithArguments
    };
    if exe == "update.exe" {
        // Squirrel installers (Discord, Slack, ...) launch `Update.exe --processStart X.exe`.
        let Some(started) = process_start(&arguments) else {
            tracing::debug!(name, "skipped: Update.exe without --processStart");
            return Ok(None);
        };
        exe = started;
        rank = Rank::Shortcut;
    }
    if skipped(&exe) {
        tracing::debug!(name, exe, "skipped");
        return Ok(None);
    }
    let name = match rank {
        // "Install Additional Tools for Node.js" shouldn't become the name of cmd.exe.
        Rank::ShortcutWithArguments => {
            version::string(Path::new(&target), "FileDescription").unwrap_or(name)
        }
        _ => name,
    };
    Ok(Some(Entry {
        identity: exe_identity(&exe, || process::product_name(Path::new(&target))),
        name,
        icon: Some(item),
        rank,
    }))
}

/// Lowercase file name of a Windows path.
fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .trim()
        .to_lowercase()
}

/// The `X.exe` in `--processStart X.exe` or `--processStart "X.exe"`, lowercased.
fn process_start(args: &str) -> Option<String> {
    let (_, rest) = args.split_once("--processStart")?;
    let rest = rest.trim_start();
    let exe = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next()?,
        None => rest.split_whitespace().next()?,
    };
    (!exe.is_empty()).then(|| exe.to_lowercase())
}

/// Running app windows not found in the Apps folder (portable apps, apps without a shortcut).
fn running() -> Vec<Entry> {
    let own_pid = std::process::id();
    let mut entries = Vec::new();
    for hwnd in foreground::top_level_windows() {
        if !foreground::is_app_window(hwnd) {
            continue;
        }
        let Some(pid) = foreground::app_pid(hwnd).filter(|&pid| pid != own_pid) else {
            continue;
        };
        let Some(identity) = process::identity(pid) else {
            continue;
        };
        // A UWP frame whose app process couldn't be resolved isn't an app of its own.
        if matches!(&identity, AppIdentity::Exe { key } if key == "applicationframehost.exe") {
            continue;
        }
        let image = process::image_path(pid);
        let (name, icon) = match &identity {
            AppIdentity::Aumid { key } => (
                image
                    .as_deref()
                    .and_then(|path| version::string(path, "FileDescription")),
                shell_item(&format!("shell:AppsFolder\\{key}")),
            ),
            AppIdentity::Exe { .. } => (
                image
                    .as_deref()
                    .and_then(|path| version::string(path, "FileDescription")),
                image
                    .as_deref()
                    .and_then(|path| shell_item(&path.to_string_lossy())),
            ),
        };
        let name = name.unwrap_or_else(|| exe_stem(&identity, image.as_ref()));
        entries.push(Entry {
            identity,
            name,
            icon,
            rank: Rank::Running,
        });
    }
    entries
}

fn exe_stem(identity: &AppIdentity, image: Option<&PathBuf>) -> String {
    image
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| match identity {
            AppIdentity::Exe { key } | AppIdentity::Aumid { key } => key.clone(),
        })
}

fn shell_item(parsing_name: &str) -> Option<IShellItem> {
    // SAFETY: COM call on this thread's STA with a valid, NUL-terminated HSTRING.
    unsafe { SHCreateItemFromParsingName(&HSTRING::from(parsing_name), None) }.ok()
}

fn display_name(item: &IShellItem, form: SIGDN) -> Result<String, CatalogError> {
    // SAFETY: COM call on this thread's STA; the returned string is freed by `take_co_string`.
    let raw = unsafe { item.GetDisplayName(form) }?;
    Ok(take_co_string(raw))
}

/// A string property, or "" if the item doesn't have it.
fn string_property(item: &IShellItem2, key: &PROPERTYKEY) -> String {
    // SAFETY: COM call on this thread's STA; the returned string is freed by `take_co_string`.
    unsafe { item.GetString(key) }
        .map(take_co_string)
        .unwrap_or_default()
}

/// Copies and frees a `CoTaskMemAlloc`ed string.
fn take_co_string(raw: PWSTR) -> String {
    // SAFETY: `raw` is a NUL-terminated string the shell allocated with CoTaskMemAlloc and
    // handed to us; it is read once and freed exactly once.
    unsafe {
        let text = raw.to_string().unwrap_or_default();
        CoTaskMemFree(Some(raw.0.cast_const().cast()));
        text
    }
}
