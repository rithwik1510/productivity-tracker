//! What the platform adapter reports once per tick (plan section 4). Never crosses IPC;
//! `Serialize` exists so `tracker-probe` can print samples as JSON lines.

use serde::Serialize;

use crate::AppIdentity;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// Wall clock, UTC milliseconds.
    pub now_ms: i64,
    /// Monotonic milliseconds; u64 so tests can fake it. The presence ring's
    /// "at least 2 s earlier" is measured on this clock, not on `now_ms`.
    pub mono_ms: u64,
    /// Session locked/disconnected or display off.
    pub away_signal: bool,
    /// Milliseconds since the last keyboard/mouse input.
    pub idle_ms: u32,
    /// True once after an unlock (counts as input).
    pub input_event: bool,
    pub foreground: Option<Foreground>,
    /// Apps with a minimized (iconic) app window this tick, each listed once, by the same window
    /// filter and identity rules as the foreground. Our own process and private browser windows
    /// are never in it. Feeds waiting credit (ADR-0007); the foreground app may be in it too.
    pub minimized: Vec<AppIdentity>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Foreground {
    pub identity: AppIdentity,
    /// The foreground window belongs to this app; it never counts.
    pub own_process: bool,
    pub audible: bool,
    /// `Some` only for known browsers (see `KNOWN_BROWSERS`).
    pub browser: Option<BrowserView>,
    /// Window title, used for the site keyword fallback. Kept in memory only, never stored.
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserView {
    /// Incognito / InPrivate / Private Browsing: nothing counts.
    pub private: bool,
    /// Normalized host from the address bar; `None` = no reliable domain this tick.
    pub domain: Option<String>,
}

/// The platform adapter. Need not be `Send`: the runtime builds it on the poll thread
/// (COM objects are `!Send`).
pub trait Sampler {
    fn sample(&mut self) -> Sample;
}
