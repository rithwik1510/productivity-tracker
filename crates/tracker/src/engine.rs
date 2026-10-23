//! Pure per-tick logic (plan section 4): target resolution, presence, cutoffs, stretches, the 1 h
//! roll, waiting credit (ADR-0007) and pause. No I/O: each tick turns a `Sample` into `Op`s that
//! the runtime applies to the store.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet, VecDeque};

use tracker_api::{AppIdentity, Category, Foreground, Sample, TargetRef, normalize_domain};

/// Sound alone keeps the user Active at most this long after the last input.
const INPUT_CAP_MS: i64 = 2 * 3_600_000;
/// A wall step above this between ticks was a gap (missed suspend, Modern Standby, clock jump).
const GAP_WALL_MS: i64 = 3_000;
/// Wall and monotonic deltas disagreeing by more than this was a gap too.
const GAP_DRIFT_MS: u64 = 2_000;
/// Input this close before a lock (Win+L itself, plus the notification's lag) isn't presence.
const LOCK_NOISE_MS: u64 = 2_000;
/// A backstop on the lookback ring, which is pruned by time (see `Engine::remember`). Only a
/// monotonic clock that stops while ticks continue could reach it.
const RING_MAX: usize = 16;
/// An open row is closed and reopened when it reaches 1 h - 1 tick. With a late tick closing a row
/// on a target change (up to `GAP_WALL_MS` later), no row exceeds 1 h + 2 s, which is what makes
/// the Day query's 1 h + 3 s lookback exact.
const ROLL_MS: i64 = 3_600_000 - 1_000;
/// A minimized Productive app can wait in for at most this long from the first tick it's seen
/// minimized after being seen not minimized (ADR-0007). This also keeps waiting rows far shorter
/// than the roll.
const MINIMIZED_CREDIT_MS: i64 = 30 * 60_000;

/// What the engine asks the store to do. The store owns row ids; the engine never sees them.
/// At most one row is open at a time, so Extend and Close need no name for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Insert a row with `open = 1`. The first Open of a stretch is where its trims start.
    Open { key: Key, start_ms: i64 },
    /// The open row's new end, kept in memory and written on the next flush.
    Extend { end_ms: i64 },
    /// Close the open row; the stretch goes on.
    Close { end_ms: i64 },
    /// Trim the stretch's rows back to `cutoff_ms` and close them; no row stays open.
    EndStretch { cutoff_ms: i64 },
}

/// The user's settings the engine applies on every tick, loaded from the store after every change.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// "Count as away after": evidence older than this means Away.
    pub away_after_ms: i64,
    /// "Keep counting minimized apps": off skips waiting credit (ADR-0007) in the priority rule.
    /// The clocks keep running either way, so turning it back on resumes them.
    pub waiting_credit: bool,
    /// Pause: no tick before this instant counts, as if Away. A past instant is no pause.
    pub paused_until_ms: Option<i64>,
}

/// The currently tracked apps and sites, loaded from the store after every config change.
#[derive(Debug, Default)]
pub struct Targets {
    pub apps: HashMap<AppIdentity, TrackedApp>,
    pub sites: Vec<TrackedSite>,
}

#[derive(Debug)]
pub struct TrackedApp {
    pub id: u32,
    /// The stored name, for `Tracker::now`.
    pub name: String,
    pub category: Category,
}

#[derive(Debug)]
pub struct TrackedSite {
    pub id: u32,
    /// Normalized, as `normalize_domain` returns it.
    pub domain: String,
    /// Already `fold`ed. May list several keywords, comma-separated ("reddit, : r/").
    pub keyword: Option<String>,
    pub category: Category,
}

/// What the open row counts toward, borrowed from the current targets.
#[derive(Debug)]
pub enum Counted<'a> {
    App {
        identity: &'a AppIdentity,
        app: &'a TrackedApp,
    },
    Site(&'a TrackedSite),
}

/// What a row counts toward. A change of any part closes the open row and opens a new one: the
/// Category (ADR-0005), and `minimized`, so "Claude waiting" and "Claude in front" are separate
/// rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub target: TargetRef,
    pub category: Category,
    /// Waiting credit (ADR-0007): a minimized app counting while the foreground isn't
    /// Productive. Stored as `interval.minimized`.
    pub minimized: bool,
}

/// Where the user was on the last tick (`Engine::presence`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// In a stretch, whether or not anything counts.
    Active,
    Paused,
    Away,
}

/// One past tick, for gap detection and the lock lookback.
#[derive(Debug, Clone, Copy)]
struct Mark {
    now_ms: i64,
    mono_ms: u64,
    evidence_ms: i64,
}

/// A run of Active ticks.
#[derive(Debug)]
struct Stretch {
    start_ms: i64,
    /// The latest wall time seen in the stretch. Ticks use `max(now, latest)`, so a small backward
    /// wall step (under the gap threshold) can't make a row overlap the one before it.
    latest_ms: i64,
    /// The open row, if this tick counts toward anything.
    open: Option<OpenRow>,
}

#[derive(Debug)]
struct OpenRow {
    key: Key,
    start_ms: i64,
}

#[derive(Debug)]
pub struct Engine {
    targets: Targets,
    rules: Rules,
    /// Never decreases, except the reset after a gap.
    last_input_ms: i64,
    /// `i64::MIN` = no sound since the last gap.
    last_sound_ms: i64,
    /// Recent ticks before the current one, oldest first, reaching back at least
    /// `LOCK_NOISE_MS` from the newest whenever there is that much history (see `remember`).
    ring: VecDeque<Mark>,
    stretch: Option<Stretch>,
    /// Credit left on the 30-minute clock of every app with a minimized window that isn't in
    /// front, tracked or not. A clock starts on the first tick its app is seen that way and runs
    /// on forward wall time only: through Away, gaps and sleep, while a clock set back adds
    /// nothing. The app leaves the map, which resets its clock, once it is in front or has no
    /// minimized window. Credit at or below 0 is spent. Only a Productive Tracked App with credit
    /// left may wait in (`counted_key`); the others keep their clocks too, so a Category flip
    /// back to Productive resumes the same clock.
    minimized: HashMap<AppIdentity, i64>,
    /// Apps this engine has seen not minimized: in front, or gone from the minimized list
    /// (restored or closed). Only these start a clock with credit; any other app enters
    /// `minimized` already spent. So an app that was minimized when tracking started (a login, a
    /// restart, the reopen after a panic, Delete all) earns nothing until the user brings it back
    /// and minimizes it again.
    seen_unminimized: HashSet<AppIdentity>,
}

impl Engine {
    pub fn new(targets: Targets, rules: Rules) -> Self {
        Self {
            targets,
            rules,
            last_input_ms: i64::MIN,
            last_sound_ms: i64::MIN,
            ring: VecDeque::with_capacity(RING_MAX + 1),
            stretch: None,
            minimized: HashMap::new(),
            seen_unminimized: HashSet::new(),
        }
    }

    /// Config changes apply here; the next tick's key comparison does the split.
    pub fn set_targets(&mut self, targets: Targets) {
        self.targets = targets;
    }

    /// Like targets, rules apply from the next tick. Pausing doesn't end the stretch by itself:
    /// the runtime ends it right away, as a shutdown would.
    pub fn set_rules(&mut self, rules: Rules) {
        self.rules = rules;
    }

    pub fn tick(&mut self, s: &Sample) -> Vec<Op> {
        let mut ops = Vec::new();
        let idle_since = s.now_ms.saturating_sub(i64::from(s.idle_ms));
        // Read before a gap clears the ring: the clocks count the gap too.
        let forward_ms = self
            .ring
            .back()
            .map_or(0, |prev| s.now_ms.saturating_sub(prev.now_ms).max(0));
        self.run_clocks(s, forward_ms);

        if let Some(prev) = self.ring.back().copied()
            && is_gap(prev, s)
        {
            // The gap was Away: end the stretch as the previous tick would have seen a lock,
            // then start presence afresh so a backward jump can't leave evidence in the future.
            self.end_stretch(self.lock_cutoff(prev.mono_ms), &mut ops);
            self.last_input_ms = idle_since;
            self.last_sound_ms = i64::MIN;
            self.ring.clear();
        }

        let unlock_ms = if s.input_event { s.now_ms } else { i64::MIN };
        self.last_input_ms = self.last_input_ms.max(idle_since).max(unlock_ms);
        // Sound behind a lock or a dark display isn't presence.
        let audible = s.foreground.as_ref().is_some_and(|f| f.audible);
        if audible && !s.away_signal {
            self.last_sound_ms = self.last_sound_ms.max(s.now_ms);
        }
        let evidence_ms = self.last_input_ms.max(self.last_sound_ms);

        let capped = s.now_ms.saturating_sub(self.last_input_ms) >= INPUT_CAP_MS;
        let silent = s.now_ms.saturating_sub(evidence_ms) >= self.rules.away_after_ms;
        // Presence is still followed while paused, so the first tick after the pause counts only
        // if the user is really there.
        let paused = self
            .rules
            .paused_until_ms
            .is_some_and(|until| s.now_ms < until);
        if !s.away_signal && !capped && !silent && !paused {
            let (key, spent_at) = self.counted_key(s);
            let stretch = self.stretch.get_or_insert(Stretch {
                start_ms: s.now_ms,
                latest_ms: s.now_ms,
                open: None,
            });
            stretch.latest_ms = stretch.latest_ms.max(s.now_ms);
            let now_ms = stretch.latest_ms;
            // A waiting row whose 30 minutes ran out between ticks ends at that exact instant, and
            // whatever counts next starts there, so per-target sums stay exact.
            let switch_ms = match (&stretch.open, spent_at) {
                (Some(open), Some(at)) => at.clamp(open.start_ms, now_ms),
                _ => now_ms,
            };
            track(&mut stretch.open, key, now_ms, switch_ms, &mut ops);
        } else if self.stretch.is_some() {
            // When several reasons hold at once the earliest cutoff wins: missing real time is
            // safer than inventing it.
            let mut cutoff = i64::MAX;
            if s.away_signal {
                cutoff = cutoff.min(self.lock_cutoff(s.mono_ms));
            }
            if capped {
                cutoff = cutoff.min(self.last_input_ms);
            }
            if silent {
                cutoff = cutoff.min(evidence_ms);
            }
            // The runtime ends the stretch when it pauses, so this is only a backstop: end where a
            // shutdown after the previous tick would have.
            if paused {
                cutoff = cutoff.min(self.stop_cutoff().unwrap_or(i64::MIN));
            }
            self.end_stretch(cutoff, &mut ops);
        }

        self.remember(Mark {
            now_ms: s.now_ms,
            mono_ms: s.mono_ms,
            evidence_ms,
        });
        ops
    }

    /// Ends the stretch at `stop_cutoff`.
    pub fn shutdown(&mut self) -> Vec<Op> {
        let mut ops = Vec::new();
        if let Some(cutoff) = self.stop_cutoff() {
            self.end_stretch(cutoff, &mut ops);
        }
        ops
    }

    /// Where the stretch would end if tracking stopped after the last tick: the normal cutoff
    /// `min(now, evidence)`, which trims trailing idle time. Shutdown ends the stretch here, and
    /// every flush stores it so crash recovery trims the same way. `None` without a stretch.
    pub fn stop_cutoff(&self) -> Option<i64> {
        let stretch = self.stretch.as_ref()?;
        let last = self.ring.back()?;
        Some(last.now_ms.min(last.evidence_ms).max(stretch.start_ms))
    }

    /// Whether the last tick was Active, paused or Away, for the status line.
    pub fn presence(&self) -> Presence {
        let paused = self
            .rules
            .paused_until_ms
            .is_some_and(|until| self.ring.back().is_some_and(|last| last.now_ms < until));
        if self.stretch.is_some() {
            Presence::Active
        } else if paused {
            Presence::Paused
        } else {
            Presence::Away
        }
    }

    /// The store failed to apply this tick's ops, so the writer no longer follows the engine's
    /// open row. Forgetting it makes the next Active tick open a new row from its own `now`: the
    /// time in between is lost, never invented. The stretch stays, so its trims still reach the
    /// rows already written.
    pub fn forget_open_row(&mut self) {
        if let Some(stretch) = &mut self.stretch {
            stretch.open = None;
        }
    }

    /// What this tick counts toward, with the open row's key. While a minimized app waits in
    /// (ADR-0007) that is the minimized app (`key.minimized`), since the time goes to it. `None`
    /// when Away or nothing counts.
    pub fn counting(&self) -> Option<(Counted<'_>, Key)> {
        let key = self.stretch.as_ref()?.open.as_ref()?.key;
        let counted = match key.target {
            // A linear scan: a handful of tracked apps, once a tick.
            TargetRef::App { id } => self
                .targets
                .apps
                .iter()
                .find(|(_, app)| app.id == id)
                .map(|(identity, app)| Counted::App { identity, app })?,
            TargetRef::Site { id } => self
                .targets
                .sites
                .iter()
                .find(|s| s.id == id)
                .map(Counted::Site)?,
        };
        Some((counted, key))
    }

    /// Adds the tick just handled to the ring and drops marks no lookback can need: a mark is kept
    /// until the one after it is itself at least `LOCK_NOISE_MS` older than the newest, so a later
    /// observer (always at or after the newest tick) finds its answer in the ring. Pruning by time
    /// rather than by count matters: tick spacing varies (timer lateness, a slow UIA read, the
    /// catch-up tick after a stall), and three ticks can span less than 2 s.
    fn remember(&mut self, mark: Mark) {
        self.ring.push_back(mark);
        while self.ring.len() > RING_MAX
            || self
                .ring
                .get(1)
                .is_some_and(|next| next.mono_ms.saturating_add(LOCK_NOISE_MS) <= mark.mono_ms)
        {
            self.ring.pop_front();
        }
    }

    /// The away-signal/gap cutoff: `min(prev.now, evidence of the newest tick at least 2 s before
    /// the observing tick)`. `i64::MIN` (none qualifies, which the ring's retention limits to a
    /// stretch younger than 2 s) becomes the stretch start in `end_stretch`.
    fn lock_cutoff(&self, observed_mono_ms: u64) -> i64 {
        let Some(prev) = self.ring.back() else {
            return i64::MIN;
        };
        let settled = self
            .ring
            .iter()
            .rev()
            .find(|m| m.mono_ms.saturating_add(LOCK_NOISE_MS) <= observed_mono_ms)
            .map_or(i64::MIN, |m| m.evidence_ms);
        prev.now_ms.min(settled)
    }

    fn end_stretch(&mut self, cutoff_ms: i64, ops: &mut Vec<Op>) {
        if let Some(stretch) = self.stretch.take() {
            ops.push(Op::EndStretch {
                cutoff_ms: cutoff_ms.max(stretch.start_ms),
            });
        }
    }

    /// Advances every minimized app's clock by this tick's forward wall step, starts the clocks of
    /// newly minimized apps and drops (resets) the rest. The foreground app never runs a clock:
    /// when another of its windows is in front, the foreground wins. A new clock has credit only
    /// if the app was seen not minimized before (`seen_unminimized`).
    fn run_clocks(&mut self, s: &Sample, forward_ms: i64) {
        let front = s.foreground.as_ref().map(|f| &f.identity);
        // Rebuilt rather than updated in place, so an app listed twice still steps once.
        let clocks: HashMap<AppIdentity, i64> = s
            .minimized
            .iter()
            .filter(|&identity| Some(identity) != front)
            .map(|identity| {
                let left = match self.minimized.get(identity) {
                    Some(left) => left.saturating_sub(forward_ms),
                    None if self.seen_unminimized.contains(identity) => MINIMIZED_CREDIT_MS,
                    None => 0,
                };
                (identity.clone(), left)
            })
            .collect();
        // An app leaving the map is in front, restored or closed: not minimized any more.
        let left_the_map = self.minimized.keys().filter(|id| !clocks.contains_key(*id));
        self.seen_unminimized
            .extend(left_the_map.chain(front).cloned());
        self.minimized = clocks;
    }

    /// What this Active tick counts toward (ADR-0007): a Productive foreground; else the minimized
    /// app waiting in for it, unless waiting credit is off; else the foreground (Entertainment, or
    /// nothing). Also returns when
    /// the open waiting row's app ran out of credit, if it has: `now + left`, `left` being <= 0.
    fn counted_key(&self, s: &Sample) -> (Option<Key>, Option<i64>) {
        // Clock left per minimized app that may wait in: a Tracked App, Productive right now.
        // Entertainment and untracked apps never do, and sites can't (apps only).
        let waiting = self.minimized.iter().filter_map(|(identity, &left)| {
            let app = self.targets.apps.get(identity)?;
            (app.category == Category::Productive).then_some((app.id, left))
        });
        let open = self.stretch.as_ref().and_then(|st| st.open.as_ref());
        let spent_at = open
            .filter(|open| open.key.minimized)
            .and_then(|open| {
                waiting
                    .clone()
                    .find(|&(id, _)| open.key.target == TargetRef::App { id })
            })
            .filter(|&(_, left)| left <= 0)
            .map(|(_, left)| s.now_ms.saturating_add(left));

        let front = self.resolve(s.foreground.as_ref());
        if !self.rules.waiting_credit
            || front.is_some_and(|key| key.category == Category::Productive)
        {
            return (front, spent_at);
        }
        // The most credit left is the latest minimize; a tie goes to the lowest app id.
        let waiting_in = waiting
            .filter(|&(_, left)| left > 0)
            .max_by_key(|&(id, left)| (left, Reverse(id)))
            .map(|(id, _)| Key {
                target: TargetRef::App { id },
                category: Category::Productive,
                minimized: true,
            });
        (waiting_in.or(front), spent_at)
    }

    /// What the foreground counts toward this tick, if anything (plan section 4 "Resolving").
    fn resolve(&self, fg: Option<&Foreground>) -> Option<Key> {
        let fg = fg.filter(|f| !f.own_process)?;
        let key = |target, category| Key {
            target,
            category,
            minimized: false,
        };
        let app = self
            .targets
            .apps
            .get(&fg.identity)
            .map(|app| key(TargetRef::App { id: app.id }, app.category));
        let Some(view) = &fg.browser else {
            return app;
        };
        if view.private {
            return None;
        }
        let site = match &view.domain {
            Some(domain) => normalize_domain(domain)
                .ok()
                .and_then(|d| self.site_by_domain(&d)),
            None => self.site_by_title(&fold(&fg.title)),
        };
        // A Tracked Site always overrides its browser.
        site.map(|s| key(TargetRef::Site { id: s.id }, s.category))
            .or(app)
    }

    /// Equal or a subdomain; the most specific tracked domain wins.
    fn site_by_domain(&self, domain: &str) -> Option<&TrackedSite> {
        self.targets
            .sites
            .iter()
            .filter(|s| {
                domain
                    .strip_suffix(s.domain.as_str())
                    .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
            })
            .max_by_key(|s| s.domain.len())
    }

    /// The site whose keyword appears as a whole word in the folded title; the longest matching
    /// keyword wins.
    fn site_by_title(&self, title: &str) -> Option<&TrackedSite> {
        self.targets
            .sites
            .iter()
            .filter_map(|s| {
                let longest = s
                    .keyword
                    .as_deref()?
                    .split(',')
                    .map(str::trim)
                    .filter(|keyword| contains_word(title, keyword))
                    .map(|keyword| keyword.chars().count())
                    .max()?;
                Some((s, longest))
            })
            .max_by_key(|&(_, chars)| chars)
            .map(|(s, _)| s)
    }
}

/// Advances the open row by one Active tick: extend (rolling at `ROLL_MS`), or close and reopen
/// when the key changed. A change closes the old row at `switch_ms`, where the new one starts:
/// `now`, or earlier when a waiting row's credit ran out between ticks. `now` never decreases
/// within a stretch (`Stretch::latest_ms`) and `switch_ms` lies between the open row's start and
/// `now`, so rows tile without overlap.
fn track(
    slot: &mut Option<OpenRow>,
    key: Option<Key>,
    now_ms: i64,
    switch_ms: i64,
    ops: &mut Vec<Op>,
) {
    match (slot, key) {
        (Some(open), Some(key)) if open.key == key => {
            // Roll at an exact instant rather than at the tick, so no row exceeds ROLL_MS even
            // when ticks arrive late.
            while now_ms - open.start_ms >= ROLL_MS {
                let at = open.start_ms + ROLL_MS;
                ops.push(Op::Close { end_ms: at });
                ops.push(Op::Open { key, start_ms: at });
                open.start_ms = at;
            }
            ops.push(Op::Extend { end_ms: now_ms });
        }
        (slot, key) => {
            let start_ms = if slot.take().is_some() {
                ops.push(Op::Close { end_ms: switch_ms });
                switch_ms
            } else {
                now_ms
            };
            if let Some(key) = key {
                *slot = Some(OpenRow { key, start_ms });
                ops.push(Op::Open { key, start_ms });
                if start_ms < now_ms {
                    ops.push(Op::Extend { end_ms: now_ms });
                }
            }
        }
    }
}

fn is_gap(prev: Mark, s: &Sample) -> bool {
    let wall = s.now_ms.saturating_sub(prev.now_ms);
    let mono = i64::try_from(s.mono_ms.saturating_sub(prev.mono_ms)).unwrap_or(i64::MAX);
    wall > GAP_WALL_MS || wall.abs_diff(mono) > GAP_DRIFT_MS
}

/// Case-folds and strips zero-width characters, so titles and keywords compare alike.
pub fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// `needle` occurs in `haystack` without starting or ending inside a word: an alphanumeric
/// first (last) character of the needle must not follow (precede) another one. So "x" never
/// matches inside "Excel", while ": r/", whose edges are punctuation, matches "post : r/rust".
fn contains_word(haystack: &str, needle: &str) -> bool {
    let word_at_start = needle.chars().next().is_some_and(char::is_alphanumeric);
    let word_at_end = needle
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric);
    !needle.is_empty()
        && haystack.match_indices(needle).any(|(at, _)| {
            let before = haystack[..at].chars().next_back();
            let after = haystack[at + needle.len()..].chars().next();
            let inside_a_word = (word_at_start && before.is_some_and(char::is_alphanumeric))
                || (word_at_end && after.is_some_and(char::is_alphanumeric));
            !inside_a_word
        })
}
