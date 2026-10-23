// Day strings are "YYYY-MM-DD" tracking Days. All math is UTC calendar math on the string itself,
// so the local zone and DST never shift it. Which Day is "today" only the backend knows.

const DAY_MS = 86_400_000;

export function addDays(day: string, n: number): string {
  return new Date(Date.parse(day) + n * DAY_MS).toISOString().slice(0, 10);
}

/** 0 = Monday … 6 = Sunday, matching the backend's Monday-first weeks. */
export function weekday(day: string): number {
  return (new Date(Date.parse(day)).getUTCDay() + 6) % 7;
}

// Formatters pin timeZone UTC because the Date only carries the calendar date. The locale is named
// explicitly as navigator.language: in WebView2 it matches ICU's default, and unlike that default
// it follows Playwright's locale emulation, so e2e date names are stable.
const LOCALE = navigator.language;
const longDay = new Intl.DateTimeFormat(LOCALE, {
  weekday: "long",
  month: "long",
  day: "numeric",
  timeZone: "UTC",
});
const shortDate = new Intl.DateTimeFormat(LOCALE, {
  month: "short",
  day: "numeric",
  timeZone: "UTC",
});
const weekdayShort = new Intl.DateTimeFormat(LOCALE, { weekday: "short", timeZone: "UTC" });
const monthShort = new Intl.DateTimeFormat(LOCALE, { month: "short", timeZone: "UTC" });

/** "Wednesday, October 21" */
export const formatLongDay = (day: string) => longDay.format(Date.parse(day));
/** "Wed" */
export const formatWeekday = (day: string) => weekdayShort.format(Date.parse(day));
/** "Oct" */
export const formatMonth = (day: string) => monthShort.format(Date.parse(day));
/** "Oct 13 – 19" */
export const formatWeekRange = (start: string) =>
  shortDate.formatRange(Date.parse(start), Date.parse(addDays(start, 6)));
