//! `export_data` (issue 13): daily totals as CSV, or everything stored as JSON.

use std::fmt::Write;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::Serialize;
use tracker_api::{AppIdentity, Appearance, Category, ExportFormat, Site, TargetRef};

use super::queries::{Reader, sort_items};
use super::{SqlCategory, StoreError, identity_from_columns};
use crate::days::{day_bounds, day_of};

/// Tracking Days start at this local time; written into the JSON so it can be bucketed the same way.
const DAY_STARTS_AT: &str = "04:00";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Export {
    exported_at: String,
    /// IANA name of the zone the timestamps are written in and Days are counted in.
    time_zone: String,
    day_starts_at: &'static str,
    settings: ExportSettings,
    /// Every app row, tracked or not (`category: null`), since intervals may name untracked ones.
    apps: Vec<ExportApp>,
    sites: Vec<Site>,
    intervals: Vec<ExportInterval>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportSettings {
    daily_goal_minutes: u32,
    away_after_minutes: u32,
    waiting_credit: bool,
    appearance: Appearance,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportApp {
    id: u32,
    identity: AppIdentity,
    name: String,
    category: Option<Category>,
    /// "Counts while I watch" (ADR-0008).
    watch: bool,
}

/// One stored row as is: RFC 3339 times with the local offset, exact to the millisecond.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportInterval {
    start: String,
    end: String,
    target: TargetRef,
    /// The Category at the time (ADR-0005).
    category: Category,
    /// Waiting credit (ADR-0007).
    minimized: bool,
}

impl Reader {
    pub fn export(&self, format: ExportFormat) -> Result<String, StoreError> {
        match format {
            ExportFormat::Csv => self.export_csv(),
            ExportFormat::Json => self.export_json(),
        }
    }

    /// One line per (Day, target, category) with time, oldest Day first: the same items and whole
    /// seconds the Day view shows for that Day.
    fn export_csv(&self) -> Result<String, StoreError> {
        let tz = (self.tz)();
        let mut csv = String::from("day,name,kind,category,seconds,waiting_seconds\r\n");
        let span: Option<(i64, i64)> = self.conn.query_row(
            "SELECT MIN(start_ms), MAX(end_ms) FROM interval",
            [],
            |row| Ok(row.get::<_, Option<i64>>(0)?.zip(row.get(1)?)),
        )?;
        let Some((first, last)) = span else {
            return Ok(csv);
        };
        let last_day = day_of((last - 1).max(first), &tz)?;
        let mut day = day_of(first, &tz)?;
        while day <= last_day {
            let (start, end) = day_bounds(day, &tz)?;
            let mut items = self.items_between(start, end)?;
            sort_items(&mut items);
            for item in items {
                let (kind, category) = (
                    match item.target {
                        TargetRef::App { .. } => "app",
                        TargetRef::Site { .. } => "site",
                    },
                    match item.category {
                        Category::Productive => "productive",
                        Category::Entertainment => "entertainment",
                    },
                );
                // Writing to a String can't fail.
                let _ = write!(
                    csv,
                    "{day},{},{kind},{category},{},{}\r\n",
                    csv_field(&item.name),
                    item.secs,
                    item.minimized_secs
                );
            }
            day = day.tomorrow()?;
        }
        Ok(csv)
    }

    fn export_json(&self) -> Result<String, StoreError> {
        let tz = (self.tz)();
        let settings = self.settings()?;
        let mut apps = self
            .conn
            .prepare("SELECT id, kind, key, name, category, watch FROM app ORDER BY id")?;
        let apps = apps
            .query_map([], |row| {
                let kind: String = row.get(1)?;
                Ok(ExportApp {
                    id: row.get(0)?,
                    identity: identity_from_columns(&kind, row.get(2)?)?,
                    name: row.get(3)?,
                    category: row.get::<_, Option<SqlCategory>>(4)?.map(|c| c.0),
                    watch: row.get(5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let mut rows = self.conn.prepare(
            "SELECT app_id, site_id, category, start_ms, end_ms, minimized FROM interval
             ORDER BY start_ms, id",
        )?;
        let mut rows = rows.query([])?;
        let mut intervals = Vec::new();
        while let Some(row) = rows.next()? {
            let target = match row.get::<_, Option<u32>>(0)? {
                Some(id) => TargetRef::App { id },
                None => TargetRef::Site { id: row.get(1)? },
            };
            let SqlCategory(category) = row.get(2)?;
            intervals.push(ExportInterval {
                start: local_time(row.get(3)?, &tz)?,
                end: local_time(row.get(4)?, &tz)?,
                target,
                category,
                minimized: row.get(5)?,
            });
        }
        let export = Export {
            exported_at: local_time((self.now_ms)(), &tz)?,
            time_zone: tz.iana_name().unwrap_or("unknown").to_owned(),
            day_starts_at: DAY_STARTS_AT,
            settings: ExportSettings {
                daily_goal_minutes: settings.daily_goal_minutes,
                away_after_minutes: settings.away_after_minutes,
                waiting_credit: settings.waiting_credit,
                appearance: settings.appearance,
            },
            apps,
            sites: self.sites()?,
            intervals,
        };
        Ok(serde_json::to_string_pretty(&export)?)
    }
}

/// RFC 3339 with the zone's offset at that instant, e.g. `2026-10-21T10:00:00-04:00`.
fn local_time(ms: i64, tz: &TimeZone) -> Result<String, StoreError> {
    let zoned = Timestamp::from_millisecond(ms)?.to_zoned(tz.clone());
    Ok(zoned
        .timestamp()
        .display_with_offset(zoned.offset())
        .to_string())
}

/// Quoted, with quotes doubled, when the text holds a comma, a quote or a line break (RFC 4180).
fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}
