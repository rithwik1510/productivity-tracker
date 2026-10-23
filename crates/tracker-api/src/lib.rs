//! Frozen shared contract between `tracker`, `tracker-win` and the shell: API types, the
//! `Sampler` trait, `Control` messages, and the few helpers all of them must agree on.
//! Platform-free and tiny. Changes need orchestrator approval.

mod control;
mod model;
mod sample;

use std::io;
use std::path::{Path, PathBuf};

use url::{Host, Url};

pub use control::Control;
pub use model::{
    ApiError, AppIdentity, Appearance, CatalogApp, Category, DayItem, DaySummary, DayTotals,
    ExportFormat, NowTracking, Settings, Share, ShareRange, Site, TargetRef, TrackerStatus,
    WeekSummary, YearHeatmap,
};
pub use sample::{BrowserView, Foreground, Sample, Sampler};

/// Folder name under `%LOCALAPPDATA%`. Renaming the product is this edit plus `tauri.conf.json`.
pub const APP_DIR_NAME: &str = "ProductivityTracker";

/// Subfolder of the data dir holding app icon PNGs. The shell allows exactly this folder in the asset scope.
pub const ICONS_DIR_NAME: &str = "icons";

/// `%LOCALAPPDATA%\<APP_DIR_NAME>`, created if missing. Every path the app uses derives from this,
/// never from Tauri's path resolver.
pub fn data_dir() -> io::Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    let dir = PathBuf::from(base).join(APP_DIR_NAME);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `<data_dir>/icons/<fnv1a64("kind:key") as 16 hex digits>.png`. The catalog writes icons here,
/// the store reports them in `iconPath`, so the naming scheme has exactly one owner.
pub fn icon_path(data_dir: &Path, identity: &AppIdentity) -> PathBuf {
    let (kind, key) = match identity {
        AppIdentity::Exe { key } => ("exe", key),
        AppIdentity::Aumid { key } => ("aumid", key),
    };
    // FNV-1a 64 rather than std's DefaultHasher, whose output may change between Rust releases.
    let hash = format!("{kind}:{key}")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    data_dir
        .join(ICONS_DIR_NAME)
        .join(format!("{hash:016x}.png"))
}

/// Turns a pasted URL, a bare domain or omnibox text into the stored domain form: lowercase host,
/// no `www.`, no port, no trailing dot. Only http(s) hosts with at least one dot are sites.
/// The store (on `upsert_site`) and the engine (on browser domains) both use it.
pub fn normalize_domain(input: &str) -> Result<String, ApiError> {
    let input = input.trim();
    let invalid = || ApiError {
        message: format!("\"{input}\" isn't a web address. Try something like youtube.com."),
    };
    // Omnibox text and typed domains usually have no scheme. Only treat "x://" as a scheme when
    // "x" looks like one, so "site.com/r?u=https://other" still parses as site.com.
    let has_scheme = input.split_once("://").is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    let url = if has_scheme {
        Url::parse(input)
    } else {
        Url::parse(&format!("https://{input}"))
    }
    .map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid());
    }
    // The url crate already lowercased (and punycoded) the host and split off the port.
    let Some(Host::Domain(host)) = url.host() else {
        return Err(invalid());
    };
    let host = host.trim_end_matches('.');
    let host = host.strip_prefix("www.").unwrap_or(host);
    if !host.contains('.') {
        return Err(invalid());
    }
    Ok(host.to_owned())
}

/// A browser the tracker knows how to read. `uia` browsers get the UI Automation omnibox read
/// (ADR-0002); the rest use the window title only.
#[derive(Debug)]
pub struct KnownBrowser {
    /// Lowercase exe file name, as in `AppIdentity::Exe`.
    pub exe: &'static str,
    pub uia: bool,
}

/// The one list the engine, the catalog and the UI's `isBrowser` flag all derive from.
///
/// `uia` only for browsers built on Chrome's own native UI, whose window → … → ToolBar → Edit
/// shape the omnibox walk expects. A browser whose UI differs (Vivaldi draws its UI as a web page,
/// Opera has its own) or isn't Chromium (the Firefox family) is title-only. A wrong guess fails
/// safe either way: no toolbar found means the title fallback, re-walked every few seconds.
/// Chrome's exe name also covers Chromium, ungoogled-chromium and Supermium.
pub const KNOWN_BROWSERS: &[KnownBrowser] = &[
    KnownBrowser {
        exe: "chrome.exe",
        uia: true,
    },
    KnownBrowser {
        exe: "msedge.exe",
        uia: true,
    },
    KnownBrowser {
        exe: "brave.exe",
        uia: true,
    },
    KnownBrowser {
        exe: "thorium.exe",
        uia: true,
    },
    // Perplexity's Comet.
    KnownBrowser {
        exe: "comet.exe",
        uia: true,
    },
    KnownBrowser {
        exe: "vivaldi.exe",
        uia: false,
    },
    KnownBrowser {
        exe: "opera.exe",
        uia: false,
    },
    KnownBrowser {
        exe: "firefox.exe",
        uia: false,
    },
    KnownBrowser {
        exe: "zen.exe",
        uia: false,
    },
    KnownBrowser {
        exe: "librewolf.exe",
        uia: false,
    },
    KnownBrowser {
        exe: "waterfox.exe",
        uia: false,
    },
    KnownBrowser {
        exe: "floorp.exe",
        uia: false,
    },
];

fn known_browser(identity: &AppIdentity) -> Option<&'static KnownBrowser> {
    match identity {
        AppIdentity::Exe { key } => KNOWN_BROWSERS.iter().find(|b| b.exe == key),
        // Packaged browsers aren't supported in v1.
        AppIdentity::Aumid { .. } => None,
    }
}

pub fn is_browser(identity: &AppIdentity) -> bool {
    known_browser(identity).is_some()
}

/// True if the omnibox should be read over UI Automation for this app.
pub fn uia_browser(identity: &AppIdentity) -> bool {
    known_browser(identity).is_some_and(|b| b.uia)
}
