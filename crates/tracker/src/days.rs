//! Tracking Day math: a Day runs from local 04:00 to local 04:00, weeks start Monday.
//!
//! Everything is civil (wall-clock) arithmetic through the time-zone database, never `ms - 4h`,
//! because some zones (e.g. EET) shift their clocks between 3 and 4am.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};

/// Local hour at which a tracking Day starts.
const DAY_START_HOUR: i8 = 4;

/// The tracking Day containing the instant `ms` (UTC epoch ms): before 04:00 local belongs to the
/// previous date.
pub fn day_of(ms: i64, tz: &TimeZone) -> Result<Date, jiff::Error> {
    let local = Timestamp::from_millisecond(ms)?.to_zoned(tz.clone());
    if local.hour() < DAY_START_HOUR {
        local.date().yesterday()
    } else {
        Ok(local.date())
    }
}

/// `[start, end)` in UTC epoch ms: local 04:00 of `day` to local 04:00 of the next date. A 04:00
/// that falls in a DST gap resolves "compatibly" (to the instant after the gap), so Days tile.
pub fn day_bounds(day: Date, tz: &TimeZone) -> Result<(i64, i64), jiff::Error> {
    Ok((day_start(day, tz)?, day_start(day.tomorrow()?, tz)?))
}

/// The start of the tracking Day after the one containing `ms`: where "Pause until tomorrow" ends.
/// At 2am that is 4am the same date, since 2am still belongs to the previous Day.
pub fn next_day_start(ms: i64, tz: &TimeZone) -> Result<i64, jiff::Error> {
    Ok(day_bounds(day_of(ms, tz)?, tz)?.1)
}

fn day_start(day: Date, tz: &TimeZone) -> Result<i64, jiff::Error> {
    // `DateTime::to_zoned` uses compatible disambiguation.
    let zoned = day.at(DAY_START_HOUR, 0, 0, 0).to_zoned(tz.clone())?;
    Ok(zoned.timestamp().as_millisecond())
}

/// The Monday of the week containing `day`.
pub fn week_start(day: Date) -> Result<Date, jiff::Error> {
    day.checked_sub(i64::from(day.weekday().to_monday_zero_offset()).days())
}

/// The start of each of `count` consecutive Days from `first`, plus the end of the last one:
/// `count + 1` ascending boundaries for bucketing rows by Day.
pub fn boundaries(first: Date, count: usize, tz: &TimeZone) -> Result<Vec<i64>, jiff::Error> {
    first
        .series(1.day())
        .take(count + 1)
        .map(|d| day_start(d, tz))
        .collect()
}
