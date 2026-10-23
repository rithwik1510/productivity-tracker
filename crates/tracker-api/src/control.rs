//! Messages from the shell to the runtime's poll thread (plan section 4). Config mutations go
//! through here so they apply inside the runtime and take effect on the next tick.

use std::sync::mpsc::Sender;

use crate::{ApiError, AppIdentity, Appearance, Category, Site};

/// Launch at login isn't stored (the autostart plugin is the source of truth), so it has no message.
#[derive(Debug)]
pub enum Control {
    SetAppCategory {
        identity: AppIdentity,
        name: String,
        category: Option<Category>,
        reply: Sender<Result<(), ApiError>>,
    },
    /// "Counts while I watch" (ADR-0008) for an app that has a row (one that is or was tracked).
    SetAppWatch {
        identity: AppIdentity,
        watch: bool,
        reply: Sender<Result<(), ApiError>>,
    },
    UpsertSite {
        domain: String,
        keyword: Option<String>,
        category: Option<Category>,
        reply: Sender<Result<Site, ApiError>>,
    },
    SetDailyGoal {
        minutes: u32,
        reply: Sender<Result<(), ApiError>>,
    },
    SetAppearance {
        appearance: Appearance,
        reply: Sender<Result<(), ApiError>>,
    },
    CompleteOnboarding {
        reply: Sender<Result<(), ApiError>>,
    },
    /// "Count as away after": 2, 5, 10 or 15 minutes.
    SetAwayAfter {
        minutes: u32,
        reply: Sender<Result<(), ApiError>>,
    },
    /// "Keep counting minimized apps" (waiting credit, ADR-0007).
    SetWaitingCredit {
        enabled: bool,
        reply: Sender<Result<(), ApiError>>,
    },
    /// "Check for updates automatically" (ADR-0009). Only stored: the shell's update loop reads it
    /// before each check.
    SetAutoUpdate {
        enabled: bool,
        reply: Sender<Result<(), ApiError>>,
    },
    /// Pauses tracking until `until_ms` (UTC epoch ms), ending the current stretch as a shutdown
    /// would; `None` resumes. Stored, so a restart keeps the pause.
    SetPause {
        until_ms: Option<i64>,
        reply: Sender<Result<(), ApiError>>,
    },
    /// Wipes history, tracked apps and sites, and settings, back to first run. The open row goes
    /// with it; tracking carries on from the next tick with nothing tracked.
    DeleteAllData {
        reply: Sender<Result<(), ApiError>>,
    },
    /// Ends the stretch, flushes, then replies.
    Shutdown {
        reply: Sender<()>,
    },
}
