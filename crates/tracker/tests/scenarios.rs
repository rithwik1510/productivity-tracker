//! Plan section 8.1 scenarios: the real engine and store on a temp SQLite DB, driven by a scripted
//! fake `Sampler` with a fake clock. One behaviour per test.
//!
//! Timing convention: tick `k` happens `k` seconds after the scenario starts. A stretch's first
//! tick opens its row, so `work(61)` from a standing start counts 60 s (t1..t61).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, process, thread};

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};
use tracker::days::{day_bounds, day_of, next_day_start};
use tracker::{Reader, RuntimeHandle, Tracker};
use tracker_api::{
    ApiError, AppIdentity, Appearance, BrowserView, Category, Control, DayItem, ExportFormat,
    Foreground, NowTracking, Sample, Sampler, Site, TrackerStatus,
};

use Category::{Entertainment, Productive};

const ZONE: &str = "America/New_York";
const SEC: i64 = 1_000;
const HOUR: i64 = 3_600_000;
const MIN: i64 = 60 * SEC;

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

/// A scratch data dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("pt-scenario-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The fake machine: a scripted `Sampler` over a fake wall clock and monotonic clock.
struct Machine {
    now_ms: i64,
    mono_ms: u64,
    /// Like `GetLastInputInfo`, input is timed on the monotonic clock, so wall jumps don't move it.
    last_input_mono_ms: u64,
    away: bool,
    unlocked: bool,
    audible: bool,
    foreground: Option<Foreground>,
    minimized: Vec<AppIdentity>,
}

impl Sampler for Machine {
    fn sample(&mut self) -> Sample {
        Sample {
            now_ms: self.now_ms,
            mono_ms: self.mono_ms,
            away_signal: self.away,
            idle_ms: u32::try_from(self.mono_ms - self.last_input_mono_ms).unwrap(),
            input_event: std::mem::take(&mut self.unlocked),
            foreground: self.foreground.clone().map(|f| Foreground {
                audible: self.audible,
                ..f
            }),
            minimized: self.minimized.clone(),
        }
    }
}

/// A `Tracker` on a temp DB plus the machine feeding it. Fields drop in order, so the DB
/// connection closes before the dir is removed.
struct Sim {
    tracker: Tracker,
    machine: Machine,
    start_ms: i64,
    /// Writes fail on purpose (`fail_writes`), so ticks may report errors.
    failing: bool,
    dir: TempDir,
}

impl Sim {
    /// Starts at 10:00 New York time on 2026-10-21, idle for hours, with waiting credit on (a new
    /// install starts with it off; `a_new_install_starts_with_waiting_credit_off`).
    fn new(name: &str) -> Self {
        Self::at(name, "2026-10-21T10:00")
    }

    fn at(name: &str, local: &str) -> Self {
        let dir = TempDir::new(name);
        let start: Zoned = format!("{local}[{ZONE}]").parse().unwrap();
        let start_ms = start.timestamp().as_millisecond();
        let mut sim = Self {
            tracker: Tracker::open(&dir.0).unwrap(),
            machine: Machine {
                now_ms: start_ms,
                mono_ms: 10 * HOUR as u64,
                last_input_mono_ms: 0,
                away: false,
                unlocked: false,
                audible: false,
                foreground: None,
                minimized: Vec::new(),
            },
            start_ms,
            failing: false,
            dir,
        };
        sim.waiting_credit(true);
        sim
    }

    /// The wall time of tick `k`, for scenarios without wall jumps before it.
    fn t(&self, k: i64) -> i64 {
        self.start_ms + k * SEC
    }

    /// One tick after the wall and monotonic clocks advance by the given amounts.
    fn step(&mut self, wall_ms: i64, mono_ms: u64, input: bool) {
        self.machine.now_ms += wall_ms;
        self.machine.mono_ms += mono_ms;
        if input {
            self.machine.last_input_mono_ms = self.machine.mono_ms;
        }
        let sample = self.machine.sample();
        let result = self.tracker.tick(&sample);
        if !self.failing {
            result.unwrap();
        }
    }

    /// `ticks` seconds with keyboard/mouse input on every tick.
    fn work(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.step(SEC, SEC as u64, true);
        }
    }

    /// `ticks` seconds without input.
    fn idle(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.step(SEC, SEC as u64, false);
        }
    }

    fn focus(&mut self, foreground: Foreground) {
        self.machine.foreground = Some(foreground);
    }

    fn audible(&mut self, audible: bool) {
        self.machine.audible = audible;
    }

    /// One of the app's windows is minimized from the next tick on.
    fn minimize(&mut self, exe: &str) {
        self.machine.minimized.push(exe_id(exe));
    }

    /// Before the first tick: one tick at t0 per app with it in front and the user idle for hours,
    /// so nothing counts. The engine has then seen these apps not minimized, so minimizing one
    /// starts its clock; an app that is already minimized when the tracker starts gets no credit.
    fn shown(&mut self, exes: &[&str]) {
        assert_eq!(
            self.machine.now_ms, self.start_ms,
            "before the first tick only"
        );
        let foreground = self.machine.foreground.take();
        for exe in exes {
            self.focus(app(exe));
            self.step(0, 0, false);
        }
        self.machine.foreground = foreground;
    }

    /// The app has no minimized window from the next tick on: restored, or closed.
    fn unminimize(&mut self, exe: &str) {
        self.machine.minimized.retain(|id| *id != exe_id(exe));
    }

    fn lock(&mut self) {
        self.machine.away = true;
    }

    fn unlock(&mut self) {
        self.machine.away = false;
        self.machine.unlocked = true;
    }

    fn shutdown(&mut self) {
        self.tracker.shutdown().unwrap();
    }

    /// Drops the tracker without a shutdown and reopens the DB, as after a crash.
    fn crash(&mut self) {
        self.tracker = Tracker::open(&self.dir.0).unwrap();
    }

    fn control<T>(&mut self, message: impl FnOnce(Sender<Result<T, ApiError>>) -> Control) -> T {
        let (reply, rx) = mpsc::channel();
        assert!(self.tracker.handle(message(reply)).is_continue());
        rx.recv().unwrap().unwrap()
    }

    fn track_app(&mut self, exe: &str, category: Option<Category>) {
        self.control(|reply| Control::SetAppCategory {
            identity: exe_id(exe),
            name: exe.into(),
            category,
            reply,
        });
    }

    fn track_site(&mut self, domain: &str, keyword: Option<&str>, category: Category) -> Site {
        self.control(|reply| Control::UpsertSite {
            domain: domain.into(),
            keyword: keyword.map(Into::into),
            category: Some(category),
            reply,
        })
    }

    /// Pauses until `until_ms`, or resumes with `None`.
    fn pause(&mut self, until_ms: Option<i64>) {
        self.control(|reply| Control::SetPause { until_ms, reply });
    }

    fn away_after(&mut self, minutes: u32) {
        self.control(|reply| Control::SetAwayAfter { minutes, reply });
    }

    fn waiting_credit(&mut self, enabled: bool) {
        self.control(|reply| Control::SetWaitingCredit { enabled, reply });
    }

    /// What the status line names as tracked, if it says "Tracking".
    fn tracking(&self) -> Option<NowTracking> {
        match self.tracker.status() {
            TrackerStatus::Tracking(now) => Some(now),
            _ => None,
        }
    }

    /// From the next tick on, every write that matches the SQL trigger condition `when` fails, as
    /// a full disk or a broken file would (`heal` ends it). A trigger rather than a lock, so the
    /// writes fail at once and only the chosen ones do. The writer's connection sees the new
    /// schema on its next statement.
    fn fail_writes(&mut self, event: &str, when: &str) {
        self.db()
            .execute_batch(&format!(
                "CREATE TRIGGER fail_writes BEFORE {event} WHEN {when}
                 BEGIN SELECT RAISE(ABORT, 'disk full'); END;"
            ))
            .unwrap();
        self.failing = true;
    }

    fn heal(&mut self) {
        self.db().execute_batch("DROP TRIGGER fail_writes").unwrap();
        self.failing = false;
    }

    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.dir.0.join("tracker.db")).unwrap()
    }

    /// A reader whose "now" is the machine's current wall time.
    fn reader(&self) -> Reader {
        let now = self.machine.now_ms;
        Reader::open(&self.dir.0, tz, move || now).unwrap()
    }

    /// Seconds today's Day holds for `name` under `category` (0 if absent).
    fn secs(&self, name: &str, category: Category) -> u32 {
        let today = self.reader().day(None).unwrap();
        today
            .items
            .iter()
            .filter(|i| i.name == name && i.category == category)
            .map(|i| i.secs)
            .sum()
    }

    /// The part of `secs` that came from waiting credit.
    fn waiting_secs(&self, name: &str, category: Category) -> u32 {
        let today = self.reader().day(None).unwrap();
        today
            .items
            .iter()
            .filter(|i| i.name == name && i.category == category)
            .map(|i| i.minimized_secs)
            .sum()
    }

    /// Every interval row as `(start_ms, end_ms, open)`, oldest first.
    fn rows(&self) -> Vec<(i64, i64, bool)> {
        self.rows_where("1")
    }

    /// The waiting-credit rows (`minimized = 1`) as `(start_ms, end_ms, open)`, oldest first.
    fn waiting_rows(&self) -> Vec<(i64, i64, bool)> {
        self.rows_where("minimized")
    }

    /// ADR-0007's invariant, every second counts once: rows never overlap, so everything stored
    /// fits in the scenario's `active_ms` of Active wall time.
    fn assert_counted_once(&self, active_ms: i64) {
        let mut rows = self.rows();
        rows.sort_unstable();
        for pair in rows.windows(2) {
            assert!(pair[1].0 >= pair[0].1, "rows overlap: {pair:?}");
        }
        let stored: i64 = rows.iter().map(|&(start, end, _)| end - start).sum();
        assert!(stored <= active_ms, "{stored} ms stored in {active_ms} ms");
    }

    fn rows_where(&self, filter: &str) -> Vec<(i64, i64, bool)> {
        let conn = rusqlite::Connection::open(self.dir.0.join("tracker.db")).unwrap();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT start_ms, end_ms, open FROM interval WHERE {filter} ORDER BY id"
            ))
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
}

fn tz() -> TimeZone {
    TimeZone::get(ZONE).unwrap()
}

fn exe_id(exe: &str) -> AppIdentity {
    AppIdentity::Exe { key: exe.into() }
}

fn app(exe: &str) -> Foreground {
    Foreground {
        identity: exe_id(exe),
        own_process: false,
        audible: false,
        browser: None,
        title: String::new(),
    }
}

/// A browser window: `domain: None` means the address bar gave nothing reliable this tick.
fn tab(exe: &str, domain: Option<&str>, title: &str, private: bool) -> Foreground {
    Foreground {
        browser: Some(BrowserView {
            private,
            domain: domain.map(Into::into),
        }),
        title: title.into(),
        ..app(exe)
    }
}

fn ms(text: &str) -> i64 {
    text.parse::<Timestamp>().unwrap().as_millisecond()
}

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

// ---------------------------------------------------------------------------------------------
// Presence
// ---------------------------------------------------------------------------------------------

#[test]
fn idle_trims_back_to_the_last_input() {
    let mut sim = Sim::new("idle-trim");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(240);
    // Still Active 4 min after the last input; the flush at t301 shows the full 300 s.
    assert_eq!(sim.secs("code.exe", Productive), 300);
    sim.idle(60);
    // 5 min without evidence: Away, trimmed back to the input at t61.
    assert_eq!(sim.secs("code.exe", Productive), 60);
}

#[test]
fn audible_video_then_silence_counts_fully() {
    let mut sim = Sim::new("video");
    sim.track_app("vlc.exe", Some(Entertainment));
    sim.focus(app("vlc.exe"));
    sim.work(1);
    sim.audible(true);
    sim.idle(2400);
    sim.audible(false);
    sim.idle(300);
    // Away 5 min after the sound stopped, trimmed back to the last sound: the whole 40 min.
    assert_eq!(sim.secs("vlc.exe", Entertainment), 40 * 60);
}

#[test]
fn sound_cap_trims_back_to_the_last_input() {
    let mut sim = Sim::new("cap");
    sim.track_app("vlc.exe", Some(Entertainment));
    sim.focus(app("vlc.exe"));
    sim.work(61);
    sim.audible(true);
    sim.idle(9000);
    // 2.5 h of sound without input: the 2 h cap made it Away and kept only time up to the
    // last input; both rolled rows after it are gone and the sound after the cap never counted.
    assert_eq!(sim.secs("vlc.exe", Entertainment), 60);
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(61), false)]);
}

#[test]
fn lock_trims_idle_minutes_and_win_l_noise() {
    let mut sim = Sim::new("lock");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(240);
    // Win+L is itself a keystroke, one tick before the lock notification lands.
    sim.work(1);
    sim.lock();
    sim.idle(600);
    // Neither the 4 idle minutes nor the Win+L keystroke count, and nothing while locked.
    assert_eq!(sim.secs("code.exe", Productive), 60);
}

#[test]
fn unlock_without_input_then_sound_stays_active() {
    let mut sim = Sim::new("unlock");
    sim.track_app("vlc.exe", Some(Entertainment));
    sim.focus(app("vlc.exe"));
    sim.lock();
    sim.idle(60);
    sim.unlock();
    sim.audible(true);
    // 10 min of sound with no input at all since the unlock.
    sim.idle(601);
    sim.audible(false);
    sim.idle(300);
    // The unlock counted as input and sound kept it Active past 5 min: 600 s from the unlock.
    assert_eq!(sim.secs("vlc.exe", Entertainment), 600);
}

#[test]
fn forward_gap_ends_the_stretch_at_the_last_evidence() {
    let mut sim = Sim::new("gap-forward");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(120);
    // A missed 2 h suspend, woken by a keypress.
    sim.step(2 * HOUR, 2 * HOUR as u64, true);
    let resumed = sim.machine.now_ms;
    sim.work(30);
    sim.shutdown();
    // The 2 idle minutes before the suspend and the suspend itself don't count; the resumed
    // work is a new stretch from the waking tick.
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(61), false),
            (resumed, resumed + 30 * SEC, false)
        ]
    );
}

#[test]
fn suspend_after_uneven_ticks_keeps_the_stretch() {
    let mut sim = Sim::new("gap-jitter");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(8);
    // Real ticks aren't exactly 1 s apart (timer lateness, a slow UIA read, the catch-up tick
    // after a stall). Here the last three before a suspend span 1.98 s.
    sim.step(990, 990, false);
    sim.step(990, 990, false);
    // A missed 40 min suspend, woken by a keypress.
    sim.step(40 * 60 * SEC, 40 * 60 * SEC as u64, true);
    let resumed = sim.machine.now_ms;
    sim.work(10);
    sim.shutdown();
    // The 2 s lookback still finds a tick to read the evidence from, so the stretch before the
    // suspend keeps its time up to the last input instead of being wiped back to its start.
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(61), false),
            (resumed, resumed + 10 * SEC, false)
        ]
    );
}

#[test]
fn small_backward_clock_step_never_overlaps_rows() {
    let mut sim = Sim::new("step-back");
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("winword.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(31);
    // A time sync steps the wall clock back 1.5 s (under the 2 s gap threshold) on the very tick
    // that sees a switch to Word.
    sim.focus(app("winword.exe"));
    sim.step(-500, SEC as u64, true);
    sim.work(30);
    sim.shutdown();
    let rows = sim.rows();
    // Word starts where VS Code ended, not 0.5 s before it: no wall instant counts twice. The
    // step costs its 1.5 s, which is the price of never overlapping.
    assert_eq!(rows[0], (sim.t(1), sim.t(31), false));
    assert_eq!(rows[1], (sim.t(31), sim.t(61) - 500, false));
    assert_eq!(sim.secs("code.exe", Productive), 30);
    assert_eq!(sim.secs("winword.exe", Productive), 29);
}

#[test]
fn backward_gap_resets_evidence() {
    let mut sim = Sim::new("gap-backward");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(10);
    // The wall clock is set back an hour while the monotonic clock moves 1 s.
    sim.step(-HOUR, SEC as u64, true);
    sim.work(30);
    sim.idle(300);
    // If the pre-jump input (now an hour in the "future") still counted as evidence, the user
    // would stay Active through these 5 idle minutes. Instead the second stretch ends at its own
    // last input: 60 s + 30 s.
    assert_eq!(sim.secs("code.exe", Productive), 90);
}

/// Through the real `RuntimeHandle`: poll thread, deadline ticks, Control, shutdown, reader.
#[test]
fn shutdown_trims_idle_time() {
    struct LiveSampler {
        origin: Instant,
        last_input: Instant,
        ticks: Arc<AtomicU32>,
    }
    impl Sampler for LiveSampler {
        fn sample(&mut self) -> Sample {
            // Input on the first 3 ticks only.
            if self.ticks.fetch_add(1, Ordering::SeqCst) < 3 {
                self.last_input = Instant::now();
            }
            Sample {
                now_ms: Timestamp::now().as_millisecond(),
                mono_ms: u64::try_from(self.origin.elapsed().as_millis()).unwrap(),
                away_signal: false,
                idle_ms: u32::try_from(self.last_input.elapsed().as_millis()).unwrap(),
                input_event: false,
                foreground: Some(app("code.exe")),
                minimized: Vec::new(),
            }
        }
    }

    let dir = TempDir::new("shutdown");
    let ticks = Arc::new(AtomicU32::new(0));
    let sampler_thread = Arc::new(Mutex::new(None));
    let runtime = RuntimeHandle::start(dir.0.clone(), {
        let (ticks, sampler_thread) = (ticks.clone(), sampler_thread.clone());
        move || {
            *sampler_thread.lock().unwrap() = thread::current().name().map(str::to_owned);
            Box::new(LiveSampler {
                origin: Instant::now(),
                last_input: Instant::now(),
                ticks,
            })
        }
    })
    .unwrap();
    let (reply, rx) = mpsc::channel();
    runtime
        .send(Control::SetAppCategory {
            identity: exe_id("code.exe"),
            name: "code.exe".into(),
            category: Some(Productive),
            reply,
        })
        .unwrap();
    rx.recv().unwrap().unwrap();

    while ticks.load(Ordering::SeqCst) < 7 {
        thread::sleep(Duration::from_millis(50));
    }
    runtime.shutdown_and_wait(Duration::from_secs(3));
    runtime.shutdown_and_wait(Duration::from_secs(3));

    assert_eq!(
        sampler_thread.lock().unwrap().as_deref(),
        Some("tracker-poll")
    );
    let (reply, _rx) = mpsc::channel();
    assert!(runtime.send(Control::CompleteOnboarding { reply }).is_err());
    let today = runtime.reader().unwrap().day(None).unwrap();
    let code = today.items.iter().find(|i| i.name == "code.exe").unwrap();
    // A ~6 s stretch whose last input was ~2 s after it began: only those ~2 s are kept.
    assert!((1..=2).contains(&code.secs), "kept {} s", code.secs);
}

// ---------------------------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------------------------

#[test]
fn browser_precedence_and_the_most_specific_domain() {
    let mut sim = Sim::new("precedence");
    sim.track_app("chrome.exe", Some(Productive));
    sim.track_site("google.com", None, Entertainment);
    sim.track_site("docs.google.com", None, Productive);
    sim.focus(tab("chrome.exe", Some("docs.google.com"), "", false));
    sim.work(10);
    sim.focus(tab("chrome.exe", Some("mail.google.com"), "", false));
    sim.work(10);
    sim.focus(tab("chrome.exe", Some("example.com"), "", false));
    sim.work(10);
    sim.focus(tab("chrome.exe", Some("notgoogle.com"), "", false));
    sim.work(10);
    sim.track_app("chrome.exe", None);
    sim.focus(tab("chrome.exe", Some("example.com"), "", false));
    sim.work(10);
    sim.shutdown();
    // docs.google.com beats google.com; mail.google.com falls to google.com; other sites
    // (notgoogle.com isn't a subdomain) use the browser's Category; nothing once it's untracked.
    assert_eq!(sim.secs("docs.google.com", Productive), 10);
    assert_eq!(sim.secs("google.com", Entertainment), 10);
    assert_eq!(sim.secs("chrome.exe", Productive), 20);
    assert_eq!(sim.rows().len(), 3);
}

#[test]
fn private_windows_count_nothing() {
    let mut sim = Sim::new("private");
    sim.track_app("chrome.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(tab("chrome.exe", Some("youtube.com"), "", false));
    sim.work(10);
    sim.focus(tab("chrome.exe", Some("youtube.com"), "", true));
    sim.work(30);
    sim.focus(tab("chrome.exe", Some("example.com"), "", true));
    sim.work(30);
    sim.shutdown();
    // Only the normal window's 10 s; private windows count neither the site nor the browser.
    assert_eq!(sim.secs("youtube.com", Entertainment), 10);
    assert_eq!(sim.secs("chrome.exe", Productive), 0);
    assert_eq!(sim.rows().len(), 1);
}

#[test]
fn title_fallback_matches_whole_words() {
    let mut sim = Sim::new("title");
    sim.track_site("x.com", Some("/ x"), Entertainment);
    let youtube = sim.track_site("youtube.com", None, Entertainment);
    assert_eq!(youtube.keyword.as_deref(), Some("youtube"));
    // Firefox is title-only and isn't a Tracked App, so only keyword matches count.
    let titles = [
        "Home / X — Mozilla Firefox",
        "Inbox (3) - Outlook — Mozilla Firefox",
        "Excel tips and tricks — Mozilla Firefox",
        "Top YouTubers of 2026 — Mozilla Firefox",
        "Lo-fi beats - You\u{200B}Tube — Mozilla Firefox",
        "YouTube / X — Mozilla Firefox",
        "New Tab — Mozilla Firefox",
    ];
    for title in titles {
        sim.focus(tab("firefox.exe", None, title, false));
        sim.work(10);
    }
    sim.shutdown();
    // "/ x" matches only the X title; "YouTubers" isn't the word "youtube"; zero-width chars
    // are ignored; with both keywords present the longer one wins.
    assert_eq!(sim.secs("x.com", Entertainment), 10);
    assert_eq!(sim.secs("youtube.com", Entertainment), 20);
    assert_eq!(sim.rows().len(), 2);
}

#[test]
fn a_keyword_can_list_several() {
    let mut sim = Sim::new("keywords");
    // Reddit's preset: its home page says "Reddit", a post says "Title : r/sub".
    let reddit = sim.track_site("reddit.com", Some(" Reddit,: r/ "), Entertainment);
    assert_eq!(reddit.keyword.as_deref(), Some("reddit, : r/"));
    let titles = [
        "Reddit - The heart of the internet — Mozilla Firefox",
        "Why is Rust fast? : r/rust — Mozilla Firefox",
        "r/rust on Reddit: Why is Rust fast? — Mozilla Firefox",
        "AskReddit tips — Mozilla Firefox",
        "owner/repo: a fork — Mozilla Firefox",
    ];
    for title in titles {
        sim.focus(tab("firefox.exe", None, title, false));
        sim.work(10);
    }
    sim.shutdown();
    // The first three, in one row: neither keyword occurs as a whole word in the last two.
    assert_eq!(sim.secs("reddit.com", Entertainment), 30);
    assert_eq!(sim.rows().len(), 1);
    // Every keyword in the list needs 3 characters.
    let (reply, rx) = mpsc::channel();
    assert!(
        sim.tracker
            .handle(Control::UpsertSite {
                domain: "reddit.com".into(),
                keyword: Some("reddit, r/".into()),
                category: Some(Entertainment),
                reply,
            })
            .is_continue()
    );
    assert!(rx.recv().unwrap().is_err());
}

/// `get_now`: what the last tick counted, or `None` whenever nothing counts.
#[test]
fn now_names_what_counts_this_tick() {
    let mut sim = Sim::new("now");
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("chrome.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    let now = |sim: &Sim| {
        sim.tracking()
            .map(|n| (n.name, n.category, n.is_browser, n.icon_path))
    };
    assert_eq!(now(&sim), None);

    sim.focus(app("notepad.exe"));
    sim.work(2);
    assert_eq!(now(&sim), None);
    sim.focus(app("code.exe"));
    sim.work(2);
    assert_eq!(
        now(&sim),
        Some(("code.exe".into(), Productive, false, None))
    );
    // A site shows by domain; other sites fall to the browser, flagged as one.
    sim.focus(tab("chrome.exe", Some("www.youtube.com"), "", false));
    sim.work(2);
    assert_eq!(
        now(&sim),
        Some(("youtube.com".into(), Entertainment, false, None))
    );
    sim.focus(tab("chrome.exe", Some("example.com"), "", false));
    sim.work(2);
    assert_eq!(
        now(&sim),
        Some(("chrome.exe".into(), Productive, true, None))
    );
    sim.focus(tab("chrome.exe", Some("youtube.com"), "", true));
    sim.work(2);
    assert_eq!(now(&sim), None);
    sim.focus(app("code.exe"));
    sim.work(2);
    sim.lock();
    sim.idle(1);
    assert_eq!(now(&sim), None);
}

/// `get_status`: the status line's state, and for Away the idle time its trim just removed.
#[test]
fn the_status_follows_presence_and_explains_a_trim() {
    let mut sim = Sim::new("status");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("notepad.exe"));
    sim.work(1);
    // There, with nothing tracked in front.
    assert_eq!(sim.tracker.status(), TrackerStatus::Untracked);
    sim.focus(app("code.exe"));
    sim.work(60);
    assert!(matches!(sim.tracker.status(), TrackerStatus::Tracking(_)));
    sim.idle(300);
    // Away 5 min after the input at t61: the row had grown to t360, and the trim took that back.
    assert_eq!(
        sim.tracker.status(),
        TrackerStatus::Away { removed_secs: 299 }
    );
    assert_eq!(sim.secs("code.exe", Productive), 59);
    sim.work(1);
    assert!(matches!(sim.tracker.status(), TrackerStatus::Tracking(_)));
    // A lock right away removes nothing, so the line says plain "Away".
    sim.lock();
    sim.idle(1);
    assert_eq!(
        sim.tracker.status(),
        TrackerStatus::Away { removed_secs: 0 }
    );
    sim.unlock();
    sim.work(120);
    sim.idle(60);
    // A pause trims the idle minute too, but a pause isn't Away: once it ends with the user
    // locked away, nothing explains a drop.
    sim.pause(Some(sim.machine.now_ms + HOUR));
    assert_eq!(sim.tracker.status(), TrackerStatus::Paused);
    sim.lock();
    sim.pause(None);
    sim.idle(1);
    assert_eq!(
        sim.tracker.status(),
        TrackerStatus::Away { removed_secs: 0 }
    );
}

// ---------------------------------------------------------------------------------------------
// Intervals
// ---------------------------------------------------------------------------------------------

#[test]
fn category_change_mid_interval_splits_it() {
    let mut sim = Sim::new("split");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(30);
    sim.track_app("code.exe", Some(Entertainment));
    sim.work(31);
    sim.shutdown();
    // The next tick after the change closes the Productive row and opens an Entertainment one.
    assert_eq!(sim.secs("code.exe", Productive), 30);
    assert_eq!(sim.secs("code.exe", Entertainment), 30);
}

#[test]
fn open_row_rolls_before_an_hour() {
    let mut sim = Sim::new("roll");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(9001);
    sim.shutdown();
    // 2.5 h as contiguous rows of exactly 1 h - 1 tick, then the remainder.
    let roll = 3599;
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(1 + roll), false),
            (sim.t(1 + roll), sim.t(1 + 2 * roll), false),
            (sim.t(1 + 2 * roll), sim.t(9001), false),
        ]
    );
    assert_eq!(sim.secs("code.exe", Productive), 9000);
}

#[test]
fn end_stretch_cutoff_before_a_rolled_row() {
    let mut sim = Sim::new("trim-roll");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(3481);
    // The row rolls at t3600, during these idle minutes, then the user goes Away.
    sim.idle(300);
    // The rolled row started after the cutoff (t3481), so it's deleted; the first is clamped.
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(3481), false)]);
}

#[test]
fn end_stretch_cutoff_before_a_target_change_row() {
    let mut sim = Sim::new("trim-switch");
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("winword.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(120);
    // A window steals focus without any input, then the user goes Away.
    sim.focus(app("winword.exe"));
    sim.idle(180);
    // The Word row started after the cutoff (t61) and is deleted; the code row is clamped.
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(61), false)]);
    assert_eq!(sim.secs("winword.exe", Productive), 0);
}

#[test]
fn end_stretch_never_touches_a_previous_stretch() {
    let mut sim = Sim::new("trim-scope");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(300);
    // While Away the clock is set back an hour; input then starts a second stretch.
    sim.step(-HOUR, SEC as u64, true);
    let second = sim.machine.now_ms;
    sim.work(30);
    sim.lock();
    sim.idle(5);
    // The second stretch's cutoff lies an hour before the first stretch's row, which survives.
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(61), false),
            (second, second + 29 * SEC, false)
        ]
    );
}

#[test]
fn failed_end_stretch_is_retried_next_tick() {
    let mut sim = Sim::new("trim-retry");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(10);
    // Another connection holds the write lock, so the lock's trim fails after the busy timeout.
    let blocker = rusqlite::Connection::open(sim.dir.0.join("tracker.db")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    sim.lock();
    sim.machine.now_ms += SEC;
    sim.machine.mono_ms += SEC as u64;
    assert!(sim.tracker.tick(&sim.machine.sample()).is_err());
    drop(blocker);
    // The next tick retries it: trimmed back to the input at t61 and closed.
    sim.idle(1);
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(61), false)]);
}

#[test]
fn a_failed_open_reopens_once_the_store_recovers() {
    let mut sim = Sim::new("fail-open");
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("winword.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(31);
    // From the switch to Word on, every INSERT fails: VS Code's row closes, Word's never opens.
    sim.fail_writes("INSERT ON interval", "1");
    sim.focus(app("winword.exe"));
    sim.work(10);
    assert_eq!(sim.tracker.status(), TrackerStatus::SaveFailed);
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(32), false)]);
    sim.heal();
    sim.work(20);
    assert_eq!(sim.tracking().map(|n| n.name), Some("winword.exe".into()));
    sim.shutdown();
    // Word counts from the first tick that could save it (t42), not from the switch: the 10 s in
    // between were never stored, so they never count.
    assert_eq!(
        sim.rows(),
        vec![(sim.t(1), sim.t(32), false), (sim.t(42), sim.t(61), false)]
    );
    sim.assert_counted_once(60 * SEC);
}

#[test]
fn a_failed_close_keeps_its_row_until_it_lands() {
    let mut sim = Sim::new("fail-close");
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("winword.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(31);
    // Every UPDATE that closes a row fails; the flushes of an open row's end still work.
    sim.fail_writes("UPDATE ON interval", "NEW.open = 0");
    sim.focus(app("winword.exe"));
    sim.work(10);
    assert_eq!(sim.tracker.status(), TrackerStatus::SaveFailed);
    // VS Code's row is still open, and still ends where the failed Close said (t32), not at the
    // flush before it (t31).
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(32), true)]);
    sim.heal();
    sim.work(20);
    sim.shutdown();
    assert_eq!(
        sim.rows(),
        vec![(sim.t(1), sim.t(32), false), (sim.t(42), sim.t(61), false)]
    );
    sim.assert_counted_once(60 * SEC);
}

#[test]
fn a_failed_flush_is_retried_every_tick() {
    let mut sim = Sim::new("fail-flush");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(11);
    // Writing an open row's end fails; the flush due at t16 fails, then every tick retries.
    sim.fail_writes("UPDATE ON interval", "OLD.open = 1 AND NEW.open = 1");
    sim.work(7);
    assert_eq!(sim.tracker.status(), TrackerStatus::SaveFailed);
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(11), true)]);
    // The first tick after the store recovers saves everything, without waiting for t21.
    sim.heal();
    sim.work(1);
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(19), true)]);
    assert!(matches!(sim.tracker.status(), TrackerStatus::Tracking(_)));
    // Failing again, then a crash: what was never flushed is lost, never invented.
    sim.fail_writes("UPDATE ON interval", "OLD.open = 1 AND NEW.open = 1");
    sim.work(10);
    sim.crash();
    sim.heal();
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(19), false)]);
}

#[test]
fn a_failed_trim_after_a_sleep_still_counts_the_new_stretch() {
    let mut sim = Sim::new("fail-trim-open");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(10);
    // The trim's UPDATE fails. A missed 40 min suspend, woken by a keypress, ends the stretch and
    // opens a new one in the same tick; the new row must wait for the trim.
    sim.fail_writes("UPDATE ON interval", "NEW.open = 0");
    sim.step(40 * MIN, 40 * MIN as u64, true);
    let resumed = sim.machine.now_ms;
    sim.work(9);
    assert_eq!(sim.tracker.status(), TrackerStatus::SaveFailed);
    sim.heal();
    sim.work(10);
    sim.shutdown();
    // The old stretch is trimmed to its last input; the new one counts from the tick that could
    // save it.
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(61), false),
            (resumed + 10 * SEC, resumed + 19 * SEC, false)
        ]
    );
}

#[test]
fn crash_recovery_closes_open_rows_at_their_last_flush() {
    let mut sim = Sim::new("crash");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(14);
    sim.crash();
    // Flushed at t11; the 3 s since are lost and the row is no longer open.
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(11), false)]);
    // A row opened and never flushed has no length, so recovery drops it.
    sim.work(1);
    sim.crash();
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(11), false)]);
}

#[test]
fn crash_during_idle_minutes_trims_like_a_shutdown() {
    let mut sim = Sim::new("crash-idle");
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("winword.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(120);
    // A window steals focus without input; 2 min later the app dies (killed, power loss) before
    // the 5 min rule could end the stretch.
    sim.focus(app("winword.exe"));
    sim.idle(120);
    sim.crash();
    // Recovery trims the unfinished stretch back to the last input, as a shutdown would have:
    // neither the 4 idle minutes nor the Word row count.
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(61), false)]);
    // Tracking goes on normally afterwards, and a second restart trims nothing more.
    sim.focus(app("code.exe"));
    sim.work(11);
    sim.shutdown();
    sim.crash();
    assert_eq!(sim.secs("code.exe", Productive), 70);
}

// ---------------------------------------------------------------------------------------------
// Days and queries
// ---------------------------------------------------------------------------------------------

#[test]
fn interval_crossing_4am_splits_between_days() {
    let mut sim = Sim::at("4am", "2026-10-22T03:58:59");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(121);
    sim.shutdown();
    // 03:59 to 04:01: one minute on each side of the boundary, in the Day query and in the
    // week's per-Day bucketing alike.
    let reader = sim.reader();
    for day in ["2026-10-21", "2026-10-22"] {
        assert_eq!(reader.day(Some(day)).unwrap().totals.productive_secs, 60);
    }
    let week = reader.week(Some("2026-10-19")).unwrap();
    let productive: Vec<u32> = week.days.iter().map(|d| d.productive_secs).collect();
    assert_eq!(productive, [0, 0, 60, 60, 0, 0, 0]);
}

#[test]
fn dst_days_in_new_york() {
    let tz = TimeZone::get("America/New_York").unwrap();
    let length = |day: &str| {
        let (start, end) = day_bounds(date(day), &tz).unwrap();
        (end - start) / HOUR
    };
    // Spring forward at 2am on 2026-03-08, fall back at 2am on 2026-11-01.
    assert_eq!(length("2026-03-07"), 23);
    assert_eq!(length("2026-03-08"), 24);
    assert_eq!(length("2026-10-31"), 25);
    assert_eq!(
        day_bounds(date("2026-03-08"), &tz).unwrap().0,
        ms("2026-03-08T08:00Z")
    );
    let day = |utc: &str| day_of(ms(utc), &tz).unwrap();
    assert_eq!(day("2026-03-08T07:30Z"), date("2026-03-07")); // 03:30 EDT
    assert_eq!(day("2026-03-08T08:00Z"), date("2026-03-08")); // 04:00 EDT
    assert_eq!(day("2026-11-01T06:30Z"), date("2026-10-31")); // the second 01:30, EST
}

#[test]
fn dst_days_in_helsinki() {
    let tz = TimeZone::get("Europe/Helsinki").unwrap();
    // Clocks jump 03:00 -> 04:00 on 2026-03-29 and fall back 04:00 -> 03:00 on 2026-10-25, right
    // against the Day boundary.
    assert_eq!(
        day_bounds(date("2026-03-28"), &tz).unwrap(),
        (ms("2026-03-28T02:00Z"), ms("2026-03-29T01:00Z"))
    );
    assert_eq!(
        day_bounds(date("2026-10-24"), &tz).unwrap(),
        (ms("2026-10-24T01:00Z"), ms("2026-10-25T02:00Z"))
    );
    let day = |utc: &str| day_of(ms(utc), &tz).unwrap();
    assert_eq!(day("2026-03-29T00:59:59Z"), date("2026-03-28")); // 02:59:59 EET
    assert_eq!(day("2026-03-29T01:00Z"), date("2026-03-29")); // 04:00 EEST
    assert_eq!(day("2026-10-25T00:30Z"), date("2026-10-24")); // 03:30 EEST
    assert_eq!(day("2026-10-25T01:30Z"), date("2026-10-24")); // 03:30 EET, the repeat
    assert_eq!(day("2026-10-25T02:00Z"), date("2026-10-25")); // 04:00 EET
}

#[test]
fn todays_summary_lists_tracked_targets_at_zero() {
    let mut sim = Sim::new("today");
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("chrome.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(app("code.exe"));
    sim.work(11);
    // Half a second in Chrome: a row that floors to 0 s.
    sim.focus(app("chrome.exe"));
    sim.step(500, 500, true);
    sim.focus(app("code.exe"));
    sim.step(500, 500, true);
    sim.shutdown();
    sim.track_app("code.exe", Some(Entertainment));

    let listed = |items: &[DayItem]| -> Vec<(String, Category, u32)> {
        items
            .iter()
            .map(|i| (i.name.clone(), i.category, i.secs))
            .collect()
    };
    let today = sim.reader().day(None).unwrap();
    assert!(today.is_today);
    // Every tracked target appears, code.exe at 0 s under its new Category beside its old time.
    assert_eq!(
        listed(&today.items),
        [
            ("code.exe".into(), Productive, 10),
            ("chrome.exe".into(), Productive, 0),
            ("code.exe".into(), Entertainment, 0),
            ("youtube.com".into(), Entertainment, 0),
        ]
    );

    // A day later the same Day is past and lists only what has whole seconds (not Chrome's 0.5 s).
    let tomorrow = sim.machine.now_ms + 24 * HOUR;
    let reader = Reader::open(&sim.dir.0, tz, move || tomorrow).unwrap();
    let past = reader.day(Some(&today.totals.day)).unwrap();
    assert!(!past.is_today);
    assert_eq!(listed(&past.items), [("code.exe".into(), Productive, 10)]);
}

#[test]
fn week_and_year_show_the_day_views_numbers() {
    let mut sim = Sim::new("rounding");
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("winword.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(11);
    // Switches on late ticks leave fractions: VS Code 10.6 s, then Word 10.7 s.
    sim.focus(app("winword.exe"));
    sim.step(600, 600, true);
    sim.work(10);
    sim.focus(app("code.exe"));
    sim.step(700, 700, true);
    sim.shutdown();

    let reader = sim.reader();
    let day = reader.day(None).unwrap().totals;
    // The Day view sums each item's whole seconds: 10 + 10, not the 21.3 s of raw time.
    assert_eq!(day.productive_secs, 20);
    let same_day = |days: Vec<tracker_api::DayTotals>| {
        days.into_iter()
            .find(|d| d.day == day.day)
            .unwrap()
            .productive_secs
    };
    assert_eq!(same_day(reader.week(None).unwrap().days), 20);
    assert_eq!(same_day(reader.year(None).unwrap().days), 20);
}

// ---------------------------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------------------------

/// An install from before Appearance existed: v2 adds it as Glass and keeps the other settings.
#[test]
fn v1_database_migrates_to_glass_appearance() {
    let dir = TempDir::new("migrate-v1");
    {
        let conn = rusqlite::Connection::open(dir.0.join("tracker.db")).unwrap();
        conn.execute_batch(include_str!("../src/store/schema_v1.sql"))
            .unwrap();
        conn.execute_batch(
            "UPDATE settings SET daily_goal_minutes = 120, onboarded = 1; PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let mut tracker = Tracker::open(&dir.0).unwrap();
    let reader = Reader::open(&dir.0, tz, || 0).unwrap();
    let settings = reader.settings().unwrap();
    assert_eq!(
        (
            settings.daily_goal_minutes,
            settings.onboarded,
            settings.appearance
        ),
        (120, true, Appearance::Glass)
    );
    // v5: the Tracking defaults, and no pause. An install onboarded before v5 always had waiting
    // credit, so it keeps it.
    assert_eq!(
        (settings.away_after_minutes, settings.waiting_credit),
        (5, true)
    );
    assert_eq!(reader.paused_until_ms().unwrap(), None);

    let (reply, rx) = mpsc::channel();
    assert!(
        tracker
            .handle(Control::SetAppearance {
                appearance: Appearance::Solid,
                reply,
            })
            .is_continue()
    );
    rx.recv().unwrap().unwrap();
    assert_eq!(reader.settings().unwrap().appearance, Appearance::Solid);
}

// ---------------------------------------------------------------------------------------------
// Waiting credit (ADR-0007)
// ---------------------------------------------------------------------------------------------

fn youtube() -> Foreground {
    tab("chrome.exe", Some("www.youtube.com"), "", false)
}

#[test]
fn a_productive_foreground_beats_a_minimized_app() {
    let mut sim = Sim::new("wait-front");
    sim.shown(&["claude.exe"]);
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("claude.exe", Some(Productive));
    // Claude minimized at 10:00, VS Code in front until 10:25.
    sim.focus(app("code.exe"));
    sim.minimize("claude.exe");
    sim.work(1501);
    sim.shutdown();
    assert_eq!(sim.secs("code.exe", Productive), 1500);
    assert_eq!(sim.secs("claude.exe", Productive), 0);
    sim.assert_counted_once(1500 * SEC);
}

#[test]
fn a_minimized_app_waits_in_for_youtube_for_30_minutes() {
    let mut sim = Sim::new("wait-youtube");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(youtube());
    sim.minimize("claude.exe");
    // A minute of clicking, then the video plays with no input: foreground sound is presence, so
    // the agent waits in through it.
    sim.work(61);
    // The live line names the agent, flagged as waiting.
    assert_eq!(
        sim.tracking().map(|n| (n.name, n.waiting)),
        Some(("claude.exe".into(), true))
    );
    sim.audible(true);
    sim.idle(1800);
    // Its 30 minutes ran out at t1801; YouTube counts from there.
    assert_eq!(
        sim.tracking().map(|n| (n.name, n.waiting)),
        Some(("youtube.com".into(), false))
    );
    sim.shutdown();
    assert_eq!(sim.secs("claude.exe", Productive), 1800);
    assert_eq!(sim.waiting_secs("claude.exe", Productive), 1800);
    assert_eq!(sim.secs("youtube.com", Entertainment), 60);
    assert_eq!(sim.waiting_rows(), vec![(sim.t(1), sim.t(1801), false)]);
    sim.assert_counted_once(1860 * SEC);
}

#[test]
fn vs_code_then_youtube_leaves_the_agent_the_rest_of_its_clock() {
    let mut sim = Sim::new("wait-rest");
    sim.shown(&["claude.exe"]);
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    // Claude minimized at 10:00 (t1); VS Code to 10:25, then YouTube to 11:00. The clock ran while
    // VS Code was in front, so 5 minutes are left.
    sim.focus(app("code.exe"));
    sim.minimize("claude.exe");
    sim.work(1500);
    sim.focus(youtube());
    sim.work(2101);
    sim.shutdown();
    assert_eq!(sim.secs("code.exe", Productive), 1500);
    assert_eq!(sim.secs("claude.exe", Productive), 300);
    assert_eq!(sim.secs("youtube.com", Entertainment), 1800);
    assert_eq!(sim.waiting_rows(), vec![(sim.t(1501), sim.t(1801), false)]);
    sim.assert_counted_once(3600 * SEC);
}

#[test]
fn the_latest_minimize_waits_in_first() {
    let mut sim = Sim::new("wait-latest");
    sim.shown(&["claude.exe", "codex.exe"]);
    for exe in ["code.exe", "claude.exe", "codex.exe"] {
        sim.track_app(exe, Some(Productive));
    }
    sim.track_site("youtube.com", None, Entertainment);
    // Claude minimized at 10:00 (t1, spent at t1801), Codex at 10:10 (t601, spent at t2401),
    // VS Code in front until YouTube at 10:12 (t721), which plays until 10:50.
    sim.focus(app("code.exe"));
    sim.minimize("claude.exe");
    sim.work(600);
    sim.minimize("codex.exe");
    sim.work(120);
    sim.focus(youtube());
    sim.work(2281);
    sim.shutdown();
    // Codex has more clock left, so it covers 10:12 to 10:40. Claude's clock runs out meanwhile.
    assert_eq!(sim.secs("code.exe", Productive), 720);
    assert_eq!(sim.secs("codex.exe", Productive), 1680);
    assert_eq!(sim.secs("claude.exe", Productive), 0);
    assert_eq!(sim.secs("youtube.com", Entertainment), 600);
    assert_eq!(sim.waiting_rows(), vec![(sim.t(721), sim.t(2401), false)]);
    sim.assert_counted_once(3000 * SEC);
}

#[test]
fn a_tie_goes_to_the_lowest_app_id() {
    let mut sim = Sim::new("wait-tie");
    sim.shown(&["claude.exe", "codex.exe"]);
    // Tracked first, so Claude has the lower id.
    sim.track_app("claude.exe", Some(Productive));
    sim.track_app("codex.exe", Some(Productive));
    // Both minimized on the same tick (Win+D), nothing in front.
    sim.minimize("codex.exe");
    sim.minimize("claude.exe");
    sim.work(61);
    sim.shutdown();
    assert_eq!(sim.secs("claude.exe", Productive), 60);
    assert_eq!(sim.secs("codex.exe", Productive), 0);
    sim.assert_counted_once(60 * SEC);
}

#[test]
fn away_trims_waiting_credit() {
    let mut sim = Sim::new("wait-away");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    // A render left minimized over lunch, nothing in front: Away 5 min after the last input,
    // trimmed back to it.
    sim.minimize("claude.exe");
    sim.work(61);
    sim.idle(3600);
    // Back an hour later: the clock ran through lunch and is spent.
    sim.work(60);
    assert_eq!(sim.secs("claude.exe", Productive), 60);
    // A fresh minimize, then 2 idle minutes and Win+L: trimmed back to the input before them.
    sim.unminimize("claude.exe");
    sim.work(1);
    sim.minimize("claude.exe");
    sim.work(61);
    sim.idle(120);
    sim.work(1);
    sim.lock();
    sim.idle(60);
    sim.shutdown();
    assert_eq!(
        sim.waiting_rows(),
        vec![
            (sim.t(1), sim.t(61), false),
            (sim.t(3723), sim.t(3783), false)
        ]
    );
    assert_eq!(sim.secs("claude.exe", Productive), 120);
    // Active: t1 to t61, then t3662 to the cutoff at t3783.
    sim.assert_counted_once((60 + 121) * SEC);
}

#[test]
fn untracked_private_and_empty_foregrounds_are_covered_by_productive_apps_only() {
    let mut sim = Sim::new("wait-untracked");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_app("vlc.exe", Some(Entertainment));
    // An Entertainment app, an untracked app and an untracked browser never wait in.
    sim.minimize("vlc.exe");
    sim.minimize("mspaint.exe");
    sim.minimize("chrome.exe");
    sim.focus(app("notepad.exe"));
    sim.work(61);
    assert_eq!(sim.rows(), vec![]);
    // A Productive app does: for an untracked app, a private window and the desktop, in one row.
    sim.minimize("claude.exe");
    sim.work(30);
    sim.focus(tab("chrome.exe", Some("youtube.com"), "", true));
    sim.work(30);
    sim.machine.foreground = None;
    sim.work(30);
    sim.shutdown();
    assert_eq!(sim.waiting_rows(), vec![(sim.t(62), sim.t(151), false)]);
    assert_eq!(sim.secs("vlc.exe", Entertainment), 0);
    sim.assert_counted_once(150 * SEC);
}

#[test]
fn a_productive_site_in_front_is_never_covered() {
    let mut sim = Sim::new("wait-site");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_app("chrome.exe", Some(Productive));
    sim.track_site("docs.rs", None, Productive);
    sim.track_site("youtube.com", None, Entertainment);
    sim.minimize("claude.exe");
    // A Productive site wins, and so does a Productive browser on other sites. An Entertainment
    // site in the same browser is covered.
    sim.focus(tab("chrome.exe", Some("docs.rs"), "", false));
    sim.work(61);
    sim.focus(youtube());
    sim.work(30);
    sim.focus(tab("chrome.exe", Some("example.com"), "", false));
    sim.work(30);
    sim.shutdown();
    assert_eq!(sim.secs("docs.rs", Productive), 61);
    assert_eq!(sim.secs("claude.exe", Productive), 30);
    assert_eq!(sim.secs("chrome.exe", Productive), 29);
    assert_eq!(sim.secs("youtube.com", Entertainment), 0);
    sim.assert_counted_once(120 * SEC);
}

#[test]
fn focus_or_restore_resets_the_clock() {
    let mut sim = Sim::new("wait-reset");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(youtube());
    sim.minimize("claude.exe");
    sim.work(600);
    // Another Claude window comes to the front; the minimized one stays minimized. "Claude in
    // front" is its own row, and the app in front runs no clock.
    sim.focus(app("claude.exe"));
    sim.work(10);
    // Back to YouTube: a fresh 30 min from t611.
    sim.focus(youtube());
    sim.work(1860);
    // Restored behind YouTube without coming to the front, then minimized again: that resets
    // the clock too.
    sim.unminimize("claude.exe");
    sim.work(10);
    sim.minimize("claude.exe");
    sim.work(100);
    sim.shutdown();
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(601), false),
            (sim.t(601), sim.t(611), false),
            (sim.t(611), sim.t(2411), false),
            (sim.t(2411), sim.t(2481), false),
            (sim.t(2481), sim.t(2580), false),
        ]
    );
    assert_eq!(
        sim.waiting_rows(),
        vec![
            (sim.t(1), sim.t(601), false),
            (sim.t(611), sim.t(2411), false),
            (sim.t(2481), sim.t(2580), false),
        ]
    );
    assert_eq!(sim.secs("claude.exe", Productive), 600 + 10 + 1800 + 99);
    assert_eq!(sim.waiting_secs("claude.exe", Productive), 600 + 1800 + 99);
    sim.assert_counted_once(2579 * SEC);
}

#[test]
fn a_minimized_app_exiting_hands_back_to_the_foreground() {
    let mut sim = Sim::new("wait-exit");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(youtube());
    sim.minimize("claude.exe");
    sim.work(31);
    // The agent's process exits while minimized; YouTube counts from the next tick.
    sim.unminimize("claude.exe");
    sim.work(30);
    // Started again later and minimized: a new row on a fresh clock.
    sim.minimize("claude.exe");
    sim.work(30);
    sim.shutdown();
    assert_eq!(
        sim.waiting_rows(),
        vec![(sim.t(1), sim.t(32), false), (sim.t(62), sim.t(91), false)]
    );
    assert_eq!(sim.secs("youtube.com", Entertainment), 30);
    sim.assert_counted_once(90 * SEC);
}

#[test]
fn category_flip_mid_minimize_closes_the_row_and_keeps_the_clock() {
    let mut sim = Sim::new("wait-flip");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(youtube());
    sim.minimize("claude.exe");
    sim.work(61);
    // Made Entertainment: the row closes on the next tick (ADR-0005) and YouTube counts, since an
    // Entertainment app never waits in.
    sim.track_app("claude.exe", Some(Entertainment));
    sim.work(1200);
    // Back to Productive: it waits in again, on the clock that started at t1, so until t1801.
    sim.track_app("claude.exe", Some(Productive));
    sim.work(600);
    sim.shutdown();
    assert_eq!(
        sim.waiting_rows(),
        vec![
            (sim.t(1), sim.t(62), false),
            (sim.t(1262), sim.t(1801), false)
        ]
    );
    assert_eq!(sim.secs("claude.exe", Entertainment), 0);
    assert_eq!(sim.secs("claude.exe", Productive), 61 + 539);
    assert_eq!(sim.secs("youtube.com", Entertainment), 1200 + 60);
    sim.assert_counted_once(1860 * SEC);
}

#[test]
fn crash_recovery_keeps_and_trims_waiting_rows() {
    let mut sim = Sim::new("wait-crash");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(youtube());
    sim.minimize("claude.exe");
    sim.work(14);
    sim.crash();
    // Flushed at t10 (every 5 s from the t0 tick) and closed there, still marked as waiting
    // credit.
    assert_eq!(sim.waiting_rows(), vec![(sim.t(1), sim.t(10), false)]);
    // The new tracker finds Claude already minimized, so it gets no fresh clock and YouTube
    // counts. Then 2 idle minutes and a crash: recovery trims back to the last input, as a
    // shutdown would have.
    sim.work(61);
    sim.idle(120);
    sim.crash();
    assert_eq!(
        sim.rows(),
        vec![(sim.t(1), sim.t(10), false), (sim.t(15), sim.t(75), false)]
    );
    assert_eq!(sim.waiting_rows(), vec![(sim.t(1), sim.t(10), false)]);
    assert_eq!(sim.secs("youtube.com", Entertainment), 60);
    sim.assert_counted_once(69 * SEC);
}

#[test]
fn an_app_minimized_before_tracking_starts_never_waits_in() {
    let mut sim = Sim::new("wait-at-start");
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    // Claude was minimized before the tracker started (a login restoring windows minimized), so
    // the tracker never saw it minimized by the user: YouTube counts.
    sim.minimize("claude.exe");
    sim.focus(youtube());
    sim.work(61);
    // A restart, as after a Delete all or a crash, changes nothing.
    sim.shutdown();
    sim.crash();
    sim.work(60);
    sim.shutdown();
    assert_eq!(sim.waiting_rows(), vec![]);
    assert_eq!(sim.secs("claude.exe", Productive), 0);
    assert_eq!(sim.secs("youtube.com", Entertainment), 119);
}

#[test]
fn restoring_then_minimizing_starts_the_clock() {
    let mut sim = Sim::new("wait-restore");
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.minimize("claude.exe");
    sim.focus(youtube());
    sim.work(61);
    // Restored behind YouTube, without coming to the front, then minimized again: a full clock.
    sim.unminimize("claude.exe");
    sim.work(10);
    sim.minimize("claude.exe");
    sim.work(1830);
    sim.shutdown();
    assert_eq!(sim.waiting_rows(), vec![(sim.t(72), sim.t(1872), false)]);
    assert_eq!(sim.secs("youtube.com", Entertainment), 71 + 29);
    sim.assert_counted_once(1900 * SEC);
}

#[test]
fn a_reopen_mid_credit_grants_no_fresh_30_minutes() {
    let mut sim = Sim::new("wait-reopen");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(youtube());
    sim.minimize("claude.exe");
    sim.work(601);
    // The poll thread panics 10 minutes in and reopens the tracker (crash recovery). Claude is
    // still minimized, but the new engine never saw it minimized: no fresh 30 minutes, and not
    // the 20 left either. YouTube counts until Claude is brought back and minimized again.
    sim.crash();
    sim.work(1800);
    sim.shutdown();
    // Closed at its last flush, t600.
    assert_eq!(sim.waiting_rows(), vec![(sim.t(1), sim.t(600), false)]);
    assert_eq!(sim.secs("claude.exe", Productive), 599);
    assert_eq!(sim.secs("youtube.com", Entertainment), 1799);
}

#[test]
fn waiting_credit_crossing_4am_splits_between_days() {
    let mut sim = Sim::at("wait-4am", "2026-10-22T03:58:59");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(youtube());
    sim.minimize("claude.exe");
    sim.work(121);
    sim.shutdown();
    // 03:59 to 04:01 with YouTube in front: a minute of waiting credit on each Day, in the Day
    // view and in the week's bars alike.
    let reader = sim.reader();
    for day in ["2026-10-21", "2026-10-22"] {
        let summary = reader.day(Some(day)).unwrap();
        let claude: Vec<(u32, u32)> = summary
            .items
            .iter()
            .filter(|i| i.name == "claude.exe")
            .map(|i| (i.secs, i.minimized_secs))
            .collect();
        assert_eq!(claude, [(60, 60)], "{day}");
        assert_eq!(summary.totals.productive_secs, 60, "{day}");
        assert_eq!(summary.totals.entertainment_secs, 0, "{day}");
    }
    let week = reader.week(Some("2026-10-19")).unwrap();
    let productive: Vec<u32> = week.days.iter().map(|d| d.productive_secs).collect();
    assert_eq!(productive, [0, 0, 60, 60, 0, 0, 0]);
    sim.assert_counted_once(120 * SEC);
}

// ---------------------------------------------------------------------------------------------
// Settings: pause, the Tracking choices, export, delete all (issue 13)
// ---------------------------------------------------------------------------------------------

#[test]
fn pause_closes_the_open_row_and_counts_nothing_until_it_ends() {
    let mut sim = Sim::new("pause");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    // Tray → Pause → 15 minutes, at t61, mid-interval.
    let until = sim.machine.now_ms + 15 * MIN;
    sim.pause(Some(until));
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(61), false)]);
    assert_eq!(sim.tracking().map(|n| n.name), None);
    assert_eq!(sim.reader().paused_until_ms().unwrap(), Some(until));
    // Working all through the pause counts nothing.
    sim.work(899);
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(61), false)]);
    assert_eq!(sim.tracking().map(|n| n.name), None);
    // It ends on its own at t961, and counting starts from that tick.
    sim.work(61);
    assert_eq!(sim.reader().paused_until_ms().unwrap(), None);
    assert_eq!(sim.tracking().map(|n| n.name), Some("code.exe".into()));
    sim.shutdown();
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(61), false),
            (sim.t(961), sim.t(1021), false)
        ]
    );
}

#[test]
fn resume_counts_again_from_the_next_tick() {
    let mut sim = Sim::new("resume");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.pause(Some(sim.machine.now_ms + HOUR));
    sim.work(60);
    sim.pause(None);
    sim.work(31);
    sim.shutdown();
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(61), false),
            (sim.t(122), sim.t(152), false)
        ]
    );
}

#[test]
fn a_pause_holds_across_a_restart() {
    let mut sim = Sim::new("pause-restart");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    let until = sim.machine.now_ms + HOUR;
    sim.pause(Some(until));
    // The app dies and starts again (a crash, or a sign-out and a login start).
    sim.work(600);
    sim.crash();
    sim.work(1200);
    sim.shutdown();
    sim.crash();
    assert_eq!(sim.reader().paused_until_ms().unwrap(), Some(until));
    sim.work(1799);
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(61), false)]);
    // t3661 is the end of the hour.
    sim.work(61);
    sim.shutdown();
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(61), false),
            (sim.t(3661), sim.t(3721), false)
        ]
    );
}

#[test]
fn pause_until_tomorrow_ends_at_4am() {
    let mut sim = Sim::at("pause-4am", "2026-10-21T23:00");
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    let until = next_day_start(sim.machine.now_ms, &tz()).unwrap();
    // 04:00 EDT the next morning.
    assert_eq!(until, ms("2026-10-22T08:00Z"));
    // At 2am the Day hasn't turned yet, so "tomorrow" is 4am the same date.
    assert_eq!(
        next_day_start(ms("2026-10-22T06:00Z"), &tz()).unwrap(),
        until
    );
    sim.pause(Some(until));
    // Up all night: t18000 is 04:00:00.
    sim.work(17_938);
    assert_eq!(sim.rows(), vec![(sim.t(1), sim.t(61), false)]);
    sim.work(61);
    sim.shutdown();
    assert_eq!(
        sim.rows(),
        vec![
            (sim.t(1), sim.t(61), false),
            (sim.t(18_000), sim.t(18_060), false)
        ]
    );
    let reader = sim.reader();
    let productive = |day: &str| reader.day(Some(day)).unwrap().totals.productive_secs;
    assert_eq!(productive("2026-10-21"), 60);
    assert_eq!(productive("2026-10-22"), 60);
}

#[test]
fn away_after_2_minutes() {
    let mut sim = Sim::new("away-2");
    sim.away_after(2);
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    sim.idle(115);
    // Still Active 1:55 after the last input (flushed at t176).
    assert_eq!(sim.secs("code.exe", Productive), 175);
    sim.idle(5);
    // 2 min without evidence: Away, trimmed back to the input at t61, as at 5 min.
    assert_eq!(sim.secs("code.exe", Productive), 60);
}

#[test]
fn away_after_15_minutes() {
    let mut sim = Sim::new("away-15");
    sim.away_after(15);
    sim.track_app("code.exe", Some(Productive));
    sim.focus(app("code.exe"));
    sim.work(61);
    // 10 idle minutes, past the default 5: still counting.
    sim.idle(600);
    assert_eq!(sim.secs("code.exe", Productive), 660);
    sim.idle(300);
    assert_eq!(sim.secs("code.exe", Productive), 60);
}

#[test]
fn away_after_rejects_other_lengths() {
    let mut sim = Sim::new("away-invalid");
    let (reply, rx) = mpsc::channel();
    assert!(
        sim.tracker
            .handle(Control::SetAwayAfter { minutes: 7, reply })
            .is_continue()
    );
    assert!(rx.recv().unwrap().is_err());
    assert_eq!(sim.reader().settings().unwrap().away_after_minutes, 5);
}

#[test]
fn a_new_install_starts_with_waiting_credit_off() {
    // An install onboarded before v5 keeps it on (`v1_database_migrates_to_glass_appearance`), and
    // Delete all turns it off again (`delete_all_mid_interval_starts_over`).
    let dir = TempDir::new("fresh-install");
    let _tracker = Tracker::open(&dir.0).unwrap();
    let reader = Reader::open(&dir.0, tz, || 0).unwrap();
    assert!(!reader.settings().unwrap().waiting_credit);
}

#[test]
fn waiting_credit_off_counts_the_foreground() {
    let mut sim = Sim::new("wait-off");
    sim.shown(&["claude.exe"]);
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.waiting_credit(false);
    sim.focus(youtube());
    sim.minimize("claude.exe");
    sim.work(61);
    assert_eq!(sim.tracking().map(|n| n.name), Some("youtube.com".into()));
    // Back on: the clock kept running from the minimize, so the agent waits in from the next tick.
    sim.waiting_credit(true);
    sim.work(60);
    sim.shutdown();
    assert_eq!(sim.secs("youtube.com", Entertainment), 61);
    assert_eq!(sim.secs("claude.exe", Productive), 59);
    assert_eq!(sim.waiting_rows(), vec![(sim.t(62), sim.t(121), false)]);
    sim.assert_counted_once(120 * SEC);
}

#[test]
fn delete_all_mid_interval_starts_over() {
    let mut sim = Sim::new("delete-all");
    let icons = sim.dir.0.join("icons");
    fs::create_dir_all(&icons).unwrap();
    fs::write(icons.join("0123456789abcdef.png"), b"png").unwrap();
    sim.track_app("code.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.control(|reply| Control::SetDailyGoal {
        minutes: 120,
        reply,
    });
    sim.control(|reply| Control::CompleteOnboarding { reply });
    sim.away_after(15);
    sim.pause(Some(sim.machine.now_ms + HOUR));
    sim.pause(None);
    sim.focus(app("code.exe"));
    // Mid-interval, with a flush (and its pending trim) behind it.
    sim.work(30);
    sim.control(|reply| Control::DeleteAllData { reply });
    assert_eq!(sim.rows(), vec![]);
    assert_eq!(sim.tracking().map(|n| n.name), None);
    let reader = sim.reader();
    let settings = reader.settings().unwrap();
    assert_eq!(
        (
            settings.onboarded,
            settings.daily_goal_minutes,
            settings.away_after_minutes,
            settings.waiting_credit,
            settings.appearance
        ),
        (false, 360, 5, false, Appearance::Glass)
    );
    assert_eq!(reader.paused_until_ms().unwrap(), None);
    assert!(reader.sites().unwrap().is_empty());
    assert!(reader.tracked_apps().unwrap().is_empty());
    assert!(reader.day(None).unwrap().items.is_empty());
    // The icons go; their folder stays for the catalog and the asset scope.
    assert_eq!(fs::read_dir(&icons).unwrap().count(), 0);

    // The tracker kept running: nothing counts until something is tracked again, then a new row
    // starts, and a restart finds nothing left over to recover or trim.
    sim.work(30);
    assert_eq!(sim.rows(), vec![]);
    sim.track_app("code.exe", Some(Productive));
    sim.work(11);
    sim.shutdown();
    sim.crash();
    assert_eq!(sim.rows(), vec![(sim.t(61), sim.t(71), false)]);
}

#[test]
fn csv_export_has_the_day_views_numbers() {
    let mut sim = Sim::at("export-csv", "2026-10-22T03:58:59");
    sim.shown(&["claude.exe"]);
    sim.track_app("code.exe", Some(Productive));
    sim.track_app("claude.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(app("code.exe"));
    sim.work(121);
    sim.focus(youtube());
    sim.minimize("claude.exe");
    sim.work(30);
    sim.shutdown();
    let csv = sim.reader().export(ExportFormat::Csv).unwrap();
    // VS Code from 03:59 to 04:01:01 splits at the Day boundary, as in the Day view; then Claude
    // waits in for YouTube until 04:01:30.
    assert_eq!(
        csv,
        "day,name,kind,category,seconds,waiting_seconds\r\n\
         2026-10-21,code.exe,app,productive,60,0\r\n\
         2026-10-22,code.exe,app,productive,61,0\r\n\
         2026-10-22,claude.exe,app,productive,29,29\r\n"
    );
    let empty = Sim::new("export-empty");
    assert_eq!(
        empty.reader().export(ExportFormat::Csv).unwrap(),
        "day,name,kind,category,seconds,waiting_seconds\r\n"
    );
}

#[test]
fn json_export_has_every_row_as_stored() {
    let mut sim = Sim::new("export-json");
    sim.track_app("code.exe", Some(Productive));
    sim.track_site("youtube.com", None, Entertainment);
    sim.focus(app("code.exe"));
    sim.work(61);
    // Untracked later: still exported, with no category, because its interval names it.
    sim.track_app("code.exe", None);
    sim.focus(youtube());
    sim.work(10);
    sim.shutdown();
    let json: serde_json::Value =
        serde_json::from_str(&sim.reader().export(ExportFormat::Json).unwrap()).unwrap();
    assert_eq!(json["timeZone"], "America/New_York");
    assert_eq!(json["dayStartsAt"], "04:00");
    assert_eq!(
        json["settings"],
        serde_json::json!({
            "dailyGoalMinutes": 360,
            "awayAfterMinutes": 5,
            "waitingCredit": true,
            "appearance": "glass"
        })
    );
    assert_eq!(
        json["apps"],
        serde_json::json!([{
            "id": 1,
            "identity": { "kind": "exe", "key": "code.exe" },
            "name": "code.exe",
            "category": null
        }])
    );
    assert_eq!(json["sites"][0]["domain"], "youtube.com");
    assert_eq!(
        json["intervals"],
        serde_json::json!([
            {
                "start": "2026-10-21T10:00:01-04:00",
                "end": "2026-10-21T10:01:02-04:00",
                "target": { "kind": "app", "id": 1 },
                "category": "productive",
                "minimized": false
            },
            {
                "start": "2026-10-21T10:01:02-04:00",
                "end": "2026-10-21T10:01:11-04:00",
                "target": { "kind": "site", "id": 1 },
                "category": "entertainment",
                "minimized": false
            }
        ])
    );
}

// ---------------------------------------------------------------------------------------------
// A realistic day
// ---------------------------------------------------------------------------------------------

/// A day of use on an uneven clock: ticks land 0.75 to 1.25 s apart, every 47th tick is slow
/// (2.6 s, under the gap threshold) and the next one catches up, and keystrokes land between ticks.
/// The script says what the user does; `Ledger` writes down, from the same script and GLOSSARY's
/// rules alone, what should count, one span at a time like the rows (ADR-0007).
struct Life {
    sim: Sim,
    /// Wall time at true time 0.
    origin_ms: i64,
    /// True time since the start. Also the idle clock: like `GetTickCount`, it runs through sleep.
    real_ms: i64,
    /// True time spent asleep, which this monotonic clock doesn't count, so a resume shows both as
    /// a wall step and as wall and monotonic disagreeing.
    slept_ms: i64,
    /// Wall minus true time, moved by clock changes.
    offset_ms: i64,
    last_tick_ms: i64,
    next_tick_ms: i64,
    ticks: u64,
    rng: u64,
    foreground: Option<Foreground>,
    minimized: Vec<AppIdentity>,
    audible: bool,
    locked: bool,
    unlocked: bool,
    last_input_ms: i64,
    ledger: Ledger,
}

impl Life {
    fn new(name: &str, local: &str) -> Self {
        let sim = Sim::at(name, local);
        let origin_ms = sim.start_ms;
        Self {
            sim,
            origin_ms,
            real_ms: 0,
            slept_ms: 0,
            offset_ms: 0,
            last_tick_ms: 0,
            next_tick_ms: SEC,
            ticks: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
            foreground: None,
            minimized: Vec::new(),
            audible: false,
            locked: false,
            unlocked: false,
            last_input_ms: -10 * HOUR,
            ledger: Ledger::default(),
        }
    }

    /// xorshift64, so a failure replays exactly.
    fn rand(&mut self, below: i64) -> i64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        i64::try_from(self.rng % u64::try_from(below).unwrap()).unwrap()
    }

    fn wall(&self, real_ms: i64) -> i64 {
        self.origin_ms + real_ms + self.offset_ms
    }

    /// The wall time of the next tick, which is where a change made now is first seen.
    fn next_wall(&self) -> i64 {
        self.wall(self.next_tick_ms)
    }

    fn tick(&mut self) {
        self.real_ms = self.next_tick_ms;
        self.last_tick_ms = self.real_ms;
        let now_ms = self.wall(self.real_ms);
        let sample = Sample {
            now_ms,
            mono_ms: u64::try_from(self.real_ms - self.slept_ms).unwrap(),
            away_signal: self.locked,
            idle_ms: u32::try_from(self.real_ms - self.last_input_ms).unwrap(),
            input_event: std::mem::take(&mut self.unlocked),
            foreground: self.foreground.clone().map(|f| Foreground {
                audible: self.audible,
                ..f
            }),
            minimized: self.minimized.clone(),
        };
        self.sim.tracker.tick(&sample).unwrap();
        self.sim.machine.now_ms = now_ms;
        self.ticks += 1;
        self.next_tick_ms += match self.ticks % 47 {
            0 => 2_600,
            1 => 300,
            _ => 750 + self.rand(500),
        };
    }

    /// The ticks of the next `dur_ms`. While typing, a keystroke lands before 3 ticks in 4.
    fn run(&mut self, dur_ms: i64, typing: bool) {
        let end = self.real_ms + dur_ms;
        while self.next_tick_ms <= end {
            if typing && self.rand(4) != 0 {
                let gap = self.next_tick_ms - self.real_ms;
                self.last_input_ms = self.real_ms + 1 + self.rand(gap);
            }
            self.tick();
        }
        self.real_ms = end;
    }

    fn run_until_wall(&mut self, wall_ms: i64, typing: bool) {
        self.run(wall_ms - self.wall(self.real_ms), typing);
    }

    /// A tick now, then `n` more 0.96 s apart, so the last three ticks span under 2 s.
    fn tight_ticks(&mut self, n: u32) {
        self.next_tick_ms = self.real_ms.max(self.last_tick_ms + 1);
        self.tick();
        for _ in 0..n {
            self.next_tick_ms = self.real_ms + 960;
            self.tick();
        }
    }

    /// A keystroke or click right now.
    fn press(&mut self) {
        self.last_input_ms = self.real_ms;
    }

    /// Clicks into `foreground`; `counts` is what GLOSSARY says it counts toward, if anything.
    fn switch(&mut self, foreground: Foreground, counts: Option<(&'static str, Category)>) {
        self.press();
        let at = self.next_wall();
        self.ledger.end(at);
        if let Some((name, category)) = counts {
            self.ledger.begin(name, category, at);
        }
        self.foreground = Some(foreground);
    }

    /// Minimizes a window of `exe` (a click). Returns the next tick's wall time, where its
    /// 30-minute clock starts.
    fn minimize(&mut self, exe: &str) -> i64 {
        self.press();
        self.minimized.push(exe_id(exe));
        self.next_wall()
    }

    /// Restores `exe`'s minimized window.
    fn restore(&mut self, exe: &str) {
        self.minimized.retain(|id| *id != exe_id(exe));
    }

    /// No more input: if the user doesn't come back within 5 min, time counts to this keystroke,
    /// waiting credit included.
    fn walk_away(&mut self) {
        let at = self.wall(self.last_input_ms);
        self.ledger.end(at);
    }

    /// Sleeps for `dur_ms`; the first tick comes 0.2 s after the resume.
    fn sleep(&mut self, dur_ms: i64) {
        self.real_ms += dur_ms;
        self.slept_ms += dur_ms;
        self.next_tick_ms = self.real_ms + 200;
    }
}

/// What should count: `(name, category, waiting, from, to)` spans in wall ms. One span is open at
/// a time, as one row is.
#[derive(Default)]
struct Ledger {
    open: Option<(&'static str, Category, bool, i64)>,
    spans: Vec<(&'static str, Category, bool, i64, i64)>,
}

impl Ledger {
    fn begin(&mut self, name: &'static str, category: Category, at: i64) {
        self.open_span(name, category, false, at);
    }

    /// A minimized app waiting in (ADR-0007): Productive, and the "incl. … waiting" part.
    fn begin_waiting(&mut self, name: &'static str, at: i64) {
        self.open_span(name, Productive, true, at);
    }

    fn open_span(&mut self, name: &'static str, category: Category, waiting: bool, at: i64) {
        assert!(self.open.is_none(), "{name} begins inside another span");
        self.open = Some((name, category, waiting, at));
    }

    fn end(&mut self, at: i64) {
        if let Some((name, category, waiting, from)) = self.open.take() {
            assert!(at >= from, "{name} would end before it began");
            self.spans.push((name, category, waiting, from, at));
        }
    }

    /// Whole seconds per "Category name" inside `[start, end)`, each item floored once, as the
    /// Day view floors it; items under a second are left out, as the Day view leaves them out.
    /// `waiting_only` counts the waiting spans alone, as `minimized_secs` does.
    fn day(&self, (start, end): (i64, i64), waiting_only: bool) -> BTreeMap<String, u32> {
        let mut ms: BTreeMap<String, i64> = BTreeMap::new();
        for &(name, category, waiting, from, to) in &self.spans {
            let overlap = to.min(end) - from.max(start);
            if overlap > 0 && (waiting || !waiting_only) {
                *ms.entry(format!("{category:?} {name}")).or_default() += overlap;
            }
        }
        ms.into_iter()
            .map(|(item, ms)| (item, u32::try_from(ms / SEC).unwrap()))
            .filter(|&(_, secs)| secs > 0)
            .collect()
    }

    fn total_ms(&self) -> i64 {
        self.spans
            .iter()
            .map(|&(_, _, _, from, to)| to - from)
            .sum()
    }
}

/// About 10 h on the night DST ends in New York: jittery and stalled ticks, a walk-away, a video
/// watched without input, a Win+L lock, a sleep, the clock set back an hour, a movie past the 2 h
/// sound cap, alt-tab churn, an untracked app, a private window, an agent left minimized three
/// times and the 4am Day boundary. Every Day item must match the ledger to the second.
#[test]
fn realistic_day_replay() {
    // 20:00 EDT on Saturday 2026-10-31; clocks fall back at 02:00, so this Day lasts 25 h.
    let mut life = Life::new("day", "2026-10-31T20:00");
    for (exe, category) in [
        ("code.exe", Productive),
        ("winword.exe", Productive),
        ("claude.exe", Productive),
        ("discord.exe", Entertainment),
        ("vlc.exe", Entertainment),
    ] {
        life.sim.track_app(exe, Some(category));
    }
    // Chrome itself isn't tracked; YouTube is.
    life.sim.track_site("youtube.com", None, Entertainment);

    // An evening in VS Code across a 1 h roll, with a 4 min read (under 5 min, so it counts).
    life.switch(app("code.exe"), Some(("code.exe", Productive)));
    life.run(90 * MIN, true);
    life.run(4 * MIN, false);
    life.press();
    life.run(6 * MIN, true);
    // A walk away: Away after 5 min, trimmed back to the last keystroke.
    life.walk_away();
    life.run(12 * MIN, false);

    // An agent gets a task and is minimized as YouTube comes to the front. The agent waits in for
    // its 30 minutes, which ends at an instant between two ticks, then YouTube counts. The video
    // plays on with no input, and the sound is presence for both; it counts up to the last tick
    // that heard it.
    life.switch(app("claude.exe"), Some(("claude.exe", Productive)));
    life.run(2 * MIN, true);
    let agent_from = life.minimize("claude.exe");
    life.switch(youtube(), None);
    let agent_until = agent_from + 30 * MIN;
    life.ledger.begin_waiting("claude.exe", agent_from);
    life.ledger.end(agent_until);
    life.ledger.begin("youtube.com", Entertainment, agent_until);
    life.run(MIN, true);
    life.audible = true;
    life.run(37 * MIN, false);
    life.audible = false;
    let last_heard = life.wall(life.last_tick_ms);
    assert!(agent_until < last_heard, "YouTube should outlast the agent");
    life.ledger.end(last_heard);
    life.run(10 * MIN, false);

    // Word. Then the agent again: brought to the front, which resets its spent clock, given a task
    // and minimized while the user reads in Notepad (untracked), so it waits in. 2 idle minutes
    // and Win+L trim it like any row. Unlocking counts as input; the agent waits in for what's left
    // of its clock, and nothing counts after that until Word is back in front.
    life.switch(app("winword.exe"), Some(("winword.exe", Productive)));
    life.run(12 * MIN, true);
    life.restore("claude.exe");
    life.switch(app("claude.exe"), Some(("claude.exe", Productive)));
    life.run(MIN, true);
    let agent_from = life.minimize("claude.exe");
    life.switch(app("notepad.exe"), None);
    life.ledger.begin_waiting("claude.exe", agent_from);
    life.run(6 * MIN, true);
    life.walk_away();
    life.run(2 * MIN, false);
    life.press();
    life.locked = true;
    life.run(20 * MIN, false);
    life.locked = false;
    life.unlocked = true;
    let unlocked = life.next_wall();
    let agent_until = agent_from + 30 * MIN;
    assert!(
        unlocked < agent_until,
        "the agent's credit should outlast the lock"
    );
    life.ledger.begin_waiting("claude.exe", unlocked);
    life.ledger.end(agent_until);
    life.run(5 * MIN, true);
    life.switch(app("winword.exe"), Some(("winword.exe", Productive)));
    life.run(40 * MIN, true);

    // A pause, then the lid closes; the last three ticks before it span under 2 s. A keypress
    // wakes it 40 min later and starts a new stretch.
    life.walk_away();
    life.run(8 * SEC, false);
    life.tight_ticks(2);
    life.sleep(40 * MIN);
    life.press();
    let resumed = life.next_wall();
    life.ledger.begin("winword.exe", Productive, resumed);
    life.run(50 * MIN, true);

    // The clock is set back an hour mid-session. The repeated wall hour counts again, because it
    // was really spent; the jump itself is Away, back to the last keystroke before it.
    life.walk_away();
    life.run(5 * SEC, false);
    life.offset_ms -= HOUR;
    life.press();
    let after_jump = life.next_wall();
    life.ledger.begin("winword.exe", Productive, after_jump);
    life.run(30 * MIN, true);

    // A movie: 5 min of input, then 2 h 25 min of sound alone. The 2 h cap trims back to the
    // last input, so the unattended part counts as nothing.
    life.switch(app("vlc.exe"), Some(("vlc.exe", Entertainment)));
    life.run(5 * MIN, true);
    life.walk_away();
    life.audible = true;
    life.run(145 * MIN, false);
    life.audible = false;

    // Discord, then alt-tab churn with VS Code.
    life.switch(app("discord.exe"), Some(("discord.exe", Entertainment)));
    life.run(28 * MIN, true);
    for i in 0..24 {
        let (exe, category) = if i % 2 == 0 {
            ("code.exe", Productive)
        } else {
            ("discord.exe", Entertainment)
        };
        life.switch(app(exe), Some((exe, category)));
        let visit = 1_500 + life.rand(2_500);
        life.run(visit, true);
    }

    // VS Code, with a few minutes in an untracked app and in a private window, neither counting
    // (the agent is still minimized, but its clock is spent).
    life.switch(app("code.exe"), Some(("code.exe", Productive)));
    life.run(30 * MIN, true);
    life.switch(app("notepad.exe"), None);
    life.run(5 * MIN, true);
    life.switch(app("code.exe"), Some(("code.exe", Productive)));
    life.run(5 * MIN, true);
    life.switch(tab("chrome.exe", Some("youtube.com"), "", true), None);
    life.run(5 * MIN, true);
    // Back to work. The agent comes to the front for 2 min, which resets its clock, and is
    // minimized while the user chats on Discord: it waits in for a fresh 30 min that crosses 4am
    // (EST by now), then Discord counts. Then quit from the tray mid-typing.
    life.switch(app("code.exe"), Some(("code.exe", Productive)));
    life.run(5 * MIN, true);
    life.restore("claude.exe");
    life.switch(app("claude.exe"), Some(("claude.exe", Productive)));
    life.run(2 * MIN, true);
    let agent_from = life.minimize("claude.exe");
    life.switch(app("discord.exe"), None);
    let agent_until = agent_from + 30 * MIN;
    let four_am = ms("2026-11-01T09:00Z");
    let quit = ms("2026-11-01T09:30Z");
    assert!(agent_from < four_am && four_am < agent_until && agent_until < quit);
    life.ledger.begin_waiting("claude.exe", agent_from);
    life.ledger.end(agent_until);
    life.ledger.begin("discord.exe", Entertainment, agent_until);
    life.run_until_wall(quit, true);
    life.sim.shutdown();
    life.walk_away();

    // The Days, local 04:00 to 04:00, written out here rather than taken from days.rs.
    let days = [
        (
            "2026-10-31",
            (ms("2026-10-31T08:00Z"), ms("2026-11-01T09:00Z")),
        ),
        (
            "2026-11-01",
            (ms("2026-11-01T09:00Z"), ms("2026-11-02T09:00Z")),
        ),
    ];
    let reader = life.sim.reader();
    let week = reader.week(Some("2026-10-26")).unwrap();
    for (day, bounds) in days {
        let expected = life.ledger.day(bounds, false);
        let summary = reader.day(Some(day)).unwrap();
        let shown = |secs: fn(&DayItem) -> u32| -> BTreeMap<String, u32> {
            summary
                .items
                .iter()
                .filter(|i| secs(i) > 0)
                .map(|i| (format!("{:?} {}", i.category, i.name), secs(i)))
                .collect()
        };
        eprintln!("{day}: {expected:?}");
        assert_eq!(shown(|i| i.secs), expected, "{day}");
        // The waiting part of each item, from the waiting spans alone.
        let expected_waiting = life.ledger.day(bounds, true);
        eprintln!("{day} waiting: {expected_waiting:?}");
        assert_eq!(shown(|i| i.minimized_secs), expected_waiting, "{day}");
        // The week's bar for the Day shows the Day view's own totals.
        let bar = week.days.iter().find(|d| d.day == day).unwrap();
        assert_eq!(
            (bar.productive_secs, bar.entertainment_secs),
            (
                summary.totals.productive_secs,
                summary.totals.entertainment_secs
            ),
            "{day}"
        );
    }
    // Nothing is left open, and the stored time is the ledger's to the millisecond.
    let rows = life.sim.rows();
    assert!(rows.iter().all(|&(start, end, open)| !open && end > start));
    let stored: i64 = rows.iter().map(|&(start, end, _)| end - start).sum();
    assert_eq!(stored, life.ledger.total_ms());
    // Every second counted once: each row starts where or after the one before it ended. The one
    // exception is the clock set back an hour, whose repeated wall hour was really spent.
    let overlaps: Vec<i64> = rows
        .windows(2)
        .map(|pair| pair[0].1 - pair[1].0)
        .filter(|&overlap| overlap > 0)
        .collect();
    assert!(
        overlaps.len() == 1 && overlaps[0] < HOUR,
        "overlaps: {overlaps:?}"
    );
    assert!(life.ticks > 30_000, "{} ticks", life.ticks);
}
