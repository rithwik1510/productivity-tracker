//! The browser's current domain from its address bar over UI Automation (ADR-0002), plus
//! private-window detection.
//!
//! The omnibox is found structurally (window → … → ToolBar → Edit), never by its localized name,
//! and the walk never reaches web content: listing the children of the web contents host is
//! enough to switch on Chromium's costly web-content accessibility (measured in the spike with
//! `--app` windows, which have no toolbar). No UIA events are used.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tracker_api::{BrowserView, normalize_domain};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest,
    IUIAutomationCondition, IUIAutomationElement, IUIAutomationValuePattern, TreeScope_Children,
    UIA_CONTROLTYPE_ID, UIA_ClassNamePropertyId, UIA_ControlTypePropertyId,
    UIA_DocumentControlTypeId, UIA_E_TIMEOUT, UIA_EditControlTypeId, UIA_NamePropertyId,
    UIA_ToolBarControlTypeId, UIA_ValuePatternId,
};
use windows::core::{BSTR, Interface, Result};

use crate::foreground::is_window;

/// Connection and transaction timeout for every UIA call. A hung browser UI thread must not
/// stall the 1 s tick.
const UIA_TIMEOUT_MS: u32 = 500;
/// After a timeout the window is read from its title only for this long.
const DEGRADED_FOR: Duration = Duration::from_secs(10);
/// A window without an omnibox (app windows, fullscreen from the start) is re-walked this often.
const NOT_FOUND_RETRY: Duration = Duration::from_secs(5);
/// How long to wait before retrying when UIA itself can't be created.
const RETRY_UIA_AFTER: Duration = Duration::from_secs(10);
/// Only elements shallower than this are expanded. Chrome and Edge put the toolbar at depth 6
/// (window → root view → non-client view → frame → browser view → top container → toolbar), and
/// the web contents host also sits at depth 6, so this cap is what keeps the walk out of it.
/// If a browser update moves the toolbar deeper, the read fails safe to the title fallback.
const MAX_DEPTH: usize = 6;
/// Depth limit for the walk from the toolbar to the omnibox (Chrome and Edge need 2).
const MAX_TOOLBAR_DEPTH: usize = 4;

/// One UIA client with the cache request and condition the walk reuses.
pub(crate) struct Uia {
    automation: IUIAutomation,
    cache: IUIAutomationCacheRequest,
    all: IUIAutomationCondition,
}

/// A child element with the properties the walk needs, fetched in the same round trip.
struct Node {
    element: IUIAutomationElement,
    control_type: UIA_CONTROLTYPE_ID,
    class: String,
    name: String,
}

impl Uia {
    /// Needs COM on the calling thread; the result must stay on it.
    pub(crate) fn new() -> Result<Uia> {
        // SAFETY: COM is initialized on this thread by the caller's `Apartment`.
        unsafe {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
            let timeouts: IUIAutomation2 = automation.cast()?;
            timeouts.SetConnectionTimeout(UIA_TIMEOUT_MS)?;
            timeouts.SetTransactionTimeout(UIA_TIMEOUT_MS)?;
            let cache = automation.CreateCacheRequest()?;
            for property in [
                UIA_ControlTypePropertyId,
                UIA_ClassNamePropertyId,
                UIA_NamePropertyId,
            ] {
                cache.AddProperty(property)?;
            }
            let all = automation.CreateTrueCondition()?;
            Ok(Uia {
                automation,
                cache,
                all,
            })
        }
    }

    fn children(&self, element: &IUIAutomationElement) -> Result<Vec<Node>> {
        // SAFETY: COM calls on objects created on this thread.
        unsafe {
            let array = element.FindAllBuildCache(TreeScope_Children, &self.all, &self.cache)?;
            let mut nodes = Vec::new();
            for i in 0..array.Length()? {
                let element = array.GetElement(i)?;
                nodes.push(Node {
                    control_type: element.CachedControlType()?,
                    class: element.CachedClassName()?.to_string(),
                    name: element.CachedName()?.to_string(),
                    element,
                });
            }
            Ok(nodes)
        }
    }

    /// The omnibox of a Chromium browser window. Also learns the window's privacy into `privacy`
    /// while it is Unknown, as soon as the window's direct children are read: they include the
    /// browser root view, whose accessible name is the window title plus a private marker in
    /// private windows (see `is_private_root`). So privacy is known even when the walk to the
    /// omnibox then fails or finds no toolbar (fullscreen).
    pub(crate) fn find_omnibox(
        &self,
        hwnd: HWND,
        privacy: &mut Privacy,
    ) -> Result<Option<IUIAutomationElement>> {
        // SAFETY: COM call on an object created on this thread; a stale HWND just fails.
        let root = unsafe { self.automation.ElementFromHandle(hwnd) }?;
        let top = self.children(&root)?;
        // No named child yet (a window still being built): stay Unknown and read again next tick.
        if *privacy == Privacy::Unknown && top.iter().any(|node| !node.name.is_empty()) {
            *privacy = if top.iter().any(|node| is_private_root(&node.name)) {
                Privacy::Private
            } else {
                Privacy::Normal
            };
        }
        for node in top {
            if let Some(omnibox) = self.search(node, 1)? {
                return Ok(Some(omnibox));
            }
        }
        Ok(None)
    }

    /// Depth-first in tree order, which reaches the toolbar before the web contents host.
    fn search(&self, node: Node, depth: usize) -> Result<Option<IUIAutomationElement>> {
        if node.control_type == UIA_DocumentControlTypeId
            || node.class == "Chrome_RenderWidgetHostHWND"
        {
            return Ok(None);
        }
        if node.control_type == UIA_ToolBarControlTypeId
            && let Some(edit) = self.find_edit(&node.element, 1)?
        {
            return Ok(Some(edit));
        }
        if depth >= MAX_DEPTH {
            return Ok(None);
        }
        for child in self.children(&node.element)? {
            if let Some(found) = self.search(child, depth + 1)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    fn find_edit(
        &self,
        element: &IUIAutomationElement,
        depth: usize,
    ) -> Result<Option<IUIAutomationElement>> {
        for child in self.children(element)? {
            if child.control_type == UIA_EditControlTypeId {
                return Ok(Some(child.element));
            }
            if depth < MAX_TOOLBAR_DEPTH
                && let Some(edit) = self.find_edit(&child.element, depth + 1)?
            {
                return Ok(Some(edit));
            }
        }
        Ok(None)
    }
}

/// What UIA has learned about a Chromium window's privacy. A window never changes privacy, so
/// once known it is kept for the window's life.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Privacy {
    /// Not read yet: the first read failed or timed out, or UIA is unavailable. Counts as
    /// private, because Chrome's title carries no marker: missing a normal window's time beats
    /// recording an incognito one.
    #[default]
    Unknown,
    Normal,
    Private,
}

/// Chrome's " (Incognito)" and Edge's " (InPrivate)" were seen in the spike; Brave's " (Private)"
/// is assumed, not verified, and so is Chrome's marker in Thorium and Comet, which keep Chrome's
/// UI. The markers are localized, so v1 detects English-UI browsers only.
fn is_private_root(name: &str) -> bool {
    ["(Incognito)", "(InPrivate)", "(Private)"]
        .iter()
        .any(|marker| name.ends_with(marker))
}

/// Private-window markers in window titles. Edge puts "[InPrivate]" in the title; Firefox
/// appends "Private Browsing", and its forks (Zen, LibreWolf, Waterfox, Floorp) are assumed to
/// keep it. Chrome's title has no marker (its UIA root view does).
pub(crate) fn title_is_private(title: &str) -> bool {
    title.contains(" - [InPrivate] - ") || title.ends_with("Private Browsing")
}

/// The view for browsers read from the title only (`uia: false` in `KNOWN_BROWSERS`).
pub(crate) fn title_view(title: &str) -> BrowserView {
    BrowserView {
        private: title_is_private(title),
        domain: None,
    }
}

pub(crate) fn value_pattern(element: &IUIAutomationElement) -> Result<IUIAutomationValuePattern> {
    // SAFETY: COM call on an element created on this thread.
    unsafe { element.GetCurrentPatternAs(UIA_ValuePatternId) }
}

/// The omnibox text and whether it has keyboard focus (the user is typing a draft).
fn read_omnibox(element: &IUIAutomationElement) -> Result<(String, bool)> {
    // SAFETY: COM calls on an element created on this thread.
    unsafe {
        let focused = element.CurrentHasKeyboardFocus()?.as_bool();
        let value: BSTR = value_pattern(element)?.CurrentValue()?;
        Ok((value.to_string(), focused))
    }
}

fn is_timeout(err: &windows::core::Error) -> bool {
    err.code().0 as u32 == UIA_E_TIMEOUT
}

#[derive(Default)]
struct WindowState {
    omnibox: Option<IUIAutomationElement>,
    privacy: Privacy,
    last_domain: Option<String>,
    /// Read from the title only until then (after a timeout, or when no omnibox was found).
    title_only_until: Option<Instant>,
}

/// Per-window omnibox cache and draft handling. Holds COM objects, so it stays on its thread.
pub(crate) struct BrowserReader {
    uia: Option<Uia>,
    uia_retry_at: Option<Instant>,
    windows: HashMap<isize, WindowState>,
}

impl BrowserReader {
    pub(crate) fn new() -> Self {
        Self {
            uia: None,
            uia_retry_at: None,
            windows: HashMap::new(),
        }
    }

    /// Lazily creates the UIA client, retrying at most every `RETRY_UIA_AFTER`.
    fn uia(&mut self) -> Option<&Uia> {
        if self.uia.is_none() && self.uia_retry_at.is_none_or(|at| Instant::now() >= at) {
            match Uia::new() {
                Ok(uia) => self.uia = Some(uia),
                Err(err) => {
                    tracing::warn!("UI Automation unavailable: {err}");
                    self.uia_retry_at = Some(Instant::now() + RETRY_UIA_AFTER);
                }
            }
        }
        self.uia.as_ref()
    }

    /// Whether UIA has found this window normal (not private) while it was in front. This answers
    /// for a minimized Chrome window, whose title has no marker, without a UIA call. A window not
    /// read since the tracker started isn't known normal, so it counts as private.
    pub(crate) fn known_normal(&self, hwnd: HWND) -> bool {
        self.windows
            .get(&(hwnd.0 as isize))
            .is_some_and(|state| state.privacy == Privacy::Normal)
    }

    pub(crate) fn read(&mut self, hwnd: HWND, title: &str) -> BrowserView {
        self.windows
            .retain(|&key, _| is_window(HWND(key as *mut _)));
        let key = hwnd.0 as isize;
        // Taken out of the map so `self.uia()` can borrow `self`.
        let mut state = self.windows.remove(&key).unwrap_or_default();
        let degraded = state
            .title_only_until
            .is_some_and(|until| Instant::now() < until);
        let domain = match self.uia() {
            Some(uia) if !degraded => read_with(uia, hwnd, &mut state).unwrap_or_else(|err| {
                if is_timeout(&err) {
                    tracing::debug!("omnibox read timed out; title only for {DEGRADED_FOR:?}");
                    state.title_only_until = Some(Instant::now() + DEGRADED_FOR);
                }
                state.omnibox = None;
                None
            }),
            _ => None,
        };
        // Unknown counts as private, whatever the title fallback would match.
        let view = BrowserView {
            private: state.privacy != Privacy::Normal || title_is_private(title),
            domain,
        };
        self.windows.insert(key, state);
        view
    }
}

fn read_with(uia: &Uia, hwnd: HWND, state: &mut WindowState) -> Result<Option<String>> {
    // A cached element that fails (tab dragged out, UI rebuilt) is re-found once in the same tick.
    // So is one found while privacy stayed Unknown, so the next read can learn it.
    let cached = state
        .omnibox
        .as_ref()
        .filter(|_| state.privacy != Privacy::Unknown);
    let read = match cached.map(read_omnibox) {
        Some(Ok(read)) => read,
        Some(Err(err)) if is_timeout(&err) => return Err(err),
        _ => {
            state.omnibox = uia.find_omnibox(hwnd, &mut state.privacy)?;
            match &state.omnibox {
                Some(omnibox) => read_omnibox(omnibox)?,
                // No toolbar: an app window, or fullscreen hid it. The title fallback takes over,
                // with the privacy the window's children told.
                None => {
                    state.title_only_until = Some(Instant::now() + NOT_FOUND_RETRY);
                    return Ok(None);
                }
            }
        }
    };
    let (value, focused) = read;
    if focused {
        // Focused and empty is a new tab; anything else is a draft being typed, so the page
        // still shows the last good domain.
        return Ok(if value.trim().is_empty() {
            None
        } else {
            state.last_domain.clone()
        });
    }
    state.last_domain = normalize_domain(&value).ok();
    Ok(state.last_domain.clone())
}
