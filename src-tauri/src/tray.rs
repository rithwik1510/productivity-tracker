//! The tray icon: the app's only presence while the window is closed (plan section 6). It also
//! owns Pause, which lives only here and in the Day view's live line.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Wry, include_image};
use tracker::days::next_day_start;
use tracker_api::{ApiError, Control, TrackerStatus};

use crate::state::AppState;
use crate::{glass, window};

const TRAY: &str = "main";
const OPEN: &str = "open";
const PAUSE_15_MIN: &str = "pause-15-min";
const PAUSE_1_HOUR: &str = "pause-1-hour";
const PAUSE_UNTIL_TOMORROW: &str = "pause-until-tomorrow";
const RESUME: &str = "resume";
const QUIT: &str = "quit";
const TOOLTIP_REFRESH: Duration = Duration::from_secs(60);
/// Emitted after every pause or resume, so the Day view swaps its live line at once.
const PAUSE_CHANGED: &str = "pause-changed";

/// Whether the menu on the tray has Resume, so it is rebuilt only when that changes: replacing it
/// on every refresh would disturb a menu the user has open.
static MENU_PAUSED: AtomicBool = AtomicBool::new(false);

/// The mono mark at Windows' small-icon size for 100, 125, 150 and 200 % scale, drawn per size by
/// `bun run icons` so Windows never has to scale it. White for a dark taskbar, black for a light one.
const ICON_SIZES: [u32; 4] = [16, 20, 24, 32];
const WHITE: [Image<'static>; 4] = [
    include_image!("icons/tray/white-16.png"),
    include_image!("icons/tray/white-20.png"),
    include_image!("icons/tray/white-24.png"),
    include_image!("icons/tray/white-32.png"),
];
const BLACK: [Image<'static>; 4] = [
    include_image!("icons/tray/black-16.png"),
    include_image!("icons/tray/black-20.png"),
    include_image!("icons/tray/black-24.png"),
    include_image!("icons/tray/black-32.png"),
];
/// The icon on the tray, as `icon_key`, so it is replaced only when the theme or scale changed.
static ICON_SHOWN: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Needs `AppState` managed: the tooltip and menu read through its shared reader.
pub fn build(app: &App) -> tauri::Result<()> {
    let paused = paused_until(&app.state());
    MENU_PAUSED.store(paused.is_some(), Ordering::Relaxed);
    let key = icon_key(app.handle());
    ICON_SHOWN.store(key, Ordering::Relaxed);
    TrayIconBuilder::with_id(TRAY)
        .icon(icon(key))
        .tooltip(tooltip(&app.state(), paused))
        .menu(&menu(app.handle(), paused.is_some())?)
        // Left-click opens the window; the menu is on right-click.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            OPEN => window::open(app),
            PAUSE_15_MIN => pause(app, |now| Ok(Some(now + 15 * 60_000))),
            PAUSE_1_HOUR => pause(app, |now| Ok(Some(now + 3_600_000))),
            // The next Day boundary, 4am local.
            PAUSE_UNTIL_TOMORROW => pause(app, |now| {
                next_day_start(now, &TimeZone::system()).map(Some)
            }),
            RESUME => pause(app, |_| Ok(None)),
            QUIT => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::open(tray.app_handle());
            }
        })
        .build(app)?;

    let app = app.handle().clone();
    thread::Builder::new()
        .name("tray-tooltip".into())
        .spawn(move || {
            loop {
                // Wakes when a pause ends too, so Resume and the tooltip don't outlive it.
                let wait = paused_until(&app.state()).map_or(TOOLTIP_REFRESH, |until| {
                    let left = until.saturating_sub(Timestamp::now().as_millisecond());
                    Duration::from_millis(u64::try_from(left).unwrap_or(0) + 500)
                        .min(TOOLTIP_REFRESH)
                });
                thread::sleep(wait);
                refresh(&app);
            }
        })?;
    Ok(())
}

/// Pauses until the instant `until` gives for the current time, or resumes when it gives `None`.
/// Off the main thread: the tracker answers within a tick, which the menu shouldn't wait for.
fn pause(
    app: &AppHandle,
    until: impl FnOnce(i64) -> Result<Option<i64>, jiff::Error> + Send + 'static,
) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = until(Timestamp::now().as_millisecond())
            .map_err(|e| ApiError {
                message: e.to_string(),
            })
            .and_then(|until_ms| set_pause(&app, until_ms));
        if let Err(e) = result {
            tracing::warn!("couldn't pause or resume: {e}");
        }
    });
}

/// Pauses tracking until `until_ms` (`None` resumes), then updates the tray and tells the window.
/// Blocks on the tracker's reply.
pub fn set_pause(app: &AppHandle, until_ms: Option<i64>) -> Result<(), ApiError> {
    app.state::<AppState>()
        .request(|reply| Control::SetPause { until_ms, reply })?;
    refresh(app);
    if let Err(e) = app.emit(PAUSE_CHANGED, ()) {
        tracing::warn!("couldn't tell the window about the pause: {e}");
    }
    Ok(())
}

/// Swaps the icon when the taskbar's theme or the primary monitor's scale changed. The session
/// watcher calls it on the theme broadcast; `refresh` calls it too, which catches a scale change
/// within `TOOLTIP_REFRESH`.
pub fn update_icon(app: &AppHandle) {
    let key = icon_key(app);
    if ICON_SHOWN.swap(key, Ordering::Relaxed) == key {
        return;
    }
    if let Some(tray) = app.tray_by_id(TRAY)
        && let Err(e) = tray.set_icon(Some(icon(key)))
    {
        tracing::warn!("couldn't update the tray icon: {e}");
    }
}

/// Which icon fits now: the index into `ICON_SIZES`, plus 4 for a light taskbar. The taskbar has
/// its own theme setting (`SystemUsesLightTheme`, not the apps' one); missing means dark, the
/// Windows 11 default. The tray sits on the primary monitor, so its scale picks the size.
fn icon_key(app: &AppHandle) -> usize {
    let light = windows_registry::CURRENT_USER
        .open(glass::PERSONALIZE)
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        == Ok(1);
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map_or(1.0, |monitor| monitor.scale_factor());
    // The smallest drawn size that covers it, so 175 % (28 px) gets 32 scaled down, not 24 up.
    let size = ICON_SIZES
        .iter()
        .position(|&px| f64::from(px) >= 16.0 * scale)
        .unwrap_or(ICON_SIZES.len() - 1);
    size + if light { ICON_SIZES.len() } else { 0 }
}

fn icon(key: usize) -> Image<'static> {
    let set = if key >= ICON_SIZES.len() {
        &BLACK
    } else {
        &WHITE
    };
    set[key % ICON_SIZES.len()].clone()
}

/// Re-reads today's total and the pause into the tooltip, rebuilds the menu when Resume has to
/// appear or go, and re-checks the icon.
pub fn refresh(app: &AppHandle) {
    update_icon(app);
    let Some(tray) = app.tray_by_id(TRAY) else {
        return;
    };
    let state = app.state::<AppState>();
    let paused = paused_until(&state);
    let mut result = tray.set_tooltip(Some(tooltip(&state, paused)));
    if MENU_PAUSED.swap(paused.is_some(), Ordering::Relaxed) != paused.is_some() {
        result = result.and(menu(app, paused.is_some()).and_then(|menu| tray.set_menu(Some(menu))));
    }
    if let Err(e) = result {
        tracing::warn!("couldn't update the tray: {e}");
    }
}

/// Open · Pause ▸ (15 minutes · 1 hour · Until tomorrow) · Resume (only while paused) · Quit.
fn menu(app: &AppHandle, paused: bool) -> tauri::Result<Menu<Wry>> {
    let open = MenuItem::with_id(app, OPEN, "Open", true, None::<&str>)?;
    let pause = Submenu::with_items(
        app,
        "Pause",
        true,
        &[
            &MenuItem::with_id(app, PAUSE_15_MIN, "15 minutes", true, None::<&str>)?,
            &MenuItem::with_id(app, PAUSE_1_HOUR, "1 hour", true, None::<&str>)?,
            &MenuItem::with_id(
                app,
                PAUSE_UNTIL_TOMORROW,
                "Until tomorrow",
                true,
                None::<&str>,
            )?,
        ],
    )?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    if paused {
        let resume = MenuItem::with_id(app, RESUME, "Resume", true, None::<&str>)?;
        Menu::with_items(app, &[&open, &pause, &resume, &quit])
    } else {
        Menu::with_items(app, &[&open, &pause, &quit])
    }
}

/// When the current pause ends, if paused. A failed read counts as not paused.
fn paused_until(state: &AppState) -> Option<i64> {
    state
        .read(|r| r.paused_until_ms())
        .inspect_err(|e| tracing::warn!("couldn't read the pause: {e}"))
        .ok()
        .flatten()
}

/// A pause's end as a local clock time, `3:40 PM`. Shared by the tooltip and `get_pause`.
pub fn clock_time(ms: i64) -> String {
    Timestamp::from_millisecond(ms).map_or_else(
        |_| "later".to_owned(),
        |t| {
            t.to_zoned(TimeZone::system())
                .strftime("%-I:%M %p")
                .to_string()
        },
    )
}

/// `Today: 4h 07m Productive`, formatted like the UI's `Duration`, under a `Paused until 3:40 PM`
/// line while paused, and under `Can't save, retrying` while the tracker's writes fail. That line
/// follows the tooltip's refresh, so it can lag by up to `TOOLTIP_REFRESH`.
fn tooltip(state: &AppState, paused_until: Option<i64>) -> String {
    let today = match state.read(|r| r.today_productive_secs()) {
        Ok(secs) => {
            let (h, m) = (secs / 3600, secs % 3600 / 60);
            let time = match (h, m) {
                (0, 0) if secs > 0 => "<1m".to_owned(),
                (0, m) => format!("{m}m"),
                (h, m) => format!("{h}h {m:02}m"),
            };
            format!("Today: {time} Productive")
        }
        Err(e) => {
            tracing::warn!("couldn't read today's total for the tray: {e}");
            "Today: unavailable".to_owned()
        }
    };
    let today = match paused_until {
        Some(until) => format!("Paused until {}\n{today}", clock_time(until)),
        None => today,
    };
    if state.runtime.status() == Some(TrackerStatus::SaveFailed) {
        format!("Can't save, retrying\n{today}")
    } else {
        today
    }
}

/// Flushes the open interval before exiting, because `exit` doesn't run the tracker's drop path.
fn quit_app(app: &AppHandle) {
    app.state::<AppState>()
        .runtime
        .shutdown_and_wait(Duration::from_secs(3));
    app.exit(0);
}
