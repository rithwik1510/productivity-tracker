//! Thin Tauri shell around the tracker crates (ADR-0004): commands, tray, window lifecycle,
//! single instance, autostart and the asset scope. Startup order is plan section 6.

mod commands;
mod glass;
mod state;
mod tray;
mod window;

use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tauri::{App, Manager, RunEvent};
use tauri_specta::{Builder, ErrorHandlingMode, collect_commands};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracker::RuntimeHandle;
use tracker_win::{SessionWatcher, WinSampler};

use crate::state::AppState;

/// Passed by the login Run entry, so a login start stays in the tray.
const AUTOSTART_ARG: &str = "--autostart";
const LOGS_DIR: &str = "logs";

/// The one command registry, shared by `run()` and the `export-bindings` bin so the generated
/// bindings always match what the app serves. `Throw` makes failed commands reject with `ApiError`.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .error_handling(ErrorHandlingMode::Throw)
        .commands(collect_commands![
            commands::get_day,
            commands::get_week,
            commands::get_share,
            commands::get_year,
            commands::list_catalog,
            commands::set_app_category,
            commands::set_app_watch,
            commands::list_sites,
            commands::upsert_site,
            commands::get_settings,
            commands::set_daily_goal,
            commands::set_launch_at_login,
            commands::complete_onboarding,
            commands::glass_available,
            commands::get_status,
            commands::set_appearance,
            commands::set_away_after,
            commands::set_waiting_credit,
            commands::get_pause,
            commands::resume_tracking,
            commands::export_data,
            commands::open_data_folder,
            commands::delete_all_data,
        ])
}

pub fn run() {
    let builder = specta_builder();
    // Registered first: a second launch hands over to this process before starting anything.
    let app =
        tauri::Builder::default().plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            window::open(app)
        }));
    // Debug builds never register the plugin, so they can't touch the Run key.
    #[cfg(not(debug_assertions))]
    let app = app.plugin(
        tauri_plugin_autostart::Builder::new()
            .arg(AUTOSTART_ARG)
            .build(),
    );
    // The export's save dialog, called from Rust only, so the UI needs no dialog permission.
    app.plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            // Tauri panics on a setup error, which a release build (no console) never shows.
            if let Err(e) = setup(app) {
                fatal(&*e);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| fatal(&e))
        .run(|_, event| {
            // Closing the last window must not end the app: tracking continues from the tray.
            // Only tray Quit exits, and it passes an explicit code.
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}

fn setup(app: &mut App) -> Result<(), Box<dyn Error>> {
    let data_dir = tracker_api::data_dir()?;
    start_logging(&data_dir)?;

    let watcher = SessionWatcher::start();
    let sampler_watcher = Arc::clone(&watcher);
    // The sampler is built on the poll thread: its COM objects are `!Send`.
    let runtime = RuntimeHandle::start(data_dir.clone(), move || {
        Box::new(WinSampler::new(sampler_watcher))
    })?;
    let hook_runtime = runtime.clone();
    // Windows kills the process soon after WM_ENDSESSION returns; flush before letting it.
    watcher.set_end_session_hook(Box::new(move || {
        hook_runtime.shutdown_and_wait(Duration::from_secs(2))
    }));

    #[cfg(not(debug_assertions))]
    repair_launch_at_login(app);

    let state = AppState::new(runtime, data_dir)?;
    app.asset_protocol_scope()
        .allow_directory(state.data_dir.join(tracker_api::ICONS_DIR_NAME), false)?;
    let onboarded = state.read(|r| r.settings())?.onboarded;
    app.manage(state);
    tray::build(app)?;
    let handle = app.handle().clone();
    watcher.set_theme_hook(Box::new(move || tray::update_icon(&handle)));

    // FirstRun must be reachable, so a login start before onboarding still shows the window.
    if !onboarded || !std::env::args().any(|arg| arg == AUTOSTART_ARG) {
        window::open(app.handle());
    }
    tracing::info!("started");
    Ok(())
}

/// The login Run entry holds the path of whichever exe last enabled it, so after a reinstall to
/// another folder, or once a preview build has enabled it, login would start the wrong exe (or
/// none). If launch at login is on and the entry isn't this exe's, this exe registers itself:
/// the release build that started last owns the entry. Failures are logged; Settings still shows
/// the real state.
#[cfg(not(debug_assertions))]
fn repair_launch_at_login(app: &App) {
    use tauri_plugin_autostart::ManagerExt;
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    let autolaunch = app.autolaunch();
    match autolaunch.is_enabled() {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => return tracing::warn!("couldn't read launch at login: {e}"),
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return tracing::warn!("couldn't find our own exe: {e}"),
    };
    // The value the plugin writes: the exe path, a space, then the arguments.
    let ours = format!("{} {AUTOSTART_ARG}", exe.display());
    let registered = windows_registry::CURRENT_USER
        .open(RUN)
        .and_then(|key| key.get_string(&app.package_info().name));
    if registered.is_ok_and(|value| value.eq_ignore_ascii_case(&ours)) {
        return;
    }
    match autolaunch.enable() {
        Ok(()) => tracing::info!("launch at login now starts {}", exe.display()),
        Err(e) => tracing::warn!("couldn't re-register launch at login: {e}"),
    }
}

/// Logs a startup failure, tells the user in a dialog (there is no console), then exits.
fn fatal(error: &dyn Error) -> ! {
    tracing::error!("startup failed: {error}");
    tracker_win::error_box(
        "Productivity Tracker",
        &format!("Productivity Tracker couldn't start.\n\n{error}"),
    );
    std::process::exit(1);
}

/// Daily files in `<data_dir>/logs`, keeping a week. Written synchronously (no worker guard to
/// keep alive), which is fine at this log volume and keeps the last lines on `exit`.
fn start_logging(data_dir: &Path) -> Result<(), Box<dyn Error>> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("tracker")
        .filename_suffix("log")
        .max_log_files(7)
        .build(data_dir.join(LOGS_DIR))?;
    tracing_subscriber::fmt()
        .with_writer(appender)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .try_init()
        .map_err(|e| e as Box<dyn Error>)?;
    Ok(())
}
