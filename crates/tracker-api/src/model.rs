//! API and domain types shared by the store, the shell and the UI (plan section 3).
//!
//! Everything here crosses IPC, so: camelCase on the wire, durations are `u32` seconds,
//! ids are `u32`, Days are `"YYYY-MM-DD"` strings, and no timestamps.

use serde::{Deserialize, Serialize};
use specta::Type;

/// One of the two fixed buckets. Serialized as `"productive"` / `"entertainment"`,
/// the same text the store keeps in its `category` columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Productive,
    Entertainment,
}

/// How the window is drawn: Acrylic behind it (`"glass"`) or opaque (`"solid"`). Stored in
/// `settings.appearance`. Glass still shows only while `glass_available` and the window is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    Glass,
    Solid,
}

/// How an app is recognised over time (ADR-0003). Wire shape: `{ kind: "exe" | "aumid", key }`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AppIdentity {
    /// Lowercase exe file name, e.g. `discord.exe`; a Chromium fork that ships `chrome.exe` adds
    /// its product name, e.g. `chrome.exe|helium` (`exe_identity`).
    Exe { key: String },
    /// AppUserModelID of a packaged app.
    Aumid { key: String },
}

/// A tracked thing: an app row or a site row. Wire shape: `{ kind: "app" | "site", id }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TargetRef {
    App { id: u32 },
    Site { id: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    pub id: u32,
    pub domain: String,
    /// Title fallback keyword, matched when the browser gives no reliable domain.
    pub keyword: Option<String>,
    /// `None` = not tracked; the row is kept for history.
    pub category: Option<Category>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub daily_goal_minutes: u32,
    /// Not stored: the autostart plugin is the source of truth (in-memory in debug builds).
    pub launch_at_login: bool,
    pub onboarded: bool,
    pub appearance: Appearance,
    /// "Count as away after": minutes without evidence of presence before the user is Away and
    /// the time is trimmed back to the last evidence. One of 2, 5, 10 or 15; 5 by default.
    pub away_after_minutes: u32,
    /// "Keep counting minimized apps": waiting credit (ADR-0007) on or off. Off on a new install
    /// and after Delete all; an install onboarded before the setting existed keeps it on.
    pub waiting_credit: bool,
    /// "Check for updates automatically" (ADR-0009): on by default, existing installs included.
    pub auto_update: bool,
}

/// What `export_data` writes: daily totals as CSV, or everything as JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Csv,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DayTotals {
    pub day: String,
    pub productive_secs: u32,
    pub entertainment_secs: u32,
}

/// One row of a Day or Share list, keyed by (target, category) (ADR-0005). Lists are sorted by secs desc.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DayItem {
    pub target: TargetRef,
    pub name: String,
    /// Absolute path under `<data_dir>/icons` (see `icon_path`); the UI uses `convertFileSrc`. Sites never have one.
    pub icon_path: Option<String>,
    pub category: Category,
    pub secs: u32,
    /// The part of `secs` that came from waiting credit (ADR-0007): the app minimized while the
    /// foreground wasn't Productive. Floored on its own, so it never exceeds `secs`. Always 0 for
    /// sites.
    pub minimized_secs: u32,
    /// Lets the UI label the item "Chrome · other sites".
    pub is_browser: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    pub totals: DayTotals,
    pub goal_secs: u32,
    pub is_today: bool,
    pub items: Vec<DayItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WeekSummary {
    /// The Monday the week starts on.
    pub start: String,
    pub is_current: bool,
    /// Always 7 entries, Monday first.
    pub days: Vec<DayTotals>,
    pub goal_secs: u32,
}

/// Current week, current month, or all time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ShareRange {
    Week,
    Month,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub productive_secs: u32,
    pub entertainment_secs: u32,
    pub items: Vec<DayItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct YearHeatmap {
    pub year: i32,
    pub is_current: bool,
    pub goal_secs: u32,
    /// Every Day of the year, zeros included.
    pub days: Vec<DayTotals>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogApp {
    pub identity: AppIdentity,
    pub name: String,
    pub icon_path: Option<String>,
    pub is_browser: bool,
    /// `None` = not tracked.
    pub category: Option<Category>,
    /// Has an app window open right now. Computed per call; nothing watches or stores it.
    pub is_running: bool,
    /// "Counts while I watch" (ADR-0008): the stored setting of an app that has a row, else
    /// whether tracking it would turn it on (`watched_by_default`). It applies only while the app
    /// is Productive, and never to a browser.
    pub watch: bool,
}

/// What the tracker is doing as of its last tick (`get_status`), for the Day view's status line
/// and the tray. Exactly one state holds; when several are true the earlier variant wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum TrackerStatus {
    /// A database write failed. Nothing more is saved until one succeeds, which is retried every
    /// tick; the time in between is lost, never invented.
    SaveFailed,
    /// Paused from the tray; `get_pause` says until when.
    Paused,
    /// The user isn't there: no evidence for the away threshold, the sound cap, lock, sleep.
    Away {
        /// Counted time the Away trim removed when the stretch ended, which is why a total just
        /// dropped. 0 when nothing counted past the cutoff.
        #[serde(rename = "removedSecs")]
        removed_secs: u32,
    },
    /// Counting toward this target.
    Tracking(NowTracking),
    /// There, but nothing tracked is in front and no app waits in (an untracked app, a private
    /// window, the desktop, this app's own window).
    Untracked,
}

/// What the tracker counts this tick (`TrackerStatus::Tracking`). For a site, `name` is the
/// domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NowTracking {
    pub name: String,
    /// As in `DayItem`: an absolute path under `<data_dir>/icons`, sites never have one.
    pub icon_path: Option<String>,
    pub category: Category,
    pub is_browser: bool,
    /// The time goes to this app as waiting credit (ADR-0007): it is minimized while the
    /// foreground isn't Productive. Always false for sites.
    pub waiting: bool,
    /// It counts only because it is a watched app in front (ADR-0008): there has been no input or
    /// sound for longer than "Count as away after", but for less than 30 minutes.
    pub watching: bool,
}

/// The only error type that crosses IPC. With `ErrorHandlingMode::Throw` the TS promise rejects with it.
#[derive(Debug, Clone, Serialize, Deserialize, Type, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct ApiError {
    pub message: String,
}
