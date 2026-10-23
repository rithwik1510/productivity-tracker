//! The read side behind the query commands, on its own `query_only` connection.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use jiff::ToSpan;
use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use rusqlite::{Connection, Row, params};
use tracker_api::{
    CatalogApp, Category, DayItem, DaySummary, DayTotals, Settings, Share, ShareRange, Site,
    TargetRef, WeekSummary, YearHeatmap, is_browser,
};

use super::{
    SqlAppearance, SqlCategory, StoreError, connect_reader, existing_icon, identity_from_columns,
};
use crate::days::{boundaries, day_bounds, day_of, week_start};

/// Every `DayItem` query selects these columns, in this order, then its total and waiting ms, so
/// one row mapper serves all.
const ITEM_COLUMNS: &str = "i.app_id, i.site_id, i.category, a.kind, a.key, a.name, s.domain";

/// Rows overlapping `[?1, ?2)`, clipped to it. The engine's roll keeps same-target rows under 1 h,
/// but a target change on a late tick (up to the 3 s gap threshold) closes the old row at that
/// tick, so rows reach at most 1 h + 2 s. A lookback of 1 h + 3 s is therefore exact. Waiting
/// rows end within 30 min of their clock's start, so they are well inside it.
const OVERLAPS: &str = "i.start_ms >= ?1 - 3603000 AND i.start_ms < ?2 AND i.end_ms > ?1";

/// Wide enough for any stored timestamp, small enough that `?1 - 3603000` can't overflow.
const ALL_TIME: (i64, i64) = (i64::MIN / 2, i64::MAX / 2);

/// Read side for the query commands. `None` arguments mean "the current one", resolved against
/// the current tracking Day.
pub struct Reader {
    pub(super) conn: Connection,
    data_dir: PathBuf,
    /// Asked once per query, so a time zone change applies without reopening the reader.
    pub(super) tz: Box<dyn Fn() -> TimeZone + Send>,
    pub(super) now_ms: Box<dyn Fn() -> i64 + Send>,
}

impl Reader {
    /// A new `query_only` connection to `<data_dir>/tracker.db`, which must already be migrated
    /// (`RuntimeHandle::start` does that). `tz` and `now_ms` are the time zone and wall clock,
    /// asked once per query; scenarios pass fixed ones.
    pub fn open(
        data_dir: &Path,
        tz: impl Fn() -> TimeZone + Send + 'static,
        now_ms: impl Fn() -> i64 + Send + 'static,
    ) -> Result<Self, StoreError> {
        Ok(Self {
            conn: connect_reader(data_dir)?,
            data_dir: data_dir.to_owned(),
            tz: Box::new(tz),
            now_ms: Box::new(now_ms),
        })
    }

    /// `get_day`: totals and items for `day` ("YYYY-MM-DD"), or today. Today also lists every
    /// tracked target at 0 s under its current Category.
    pub fn day(&self, day: Option<&str>) -> Result<DaySummary, StoreError> {
        let tz = (self.tz)();
        let today = self.today(&tz)?;
        let day = day.map(parse_day).transpose()?.unwrap_or(today);
        let (start, end) = day_bounds(day, &tz)?;
        let mut items = self.items_between(start, end)?;
        let is_today = day == today;
        if is_today {
            for idle in self.tracked_items()? {
                if !items
                    .iter()
                    .any(|i| i.target == idle.target && i.category == idle.category)
                {
                    items.push(idle);
                }
            }
        }
        sort_items(&mut items);
        let (productive_secs, entertainment_secs) = sum_items(&items);
        Ok(DaySummary {
            totals: DayTotals {
                day: day.to_string(),
                productive_secs,
                entertainment_secs,
            },
            goal_secs: self.goal_secs()?,
            is_today,
            items,
        })
    }

    /// `get_week`: the 7 Days from `start` (a Monday), or the current week. A non-Monday start is
    /// `StoreError::Invalid`.
    pub fn week(&self, start: Option<&str>) -> Result<WeekSummary, StoreError> {
        let tz = (self.tz)();
        let current = week_start(self.today(&tz)?)?;
        let start = match start {
            Some(text) => {
                let day = parse_day(text)?;
                if day.weekday() != Weekday::Monday {
                    return Err(StoreError::Invalid(
                        "A week has to start on a Monday.".into(),
                    ));
                }
                day
            }
            None => current,
        };
        Ok(WeekSummary {
            start: start.to_string(),
            is_current: start == current,
            days: self.day_totals(start, 7, &tz)?,
            goal_secs: self.goal_secs()?,
        })
    }

    /// `get_share`: totals and items for the current week, the current month or all time.
    pub fn share(&self, range: ShareRange) -> Result<Share, StoreError> {
        let tz = (self.tz)();
        let today = self.today(&tz)?;
        let (start, end) = match range {
            ShareRange::Week => span(week_start(today)?, 7, &tz)?,
            ShareRange::Month => {
                let first = today.first_of_month();
                span(first, first.days_in_month().unsigned_abs().into(), &tz)?
            }
            ShareRange::All => ALL_TIME,
        };
        let mut items = self.items_between(start, end)?;
        sort_items(&mut items);
        let (productive_secs, entertainment_secs) = sum_items(&items);
        Ok(Share {
            productive_secs,
            entertainment_secs,
            items,
        })
    }

    /// `get_year`: every Day of `year`, or of the current tracking Day's year, zeros included.
    pub fn year(&self, year: Option<i32>) -> Result<YearHeatmap, StoreError> {
        let tz = (self.tz)();
        let current = i32::from(self.today(&tz)?.year());
        let year = year.unwrap_or(current);
        let first = i16::try_from(year)
            .ok()
            .and_then(|y| Date::new(y, 1, 1).ok())
            .ok_or_else(|| StoreError::Invalid(format!("{year} isn't a year we can show.")))?;
        Ok(YearHeatmap {
            year,
            is_current: year == current,
            goal_secs: self.goal_secs()?,
            days: self.day_totals(first, first.days_in_year().unsigned_abs().into(), &tz)?,
        })
    }

    /// `list_sites`: every site row, tracked or not, ordered by domain.
    pub fn sites(&self) -> Result<Vec<Site>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, domain, keyword, category FROM site ORDER BY domain")?;
        let sites = stmt
            .query_map([], |row| {
                Ok(Site {
                    id: row.get(0)?,
                    domain: row.get(1)?,
                    keyword: row.get(2)?,
                    category: row.get::<_, Option<SqlCategory>>(3)?.map(|c| c.0),
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(sites)
    }

    /// Every app row, tracked or not (`category: None`), for `list_catalog` to merge with installed
    /// and running apps by identity: an untracked row still holds the app's watch setting.
    /// `is_running` is always false here: the catalog knows what runs.
    pub fn stored_apps(&self) -> Result<Vec<CatalogApp>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT kind, key, name, category, watch FROM app ORDER BY name")?;
        let apps = stmt
            .query_map([], |row| {
                let kind: String = row.get(0)?;
                let identity = identity_from_columns(&kind, row.get(1)?)?;
                Ok(CatalogApp {
                    icon_path: existing_icon(&self.data_dir, &identity),
                    is_browser: is_browser(&identity),
                    identity,
                    name: row.get(2)?,
                    category: row.get::<_, Option<SqlCategory>>(3)?.map(|c| c.0),
                    is_running: false,
                    watch: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(apps)
    }

    /// `get_settings`. `launch_at_login` is always false here; the shell fills it in from the
    /// autostart plugin.
    pub fn settings(&self) -> Result<Settings, StoreError> {
        let settings = self.conn.query_row(
            "SELECT daily_goal_minutes, onboarded, appearance, away_after_minutes, waiting_credit,
                    auto_update
             FROM settings WHERE id = 1",
            [],
            |row| {
                let SqlAppearance(appearance) = row.get(2)?;
                Ok(Settings {
                    daily_goal_minutes: row.get(0)?,
                    launch_at_login: false,
                    onboarded: row.get(1)?,
                    appearance,
                    away_after_minutes: row.get(3)?,
                    waiting_credit: row.get(4)?,
                    auto_update: row.get(5)?,
                })
            },
        )?;
        Ok(settings)
    }

    /// When the current pause ends (UTC epoch ms), or `None` when not paused. An expired pause is
    /// no pause, so nothing has to clear it.
    pub fn paused_until_ms(&self) -> Result<Option<i64>, StoreError> {
        let until: Option<i64> = self.conn.query_row(
            "SELECT paused_until_ms FROM settings WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        let now = (self.now_ms)();
        Ok(until.filter(|&until| until > now))
    }

    /// Productive seconds of the current tracking Day, for the tray tooltip. Same number as the
    /// Day view's hero.
    pub fn today_productive_secs(&self) -> Result<u32, StoreError> {
        Ok(self.day(None)?.totals.productive_secs)
    }

    fn today(&self, tz: &TimeZone) -> Result<Date, StoreError> {
        Ok(day_of((self.now_ms)(), tz)?)
    }

    fn goal_secs(&self) -> Result<u32, StoreError> {
        Ok(self.settings()?.daily_goal_minutes * 60)
    }

    /// Time per (target, category) within `[start, end)`, foreground and waiting rows together,
    /// plus the waiting part on its own; only pairs with at least a whole second, since less would
    /// list as 0 s (today's 0 s rows come from `tracked_items` instead). Totals don't change: they
    /// sum floored seconds. Rows never overlap within a stretch (ADR-0007), so the sum is time.
    pub(super) fn items_between(&self, start: i64, end: i64) -> Result<Vec<DayItem>, StoreError> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {ITEM_COLUMNS}, SUM(MIN(i.end_ms, ?2) - MAX(i.start_ms, ?1)) AS ms,
                    SUM(CASE WHEN i.minimized THEN MIN(i.end_ms, ?2) - MAX(i.start_ms, ?1) ELSE 0 END)
             FROM interval i
             LEFT JOIN app a ON a.id = i.app_id
             LEFT JOIN site s ON s.id = i.site_id
             WHERE {OVERLAPS}
             GROUP BY i.app_id, i.site_id, i.category
             HAVING ms >= 1000"
        ))?;
        let items = stmt
            .query_map(params![start, end], |row| self.item(row))?
            .collect::<Result<_, _>>()?;
        Ok(items)
    }

    /// Every tracked target at 0 s under its current Category.
    fn tracked_items(&self) -> Result<Vec<DayItem>, StoreError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT a.id, NULL, a.category, a.kind, a.key, a.name, NULL, 0, 0
             FROM app a WHERE a.category IS NOT NULL
             UNION ALL
             SELECT NULL, s.id, s.category, NULL, NULL, NULL, s.domain, 0, 0
             FROM site s WHERE s.category IS NOT NULL",
        )?;
        let items = stmt
            .query_map([], |row| self.item(row))?
            .collect::<Result<_, _>>()?;
        Ok(items)
    }

    /// Maps an `ITEM_COLUMNS, ms, waiting ms` row. Exactly one of app_id/site_id is set (a
    /// schema CHECK). Both durations floor on their own, so `minimized_secs <= secs`.
    fn item(&self, row: &Row<'_>) -> rusqlite::Result<DayItem> {
        let SqlCategory(category) = row.get(2)?;
        let secs = ms_to_secs(row.get(7)?);
        let minimized_secs = ms_to_secs(row.get(8)?);
        Ok(match row.get::<_, Option<u32>>(0)? {
            Some(id) => {
                let kind: String = row.get(3)?;
                let identity = identity_from_columns(&kind, row.get(4)?)?;
                DayItem {
                    target: TargetRef::App { id },
                    name: row.get(5)?,
                    icon_path: existing_icon(&self.data_dir, &identity),
                    category,
                    secs,
                    minimized_secs,
                    is_browser: is_browser(&identity),
                }
            }
            None => DayItem {
                target: TargetRef::Site { id: row.get(1)? },
                name: row.get(6)?,
                icon_path: None,
                category,
                secs,
                minimized_secs,
                is_browser: false,
            },
        })
    }

    /// Per-Day totals for `count` Days from `first`: one fetch, bucketed in Rust by Day bounds.
    /// Each Day is summed the way `day` sums it, from per-item floored seconds over foreground and
    /// waiting rows alike, so a Day's bar or heatmap cell always shows its Day view's number.
    fn day_totals(
        &self,
        first: Date,
        count: usize,
        tz: &TimeZone,
    ) -> Result<Vec<DayTotals>, StoreError> {
        let bounds = boundaries(first, count, tz)?;
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT i.app_id, i.site_id, i.category, i.start_ms, i.end_ms FROM interval i
             WHERE {OVERLAPS}"
        ))?;
        let mut rows = stmt.query(params![bounds[0], bounds[count]])?;
        // ms per (Day index, item), an item being what `day` groups by: (app, site, productive).
        let mut items: HashMap<(usize, Option<u32>, Option<u32>, bool), i64> = HashMap::new();
        while let Some(row) = rows.next()? {
            let (app_id, site_id): (Option<u32>, Option<u32>) = (row.get(0)?, row.get(1)?);
            let SqlCategory(category) = row.get(2)?;
            let productive = category == Category::Productive;
            let (start, end): (i64, i64) = (row.get(3)?, row.get(4)?);
            // Rows are at most about 1 h, so this walks one or two Days.
            let mut day = bounds.partition_point(|&b| b <= start).saturating_sub(1);
            while day < count && bounds[day] < end {
                let overlap = end.min(bounds[day + 1]) - start.max(bounds[day]);
                if overlap > 0 {
                    *items.entry((day, app_id, site_id, productive)).or_default() += overlap;
                }
                day += 1;
            }
        }
        // (productive, entertainment) seconds per Day.
        let mut secs = vec![(0_u32, 0_u32); count];
        for ((day, _, _, productive), ms) in items {
            let slot = &mut secs[day];
            if productive {
                slot.0 = slot.0.saturating_add(ms_to_secs(ms));
            } else {
                slot.1 = slot.1.saturating_add(ms_to_secs(ms));
            }
        }
        Ok(first
            .series(1.day())
            .zip(secs)
            .map(|(day, (productive_secs, entertainment_secs))| DayTotals {
                day: day.to_string(),
                productive_secs,
                entertainment_secs,
            })
            .collect())
    }
}

/// `[start, end)` of `count` Days from `first`.
fn span(first: Date, count: usize, tz: &TimeZone) -> Result<(i64, i64), StoreError> {
    let bounds = boundaries(first, count, tz)?;
    Ok((bounds[0], bounds[count]))
}

/// Strict "YYYY-MM-DD": the round trip rejects anything jiff would also accept.
fn parse_day(text: &str) -> Result<Date, StoreError> {
    text.parse::<Date>()
        .ok()
        .filter(|day| day.to_string() == text)
        .ok_or_else(|| StoreError::Invalid(format!("\"{text}\" isn't a valid day.")))
}

fn ms_to_secs(ms: i64) -> u32 {
    u32::try_from(ms / 1000).unwrap_or(u32::MAX)
}

/// Secs desc, then name, so equal items keep a stable order.
pub(super) fn sort_items(items: &mut [DayItem]) {
    items.sort_by(|a, b| b.secs.cmp(&a.secs).then_with(|| a.name.cmp(&b.name)));
}

/// (productive, entertainment) seconds; totals are the sum of the listed items.
fn sum_items(items: &[DayItem]) -> (u32, u32) {
    items
        .iter()
        .fold((0_u32, 0_u32), |(p, e), item| match item.category {
            Category::Productive => (p.saturating_add(item.secs), e),
            Category::Entertainment => (p, e.saturating_add(item.secs)),
        })
}
