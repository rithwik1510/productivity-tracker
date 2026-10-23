//! IPC commands (plan section 3). The arguments and return types are frozen: they generate
//! `src/api/bindings.ts`. The `AppHandle` parameter is left out of the bindings.
//!
//! Every body runs in `spawn_blocking`: reply channels, SQLite and the catalog's shell calls all
//! block, and a blocked tokio worker would stall every other command.

use std::collections::HashMap;

use jiff::Zoned;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tracker::Reader;
use tracker_api::{
    ApiError, AppIdentity, Appearance, CatalogApp, Category, Control, DaySummary, ExportFormat,
    Settings, Share, ShareRange, Site, TrackerStatus, WeekSummary, YearHeatmap,
};
use tracker_win::catalog;

use crate::state::AppState;
use crate::{glass, tray, window};

/// Runs `body` on the blocking pool with the app state.
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    body: impl FnOnce(&AppHandle, &AppState) -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tauri::async_runtime::spawn_blocking(move || body(&app, &app.state::<AppState>()))
        .await
        .map_err(|e| ApiError {
            message: format!("The command stopped unexpectedly: {e}"),
        })?
}

/// `None` = the current tracking Day.
#[tauri::command]
#[specta::specta]
pub async fn get_day(app: AppHandle, day: Option<String>) -> Result<DaySummary, ApiError> {
    blocking(app, move |_, s| s.read(|r| r.day(day.as_deref()))).await
}

/// `None` = the current week; a non-Monday start is an error.
#[tauri::command]
#[specta::specta]
pub async fn get_week(app: AppHandle, start: Option<String>) -> Result<WeekSummary, ApiError> {
    blocking(app, move |_, s| s.read(|r| r.week(start.as_deref()))).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_share(app: AppHandle, range: ShareRange) -> Result<Share, ApiError> {
    blocking(app, move |_, s| s.read(|r| r.share(range))).await
}

/// `None` = the year of the current tracking Day.
#[tauri::command]
#[specta::specta]
pub async fn get_year(app: AppHandle, year: Option<i32>) -> Result<YearHeatmap, ApiError> {
    blocking(app, move |_, s| s.read(|r| r.year(year))).await
}

/// Emitted when a listing's missing icons are on disk, so the UI lists again to show them.
const CATALOG_ICONS: &str = "catalog-icons";

/// Installed ∪ running ∪ every app row with a category, merged by identity. Icons not yet
/// extracted come without `iconPath`; `catalog-icons` follows once they're written.
#[tauri::command]
#[specta::specta]
pub async fn list_catalog(app: AppHandle) -> Result<Vec<CatalogApp>, ApiError> {
    blocking(app, |app, s| {
        let events = app.clone();
        // About a second (the Apps folder), so it runs before taking the reader lock.
        let mut apps = catalog::list(&s.data_dir, move |written| {
            if written > 0
                && let Err(e) = events.emit(CATALOG_ICONS, ())
            {
                tracing::warn!("couldn't tell the window about new icons: {e}");
            }
        })
        .map_err(|e| ApiError {
            message: format!("Couldn't list your apps: {e}"),
        })?;
        let mut stored: HashMap<AppIdentity, CatalogApp> = s
            .read(Reader::stored_apps)?
            .into_iter()
            .map(|app| (app.identity.clone(), app))
            .collect();
        for app in &mut apps {
            if let Some(row) = stored.remove(&app.identity) {
                app.category = row.category;
                app.watch = row.watch;
            }
        }
        // Tracked apps that are neither installed nor running keep their stored name.
        apps.extend(stored.into_values().filter(|row| row.category.is_some()));
        Ok(apps)
    })
    .await
}

/// `category: None` stops tracking the app; its history stays.
#[tauri::command]
#[specta::specta]
pub async fn set_app_category(
    app: AppHandle,
    identity: AppIdentity,
    name: String,
    category: Option<Category>,
) -> Result<(), ApiError> {
    blocking(app, move |_, s| {
        s.request(|reply| Control::SetAppCategory {
            identity,
            name,
            category,
            reply,
        })
    })
    .await
}

/// "Counts while I watch" for an app that is or was tracked.
#[tauri::command]
#[specta::specta]
pub async fn set_app_watch(
    app: AppHandle,
    identity: AppIdentity,
    watch: bool,
) -> Result<(), ApiError> {
    blocking(app, move |_, s| {
        s.request(|reply| Control::SetAppWatch {
            identity,
            watch,
            reply,
        })
    })
    .await
}

/// Every site row, tracked or not, ordered by domain.
#[tauri::command]
#[specta::specta]
pub async fn list_sites(app: AppHandle) -> Result<Vec<Site>, ApiError> {
    blocking(app, |_, s| s.read(Reader::sites)).await
}

/// The domain is normalized. `keyword: Some(k)` may list several, comma-separated, each at least
/// 3 chars after trim; `None` on create defaults to the domain label if that has at least 3 chars.
// (Doc comments land in the bindings, which are frozen.) `None` on update keeps the keyword.
#[tauri::command]
#[specta::specta]
pub async fn upsert_site(
    app: AppHandle,
    domain: String,
    keyword: Option<String>,
    category: Option<Category>,
) -> Result<Site, ApiError> {
    blocking(app, move |_, s| {
        s.request(|reply| Control::UpsertSite {
            domain,
            keyword,
            category,
            reply,
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings(app: AppHandle) -> Result<Settings, ApiError> {
    blocking(app, |app, s| {
        Ok(Settings {
            launch_at_login: s.launch_at_login(app)?,
            ..s.read(Reader::settings)?
        })
    })
    .await
}

/// 30..=960 minutes.
#[tauri::command]
#[specta::specta]
pub async fn set_daily_goal(app: AppHandle, minutes: u32) -> Result<(), ApiError> {
    blocking(app, move |_, s| {
        s.request(|reply| Control::SetDailyGoal { minutes, reply })
    })
    .await
}

/// Release: the autostart plugin. Debug: in memory only.
#[tauri::command]
#[specta::specta]
pub async fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<(), ApiError> {
    blocking(app, move |app, s| s.set_launch_at_login(app, enabled)).await
}

/// Sets onboarded; release builds also enable autostart.
#[tauri::command]
#[specta::specta]
pub async fn complete_onboarding(app: AppHandle) -> Result<(), ApiError> {
    blocking(app, |app, s| {
        s.request(|reply| Control::CompleteOnboarding { reply })?;
        // Onboarding stands even if the Run key can't be written: Settings reads the real state
        // and offers the toggle again, while failing here would strand the user in FirstRun.
        if let Err(e) = s.set_launch_at_login(app, true) {
            tracing::warn!("couldn't enable launch at login: {e}");
        }
        Ok(())
    })
    .await
}

/// Stored, then applied to the open window: Acrylic for Glass, no effects for Solid.
#[tauri::command]
#[specta::specta]
pub async fn set_appearance(app: AppHandle, appearance: Appearance) -> Result<(), ApiError> {
    blocking(app, move |app, s| {
        s.request(|reply| Control::SetAppearance { appearance, reply })?;
        window::apply_appearance(app, appearance).map_err(|e| ApiError {
            message: format!("Saved, but the window couldn't change its look: {e}"),
        })
    })
    .await
}

/// What the tracker is doing as of its last tick, for the status line; null before the first
/// tick. Never queries the database.
#[tauri::command]
#[specta::specta]
pub async fn get_status(app: AppHandle) -> Result<Option<TrackerStatus>, ApiError> {
    blocking(app, |_, s| Ok(s.runtime.status())).await
}

/// False if Windows "Transparency effects" is off.
#[tauri::command]
#[specta::specta]
pub async fn glass_available(app: AppHandle) -> Result<bool, ApiError> {
    blocking(app, |_, _| Ok(glass::available())).await
}

/// 2, 5, 10 or 15 minutes.
#[tauri::command]
#[specta::specta]
pub async fn set_away_after(app: AppHandle, minutes: u32) -> Result<(), ApiError> {
    blocking(app, move |_, s| {
        s.request(|reply| Control::SetAwayAfter { minutes, reply })
    })
    .await
}

/// Waiting credit (ADR-0007) on or off.
#[tauri::command]
#[specta::specta]
pub async fn set_waiting_credit(app: AppHandle, enabled: bool) -> Result<(), ApiError> {
    blocking(app, move |_, s| {
        s.request(|reply| Control::SetWaitingCredit { enabled, reply })
    })
    .await
}

/// When the current pause ends, as a local clock time ("3:40 PM"); null when not paused. Pausing
/// is the tray's; the window only shows it and resumes.
#[tauri::command]
#[specta::specta]
pub async fn get_pause(app: AppHandle) -> Result<Option<String>, ApiError> {
    blocking(app, |_, s| {
        Ok(s.read(Reader::paused_until_ms)?.map(tray::clock_time))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn resume_tracking(app: AppHandle) -> Result<(), ApiError> {
    blocking(app, |app, _| tray::set_pause(app, None)).await
}

/// Asks where to save, then writes the export there: daily totals as CSV, or everything as JSON.
/// Returns the saved file's path, or null if the save dialog was cancelled.
#[tauri::command]
#[specta::specta]
pub async fn export_data(app: AppHandle, format: ExportFormat) -> Result<Option<String>, ApiError> {
    blocking(app, move |app, s| {
        let (ext, filter) = match format {
            ExportFormat::Csv => ("csv", "CSV"),
            ExportFormat::Json => ("json", "JSON"),
        };
        let mut dialog = app
            .dialog()
            .file()
            .set_file_name(format!(
                "productivity-tracker-{}.{ext}",
                Zoned::now().date()
            ))
            .add_filter(filter, &[ext]);
        if let Some(window) = app.get_webview_window(window::MAIN) {
            dialog = dialog.set_parent(&window);
        }
        let Some(path) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        let failed = |e: &dyn std::fmt::Display| ApiError {
            message: format!("Couldn't save the export: {e}"),
        };
        let path = path.into_path().map_err(|e| failed(&e))?;
        let contents = s.read(|r| r.export(format))?;
        std::fs::write(&path, contents).map_err(|e| failed(&e))?;
        Ok(Some(path.display().to_string()))
    })
    .await
}

/// Opens the folder holding the database in Explorer.
#[tauri::command]
#[specta::specta]
pub async fn open_data_folder(app: AppHandle) -> Result<(), ApiError> {
    blocking(app, |_, s| {
        std::process::Command::new("explorer.exe")
            .arg(&s.data_dir)
            .spawn()
            .map(drop)
            .map_err(|e| ApiError {
                message: format!("Couldn't open the data folder: {e}"),
            })
    })
    .await
}

/// Wipes history, tracked apps and sites, and settings, so the app starts over at first run.
/// Tracking keeps running. Like the settings, launch at login goes back to off (onboarding turns it
/// on), and the window to Glass.
#[tauri::command]
#[specta::specta]
pub async fn delete_all_data(app: AppHandle) -> Result<(), ApiError> {
    blocking(app, |app, s| {
        s.request(|reply| Control::DeleteAllData { reply })?;
        if let Err(e) = s.set_launch_at_login(app, false) {
            tracing::warn!("couldn't turn launch at login off: {e}");
        }
        if let Err(e) = window::apply_appearance(app, Appearance::Glass) {
            tracing::warn!("couldn't put the window back to glass: {e}");
        }
        // A pause is a setting too.
        tray::refresh(app);
        Ok(())
    })
    .await
}
