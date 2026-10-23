//! The main window (plan section 6). It exists only while the user looks at it: closing destroys
//! it, which ends its WebView2 processes, and the tray or a second launch builds it again. It
//! reopens where it was last closed (size, position, maximized), see [`Placement`].

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use std::{fs, thread};

use tauri::utils::config::WindowEffectsConfig;
use tauri::webview::PageLoadEvent;
use tauri::window::{Effect, EffectsBuilder};
use tauri::{
    AppHandle, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, Theme, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tracker_api::Appearance;

use crate::state::AppState;

pub const MAIN: &str = "main";

/// A new window stays hidden until its page has loaded, but never longer than this: a window
/// beats no window if the load event goes missing.
const SHOW_AT_THE_LATEST: Duration = Duration::from_secs(3);

/// WebView2 profile folder inside our data dir, so every path derives from
/// `tracker_api::data_dir()` (Tauri would otherwise pick its own per-identifier folder).
const WEBVIEW_DIR: &str = "webview";

/// First-open size in logical px, shrunk to fit a smaller work area (a 1366×768 laptop at 125%
/// has about 1093×576).
const DEFAULT_SIZE: LogicalSize<f64> = LogicalSize::new(1000.0, 680.0);
/// The UI's smallest layout (docs/design/reference.md, section 4): it fits that laptop's work
/// area, and half of a 1536 px wide screen when snapped.
const MIN_SIZE: LogicalSize<f64> = LogicalSize::new(760.0, 520.0);
/// Logical px a fitted first-open window leaves free on each side of the work area.
const FIT_MARGIN: f64 = 16.0;

/// The last placement, in the data dir next to the database.
const PLACEMENT_FILE: &str = "window.txt";
/// Moves and resizes arrive many times a second while dragging; the placement is written once
/// they have stopped for this long, and a last time when the window closes.
const SAVE_AFTER: Duration = Duration::from_millis(500);

/// Where the window was: its outer position and inner size in physical px (one virtual-screen
/// coordinate space across monitors of any scale), and whether it was maximized. While maximized,
/// position and size are the restored bounds it returns to. Stored as one line of five numbers.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Placement {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
}

impl Placement {
    fn load(path: &Path) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        let mut numbers = text.split_whitespace();
        let mut next = || numbers.next()?.parse::<i64>().ok();
        let (x, y, width, height, maximized) = (next()?, next()?, next()?, next()?, next()?);
        Some(Self {
            x: x.try_into().ok()?,
            y: y.try_into().ok()?,
            width: width.try_into().ok()?,
            height: height.try_into().ok()?,
            maximized: maximized == 1,
        })
    }

    fn save(self, path: &Path) {
        let Self {
            x,
            y,
            width,
            height,
            maximized,
        } = self;
        let line = format!("{x} {y} {width} {height} {}\n", u8::from(maximized));
        if let Err(e) = fs::write(path, line) {
            tracing::warn!("couldn't save the window placement: {e}");
        }
    }

    /// Whether the middle of its title bar lies in this monitor's work area, so the window can be
    /// seen and dragged there. A placement on a monitor that is gone matches none.
    fn shows_on(&self, monitor: &Monitor) -> bool {
        let work = monitor.work_area();
        let x = i64::from(self.x) + i64::from(self.width / 2);
        let y = i64::from(self.y) + 20;
        let (left, top) = (i64::from(work.position.x), i64::from(work.position.y));
        (left..left + i64::from(work.size.width)).contains(&x)
            && (top..top + i64::from(work.size.height)).contains(&y)
    }

    /// The same size (no larger than the work area) moved fully inside the monitor's work area.
    fn inside(self, monitor: &Monitor) -> Self {
        let work = monitor.work_area();
        let width = self.width.min(work.size.width);
        let height = self.height.min(work.size.height);
        let right = work.position.x + i32::try_from(work.size.width - width).unwrap_or(0);
        let bottom = work.position.y + i32::try_from(work.size.height - height).unwrap_or(0);
        Self {
            x: self.x.clamp(work.position.x, right),
            y: self.y.clamp(work.position.y, bottom),
            width,
            height,
            ..self
        }
    }

    /// The default size, fitted to the monitor's work area, centered in it.
    fn centered(monitor: &Monitor) -> Self {
        let work = monitor.work_area();
        let room = |px: u32| f64::from(px) / monitor.scale_factor() - 2.0 * FIT_MARGIN;
        let size = LogicalSize::new(
            DEFAULT_SIZE.width.min(room(work.size.width)),
            DEFAULT_SIZE.height.min(room(work.size.height)),
        )
        .to_physical::<u32>(monitor.scale_factor());
        let offset = |free: u32, len: u32| i32::try_from(free.saturating_sub(len) / 2).unwrap_or(0);
        Self {
            x: work.position.x + offset(work.size.width, size.width),
            y: work.position.y + offset(work.size.height, size.height),
            width: size.width,
            height: size.height,
            maximized: false,
        }
    }
}

/// Shows the main window, building it if needed. Always hops onto an async task: building a
/// webview window from a sync handler (setup, tray, single-instance) deadlocks on Windows.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = match app.get_webview_window(MAIN) {
            Some(window) => window
                .unminimize()
                .and_then(|()| window.show())
                .and_then(|()| window.set_focus()),
            None => build(&app),
        };
        if let Err(e) = result {
            tracing::error!("couldn't open the window: {e}");
        }
    });
}

/// Switches the open window's backdrop, if there is a window; a new one reads the stored choice.
pub fn apply_appearance(app: &AppHandle, appearance: Appearance) -> tauri::Result<()> {
    match app.get_webview_window(MAIN) {
        Some(window) => window.set_effects(effects(appearance)),
        None => Ok(()),
    }
}

/// Acrylic for Glass; none for Solid, where the UI paints its opaque background over the
/// transparent window.
fn effects(appearance: Appearance) -> Option<WindowEffectsConfig> {
    match appearance {
        Appearance::Glass => Some(EffectsBuilder::new().effect(Effect::Acrylic).build()),
        Appearance::Solid => None,
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let appearance = state.read(|r| r.settings()).map_or_else(
        |e| {
            // A window beats no window; Glass is the default look.
            tracing::warn!("couldn't read the appearance, using glass: {e}");
            Appearance::Glass
        },
        |settings| settings.appearance,
    );
    let webview_dir = state.data_dir.join(WEBVIEW_DIR);
    let placement_file = state.data_dir.join(PLACEMENT_FILE);
    let builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::default())
        .title(&app.package_info().name)
        // Where it lands if `place` fails; `place` sets the real size and position.
        .inner_size(DEFAULT_SIZE.width, DEFAULT_SIZE.height)
        .min_inner_size(MIN_SIZE.width, MIN_SIZE.height)
        .center()
        // The UI draws its own title bar, with a maximize/restore button. Maximizable keeps the
        // WS_MAXIMIZEBOX style, without which Win+Up and Aero Snap don't maximize the window.
        .decorations(false)
        .maximizable(true)
        .transparent(true)
        // The UI is dark-only; without this, Acrylic takes a light tint in light-mode Windows.
        .theme(Some(Theme::Dark))
        .data_directory(webview_dir)
        // The window is transparent (so Glass can be switched on live), and until WebView2 paints
        // its first frame there is nothing in it: a Solid window would show the desktop through
        // it for a moment, which reads as a flash of glass. So it appears only once the page has
        // loaded, by which point index.html's render-blocking CSS has painted the opaque --bg.
        .visible(false)
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                reveal(&window);
            }
        });
    let window = match effects(appearance) {
        Some(effects) => builder.effects(effects),
        None => builder,
    }
    .build()?;
    // Still hidden, so it appears in place: no jump from the default spot.
    let placed = place(&window, Placement::load(&placement_file)).unwrap_or_else(|e| {
        tracing::warn!("couldn't place the window: {e}");
        None
    });
    remember(&window, placed, placement_file);
    thread::spawn(move || {
        thread::sleep(SHOW_AT_THE_LATEST);
        reveal(&window);
    });
    Ok(())
}

/// Puts a new, hidden window back where it was last closed, kept inside that monitor's work area,
/// if that monitor is still connected. Otherwise (first open, or the monitor is gone) it gets the
/// default size, fitted to its monitor's work area and centered. Returns what it applied.
fn place(window: &WebviewWindow, saved: Option<Placement>) -> tauri::Result<Option<Placement>> {
    let monitors = window.available_monitors()?;
    let placement = match saved.and_then(|p| Some((p, monitors.iter().find(|m| p.shows_on(m))?))) {
        Some((saved, monitor)) => saved.inside(monitor),
        None => match window.current_monitor()?.or(window.primary_monitor()?) {
            Some(monitor) => Placement::centered(&monitor),
            None => return Ok(None),
        },
    };
    window.set_size(PhysicalSize::new(placement.width, placement.height))?;
    window.set_position(PhysicalPosition::new(placement.x, placement.y))?;
    // On a hidden window tao defers this until it is shown, so it appears maximized, no flash.
    if placement.maximized {
        window.maximize()?;
    }
    Ok(Some(placement))
}

/// Follows the window's moves and resizes and saves its placement once they settle (see
/// [`SAVE_AFTER`]). While maximized only the flag changes, so the restored bounds are kept;
/// minimized states (parked at -32000) are skipped.
fn remember(window: &WebviewWindow, placed: Option<Placement>, path: PathBuf) {
    let (changed, changes) = mpsc::channel();
    thread::spawn(move || save_when_settled(&changes, &path));
    let last = Mutex::new(placed);
    let this = window.clone();
    window.on_window_event(move |event| {
        if !matches!(event, WindowEvent::Moved(_) | WindowEvent::Resized(_)) {
            return;
        }
        // Errors mean the window is going away; its last settled placement is already queued.
        let (Ok(false), Ok(maximized)) = (this.is_minimized(), this.is_maximized()) else {
            return;
        };
        let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
        let now = if maximized {
            last.map(|p| Placement {
                maximized: true,
                ..p
            })
        } else {
            match (this.outer_position(), this.inner_size()) {
                (Ok(at), Ok(size)) => Some(Placement {
                    x: at.x,
                    y: at.y,
                    width: size.width,
                    height: size.height,
                    maximized: false,
                }),
                _ => None,
            }
        };
        if let Some(now) = now
            && *last != Some(now)
        {
            *last = Some(now);
            // The saver only stops once this handler (the sender) is dropped with the window.
            let _ = changed.send(now);
        }
    });
}

/// Saves the latest placement once no change has come for [`SAVE_AFTER`]. When the window is
/// destroyed its handler drops the sender, which saves what is pending and ends the thread.
fn save_when_settled(changes: &Receiver<Placement>, path: &Path) {
    while let Ok(mut latest) = changes.recv() {
        while let Ok(next) = changes.recv_timeout(SAVE_AFTER) {
            latest = next;
        }
        latest.save(path);
    }
}

/// Shows and focuses a window that is still hidden. Reloads (dev HMR) fire the load event again;
/// a visible window is left alone so they don't steal focus. A destroyed window just errors.
fn reveal(window: &WebviewWindow) {
    if window.is_visible().is_ok_and(|visible| !visible)
        && let Err(e) = window.show().and_then(|()| window.set_focus())
    {
        tracing::warn!("couldn't show the window: {e}");
    }
}
