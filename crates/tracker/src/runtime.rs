//! The poll thread and the handle the shell holds (plan section 4 "Runtime").

use std::fs;
use std::ops::ControlFlow;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use tracing::{error, warn};
use tracker_api::{
    ApiError, Control, ICONS_DIR_NAME, NowTracking, Sample, Sampler, TrackerStatus, is_browser,
};

use crate::engine::{Counted, Engine, Op, Presence};
use crate::store::{Reader, StoreError, Writer, existing_icon};

const TICK: Duration = Duration::from_secs(1);
/// Open rows' ends are written this often (on the sample's monotonic clock), bounding what a
/// crash can lose.
const FLUSH_EVERY_MS: u64 = 5_000;

/// The single writer: runs the engine on each sample and applies its ops and every config change
/// to the store. The poll thread owns one; scenarios drive one directly with a fake clock.
pub struct Tracker {
    engine: Engine,
    writer: Writer,
    last_flush_mono_ms: Option<u64>,
    /// The last tick's writes failed. Until one succeeds, every tick flushes, so saving resumes
    /// (and the status clears) on the first tick the store works again.
    save_failed: bool,
    /// For `status`'s icon paths.
    data_dir: PathBuf,
}

impl Tracker {
    /// Opens (creating and migrating) `<data_dir>/tracker.db` and runs crash recovery.
    pub fn open(data_dir: &Path) -> Result<Self, StoreError> {
        let writer = Writer::open(data_dir)?;
        Ok(Self {
            // A stored pause holds across a restart: the new engine starts paused.
            engine: Engine::new(writer.targets()?, writer.rules()?),
            writer,
            last_flush_mono_ms: None,
            save_failed: false,
            data_dir: data_dir.to_owned(),
        })
    }

    /// One tick: a failed EndStretch's retry, the engine, its ops, then a flush if one is due.
    /// Any store error is lost sync: the ops stop at the failure and the engine forgets its open
    /// row, so it and the writer agree again from the next tick (`Engine::forget_open_row`).
    pub fn tick(&mut self, sample: &Sample) -> Result<(), StoreError> {
        let ops = self.engine.tick(sample);
        let applied = self
            .writer
            .finish_stretch()
            .and_then(|()| self.apply_all(ops));
        if applied.is_err() {
            self.engine.forget_open_row();
        }
        let last = *self.last_flush_mono_ms.get_or_insert(sample.mono_ms);
        let result = if self.save_failed || sample.mono_ms.saturating_sub(last) >= FLUSH_EVERY_MS {
            self.last_flush_mono_ms = Some(sample.mono_ms);
            // Even after a failed op: the writer's open row (one whose Close failed, say) keeps
            // its end on disk.
            applied.and(self.writer.flush(self.engine.stop_cutoff()))
        } else {
            applied
        };
        self.save_failed = result.is_err();
        result
    }

    /// Applies one Control message and replies on its channel. `Break` after `Shutdown`.
    pub fn handle(&mut self, control: Control) -> ControlFlow<()> {
        // A send fails only if the caller stopped waiting, which is theirs to handle.
        match control {
            Control::SetAppCategory {
                identity,
                name,
                category,
                reply,
            } => {
                let result = self
                    .writer
                    .set_app_category(&identity, &name, category)
                    .and_then(|()| self.reload_targets());
                let _ = reply.send(result.map_err(ApiError::from));
            }
            Control::SetAppWatch {
                identity,
                watch,
                reply,
            } => {
                let result = self
                    .writer
                    .set_app_watch(&identity, watch)
                    .and_then(|()| self.reload_targets());
                let _ = reply.send(result.map_err(ApiError::from));
            }
            Control::UpsertSite {
                domain,
                keyword,
                category,
                reply,
            } => {
                let result = self
                    .writer
                    .upsert_site(&domain, keyword.as_deref(), category)
                    .and_then(|site| self.reload_targets().map(|()| site));
                let _ = reply.send(result.map_err(ApiError::from));
            }
            Control::SetDailyGoal { minutes, reply } => {
                let _ = reply.send(self.writer.set_daily_goal(minutes).map_err(ApiError::from));
            }
            Control::SetAppearance { appearance, reply } => {
                let _ = reply.send(
                    self.writer
                        .set_appearance(appearance)
                        .map_err(ApiError::from),
                );
            }
            Control::CompleteOnboarding { reply } => {
                let _ = reply.send(self.writer.complete_onboarding().map_err(ApiError::from));
            }
            Control::SetAwayAfter { minutes, reply } => {
                let result = self
                    .writer
                    .set_away_after(minutes)
                    .and_then(|()| self.reload_rules());
                let _ = reply.send(result.map_err(ApiError::from));
            }
            Control::SetWaitingCredit { enabled, reply } => {
                let result = self
                    .writer
                    .set_waiting_credit(enabled)
                    .and_then(|()| self.reload_rules());
                let _ = reply.send(result.map_err(ApiError::from));
            }
            Control::SetPause { until_ms, reply } => {
                let _ = reply.send(self.set_pause(until_ms).map_err(ApiError::from));
            }
            Control::DeleteAllData { reply } => {
                let _ = reply.send(self.delete_all().map_err(ApiError::from));
            }
            Control::Shutdown { reply } => {
                if let Err(e) = self.shutdown() {
                    warn!("shutdown write failed: {e}");
                }
                let _ = reply.send(());
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }

    /// What the tracker is doing as of the last tick (`get_status`), in `TrackerStatus`'s order.
    /// While an app waits in (ADR-0007) it is the one tracked, flagged `waiting`; a watched app
    /// held only by its watch (ADR-0008) is flagged `watching`.
    pub fn status(&self) -> TrackerStatus {
        if self.save_failed {
            return TrackerStatus::SaveFailed;
        }
        match self.engine.presence() {
            Presence::Paused => TrackerStatus::Paused,
            Presence::Away => TrackerStatus::Away {
                removed_secs: u32::try_from(self.writer.removed_ms / 1000).unwrap_or(u32::MAX),
            },
            Presence::Active => self
                .tracking()
                .map_or(TrackerStatus::Untracked, TrackerStatus::Tracking),
        }
    }

    /// What the open row counts toward, if anything.
    fn tracking(&self) -> Option<NowTracking> {
        let (counted, key) = self.engine.counting()?;
        Some(match counted {
            Counted::App { identity, app } => NowTracking {
                name: app.name.clone(),
                icon_path: existing_icon(&self.data_dir, identity),
                category: key.category,
                is_browser: is_browser(identity),
                waiting: key.minimized,
                watching: self.engine.watching(),
            },
            Counted::Site(site) => NowTracking {
                name: site.domain.clone(),
                icon_path: None,
                category: key.category,
                is_browser: false,
                waiting: false,
                watching: false,
            },
        })
    }

    /// Ends the stretch with the normal cutoff (trimming trailing idle time), then flushes.
    pub fn shutdown(&mut self) -> Result<(), StoreError> {
        let ops = self.engine.shutdown();
        self.writer
            .finish_stretch()
            .and_then(|()| self.apply_all(ops))
            // The engine's stretch has ended, so there is no cutoff left to store.
            .and(self.writer.flush(None))
    }

    /// Pausing ends the stretch where a shutdown would (the last tick, trimmed back to the last
    /// evidence), so the open row closes at the pause; the engine then counts nothing until
    /// `until_ms`. `None` resumes from the next tick.
    fn set_pause(&mut self, until_ms: Option<i64>) -> Result<(), StoreError> {
        self.writer.set_paused_until(until_ms)?;
        self.reload_rules()?;
        if until_ms.is_some() {
            self.shutdown()?;
            // A pause isn't Away: its trim doesn't explain a later away period.
            self.writer.removed_ms = 0;
        }
        Ok(())
    }

    /// Back to first run with tracking still running: the wipe takes the open row with it, and a
    /// fresh engine (no stretch, no clocks, nothing tracked, default rules) carries on from the
    /// next tick. The extracted icons go too (the catalog extracts them again as needed); their
    /// folder stays, since the catalog writes into it and the shell's asset scope names it.
    fn delete_all(&mut self) -> Result<(), StoreError> {
        self.writer.delete_all()?;
        self.engine = Engine::new(self.writer.targets()?, self.writer.rules()?);
        self.last_flush_mono_ms = None;
        let icons = self.data_dir.join(ICONS_DIR_NAME);
        let removed = fs::read_dir(&icons)
            .and_then(|mut entries| entries.try_for_each(|entry| fs::remove_file(entry?.path())));
        // The data is gone either way; a leftover icon only shows an app's picture.
        if let Err(e) = removed {
            warn!("couldn't delete every icon in {}: {e}", icons.display());
        }
        Ok(())
    }

    /// Applies the ops in order, stopping at the first error: the ops after it assume it landed.
    fn apply_all(&mut self, ops: Vec<Op>) -> Result<(), StoreError> {
        ops.into_iter().try_for_each(|op| self.writer.apply(op))
    }

    fn reload_targets(&mut self) -> Result<(), StoreError> {
        self.engine.set_targets(self.writer.targets()?);
        Ok(())
    }

    fn reload_rules(&mut self) -> Result<(), StoreError> {
        self.engine.set_rules(self.writer.rules()?);
        Ok(())
    }
}

/// Cheap to clone and shareable across threads; the shell keeps one in its state.
#[derive(Debug, Clone)]
pub struct RuntimeHandle {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    data_dir: PathBuf,
    tx: Sender<Control>,
    /// `Tracker::status` as of the last tick, published by the poll thread so `get_status` reads
    /// it without a round trip or a query. `None` before the first tick and once stopped.
    status: Arc<Mutex<Option<TrackerStatus>>>,
    /// Taken by the first `shutdown_and_wait`, which makes later calls no-ops.
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl RuntimeHandle {
    /// Opens/migrates the DB and runs crash recovery before returning, then starts the poll
    /// thread. `make_sampler` runs on that thread because COM objects are `!Send`.
    pub fn start(
        data_dir: PathBuf,
        make_sampler: impl FnOnce() -> Box<dyn Sampler> + Send + 'static,
    ) -> Result<RuntimeHandle, StoreError> {
        let tracker = Tracker::open(&data_dir)?;
        let (tx, rx) = mpsc::channel();
        let poll_dir = data_dir.clone();
        let status = Arc::new(Mutex::new(None));
        let poll_status = Arc::clone(&status);
        let thread = thread::Builder::new()
            .name("tracker-poll".into())
            .spawn(move || {
                poll(tracker, &poll_dir, &rx, &poll_status, make_sampler);
                // Stopped: nothing counts any more.
                publish(&poll_status, None);
            })?;
        Ok(RuntimeHandle {
            inner: Arc::new(Inner {
                data_dir,
                tx,
                status,
                thread: Mutex::new(Some(thread)),
            }),
        })
    }

    /// Queues a config change; the reply arrives on the message's own `Sender`. Fails once the
    /// runtime has shut down.
    pub fn send(&self, c: Control) -> Result<(), ApiError> {
        self.inner.tx.send(c).map_err(|_| ApiError {
            message: "The tracker isn't running.".into(),
        })
    }

    /// What the tracker was doing on its last tick; see `Tracker::status`.
    pub fn status(&self) -> Option<TrackerStatus> {
        self.inner
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Ends the current stretch, flushes and joins the poll thread, waiting at most `timeout` for
    /// the flush. Idempotent; a concurrent second call waits for the first.
    pub fn shutdown_and_wait(&self, timeout: Duration) {
        let mut thread = self
            .inner
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(handle) = thread.take() else {
            return;
        };
        let (reply, done) = mpsc::channel();
        if self.inner.tx.send(Control::Shutdown { reply }).is_err() {
            warn!("tracker poll thread was already gone");
            return;
        }
        match done.recv_timeout(timeout) {
            // The thread returns right after replying, so this join is immediate.
            Ok(()) => {
                if handle.join().is_err() {
                    warn!("tracker poll thread panicked");
                }
            }
            // Leave it detached rather than block the caller (e.g. Windows ending the session).
            Err(_) => warn!("tracker didn't shut down within {timeout:?}"),
        }
    }

    /// A new `query_only` connection for the section 3 queries, on the system clock and time zone.
    /// The zone is looked up per query (jiff caches it for 5 minutes), so a change applies.
    pub fn reader(&self) -> Result<Reader, StoreError> {
        Reader::open(&self.inner.data_dir, TimeZone::system, || {
            Timestamp::now().as_millisecond()
        })
    }
}

fn poll(
    mut tracker: Tracker,
    data_dir: &Path,
    rx: &Receiver<Control>,
    status: &Mutex<Option<TrackerStatus>>,
    make_sampler: impl FnOnce() -> Box<dyn Sampler>,
) {
    let mut sampler = make_sampler();
    // Deadline-based, so Control messages arriving between ticks don't shift the tick schedule.
    let mut deadline = Instant::now() + TICK;
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(control) => {
                let flow = guarded(&mut tracker, data_dir, |t| t.handle(control));
                if flow.is_some_and(|flow| flow.is_break()) {
                    return;
                }
                // A pause or a wipe stops the counting now, not at the next tick.
                publish(status, Some(tracker.status()));
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(Err(e)) = guarded(&mut tracker, data_dir, |t| t.tick(&sampler.sample()))
                {
                    warn!("tick failed: {e}");
                }
                publish(status, Some(tracker.status()));
                deadline += TICK;
                // After a stall (e.g. a suspend) restart the schedule instead of catching up.
                let now = Instant::now();
                if now.saturating_duration_since(deadline) > TICK {
                    deadline = now + TICK;
                }
            }
            // Every handle was dropped without a shutdown: still trim and flush.
            Err(RecvTimeoutError::Disconnected) => {
                if let Err(e) = tracker.shutdown() {
                    warn!("shutdown write failed: {e}");
                }
                return;
            }
        }
    }
}

fn publish(slot: &Mutex<Option<TrackerStatus>>, value: Option<TrackerStatus>) {
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = value;
}

/// Runs one tick or Control message, catching a panic so one bad tick can't silently end
/// tracking for the rest of the session. After a panic the tracker is reopened from the DB: the
/// engine and writer may have been left disagreeing mid-tick, and reopening runs crash recovery
/// (open rows close at their last flush, losing at most 5 s), which is a known-good state.
fn guarded<T>(
    tracker: &mut Tracker,
    data_dir: &Path,
    work: impl FnOnce(&mut Tracker) -> T,
) -> Option<T> {
    let panic = match panic::catch_unwind(AssertUnwindSafe(|| work(tracker))) {
        Ok(result) => return Some(result),
        Err(panic) => panic,
    };
    let message = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic");
    error!("tracker poll thread panicked: {message}");
    match Tracker::open(data_dir) {
        Ok(fresh) => *tracker = fresh,
        Err(e) => error!("couldn't reopen the tracker after a panic, keeping the old one: {e}"),
    }
    None
}
