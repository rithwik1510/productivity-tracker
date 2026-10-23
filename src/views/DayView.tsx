import { useEffect } from "react";
import type { Category, DayItem, DaySummary, TrackerStatus } from "../api/bindings";
import { useDay, usePause, useResume, useStatus } from "../api/queries";
import { AppIcon } from "../components/AppIcon";
import { AppRow } from "../components/AppRow";
import { PRIMARY, TITLEBAR_GHOST, TITLEBAR_ICON } from "../components/buttons";
import { Duration, formatDuration } from "../components/Duration";
import { GlassCard } from "../components/GlassCard";
import { GoalRing } from "../components/GoalRing";
import { ChevronLeft, ChevronRight } from "../components/Icons";
import { QuietError } from "../components/QuietError";
import { WindowTitle } from "../components/TitleBar";
import { addDays, formatLongDay } from "../lib/day";
import { useUi } from "../state/ui";

const CATEGORIES: readonly { category: Category; label: string; color: string }[] = [
  { category: "productive", label: "Productive", color: "var(--productive)" },
  { category: "entertainment", label: "Entertainment", color: "var(--entertainment)" },
];

export function DayView() {
  const selectedDay = useUi((s) => s.selectedDay);
  const { data, error, isPlaceholderData, isFetching } = useDay(selectedDay);
  const status = useStatus(data?.isToday === true).data;

  // Showing today means following today: null lets the 4am rollover move the view along.
  // Placeholder data is the previous Day's, and a cached copy can predate the rollover (isToday
  // for what is now yesterday), so only settled, fresh data may trigger this.
  useEffect(() => {
    if (data?.isToday && !isPlaceholderData && !isFetching && selectedDay !== null) {
      useUi.setState({ selectedDay: null });
    }
  }, [data, isPlaceholderData, isFetching, selectedDay]);

  if (!data) return error ? <QuietError error={error} /> : null;
  const { totals, isToday } = data;
  const over = totals.productiveSecs >= data.goalSecs;

  return (
    // Narrow (a window under about 900px): the ring card becomes a strip over the full-width list
    // and the page scrolls, so rows keep their names. pb-6 stands in for <main>'s bottom padding,
    // which an overflowing view doesn't get.
    <div className="flex h-full flex-col gap-4 @max-narrow:h-auto @max-narrow:pb-6">
      <WindowTitle
        title={isToday ? "Today" : formatLongDay(totals.day)}
        meta={isToday ? formatLongDay(totals.day) : totals.day.slice(0, 4)}
      >
        {!isToday && (
          <button
            type="button"
            className={TITLEBAR_GHOST}
            onClick={() => useUi.setState({ selectedDay: null })}
          >
            Back to today
          </button>
        )}
        <button
          type="button"
          aria-label="Previous day"
          title="Previous day"
          className={TITLEBAR_ICON}
          onClick={() => useUi.setState({ selectedDay: addDays(totals.day, -1) })}
        >
          <ChevronLeft />
        </button>
        <button
          type="button"
          aria-label="Next day"
          title="Next day"
          className={TITLEBAR_ICON}
          disabled={isToday}
          onClick={() => useUi.setState({ selectedDay: addDays(totals.day, 1) })}
        >
          <ChevronRight />
        </button>
      </WindowTitle>

      {error && <QuietError error={error} />}

      {/* Side by side, the ring column is 36% of the view column (316px at the default size), from
          268px up to 420px; the list takes the rest, which the Day column's 1160px cap keeps under
          about 720px. The grid fills the height up to --view-max-h. */}
      <div className="grid max-h-(--view-max-h) min-h-0 flex-1 grid-cols-[clamp(268px,36cqw,420px)_minmax(0,1fr)] gap-4 @max-narrow:max-h-none @max-narrow:flex-none @max-narrow:grid-cols-1">
        <GlassCard className="flex min-h-0 flex-col px-5 pb-4.5 @max-narrow:flex-row @max-narrow:pb-0">
          {/* The status line sits in the ring box's bottom padding, so the ring never moves. The
              box is a size container: the ring fits both its width and its height. In the narrow
              strip it has a fixed height and takes the width beside the stats. */}
          <div className="relative grid min-h-0 flex-1 place-items-center pt-3.5 pb-8 @container-size @max-narrow:h-62">
            <GoalRing
              productiveSecs={totals.productiveSecs}
              goalSecs={data.goalSecs}
              day={totals.day}
            />
            {isToday && status && <StatusLine status={status} />}
          </div>
          {/* Under the ring, two columns; beside it in the narrow strip, one column. */}
          <dl className="grid grid-cols-2 border-t border-border pt-3.5 @max-narrow:w-48 @max-narrow:shrink-0 @max-narrow:grid-cols-1 @max-narrow:content-center @max-narrow:gap-5 @max-narrow:border-t-0 @max-narrow:border-l @max-narrow:pt-0 @max-narrow:pl-5">
            <div>
              <dt className="flex items-center gap-2 text-meta text-fg-2">
                <span className="size-2 rounded-full bg-entertainment" />
                Entertainment
              </dt>
              <dd className="mt-1 text-title font-medium">
                <Duration secs={totals.entertainmentSecs} />
              </dd>
            </div>
            <div className="border-l border-border pl-4.5 @max-narrow:border-l-0 @max-narrow:pl-0">
              <dt className="text-meta text-fg-2">
                {over ? "Above goal" : isToday ? "Left to goal" : "Below goal"}
              </dt>
              <dd className="mt-1 text-title font-medium">
                {/* From the shown (floored) minutes, so 36m of 8h reads 7h 24m, not 7h 23m. */}
                <Duration
                  secs={Math.abs(data.goalSecs - Math.floor(totals.productiveSecs / 60) * 60)}
                />
              </dd>
            </div>
          </dl>
        </GlassCard>

        <GlassCard className="min-h-0 overflow-y-auto">
          <ItemList data={data} />
        </GlassCard>
      </div>
    </div>
  );
}

/**
 * Today's status line under the ring: what the tracker is doing right now. The backend picks the
 * one state that holds, in TrackerStatus's order: a failed save, a pause, away, tracking, then
 * nothing tracked in front.
 */
function StatusLine({ status }: { status: TrackerStatus }) {
  const pausedUntil = usePause(status.state === "paused").data;
  const resume = useResume();
  const at = "absolute inset-x-0 flex items-center justify-center text-meta text-fg-3";
  const line = `${at} bottom-2 gap-2`;
  switch (status.state) {
    case "saveFailed":
      // The one warning in the app, so it borrows the danger red for its dot; the words stay
      // high-contrast text.
      return (
        <p role="status" className={line}>
          <span className="size-2 shrink-0 rounded-full bg-danger" />
          <span className="text-fg-1">Can't save, retrying</span>
        </p>
      );
    case "paused":
      // Paused from the tray: the pause replaces what is being counted (nothing is).
      return (
        <p className={`${at} bottom-1 gap-1`}>
          <span className="truncate">
            {pausedUntil ? (
              <>
                Paused until <span className="text-fg-2">{pausedUntil}</span>
              </>
            ) : (
              "Paused"
            )}
          </span>
          <span aria-hidden="true">·</span>
          <button
            type="button"
            className="rounded-control px-1.5 py-0.5 font-medium text-fg-1 transition-colors duration-(--dur-fast) ease-standard hover:bg-ghost-hover disabled:opacity-40"
            disabled={resume.isPending}
            onClick={() => resume.mutate()}
          >
            Resume
          </button>
        </p>
      );
    case "away":
      // Says why a total just dropped: the Away rule trimmed the idle tail. Under a minute isn't
      // worth explaining.
      return (
        <p className={line}>
          <span className="truncate">
            Away
            {status.removedSecs >= 60 && (
              <>
                {" · removed "}
                <span className="text-fg-2">{formatDuration(status.removedSecs)}</span>
                {" idle time"}
              </>
            )}
          </span>
        </p>
      );
    case "tracking":
      return (
        <p className={line}>
          <span
            className={`size-2 shrink-0 rounded-full ${status.category === "productive" ? "bg-productive" : "bg-entertainment"}`}
          />
          <span className="min-w-0 truncate">
            Tracking · <span className="text-fg-2">{status.name}</span>
            {status.isBrowser && " · other sites"}
            {/* Minimized and counting as waiting credit (ADR-0007). */}
            {status.waiting && " · waiting"}
            {/* Counting without input only because it is watched (ADR-0008). */}
            {status.watching && " · watching"}
          </span>
        </p>
      );
    case "untracked":
      // Never the untracked app's name: only tracked things are ever named.
      return (
        <p className={line}>
          <span className="truncate">Nothing tracked in front</span>
        </p>
      );
  }
}

function ItemList({ data }: { data: DaySummary }) {
  if (data.items.length === 0) {
    // Today lists every tracked target even at 0s, so an empty today means nothing is tracked.
    return data.isToday ? (
      <div className="flex h-full flex-col items-center justify-center px-10 text-center">
        <h2 className="text-title">Nothing is tracked yet</h2>
        <p className="mt-1.5 max-w-80 text-fg-2">
          Pick the apps and sites that count. Only those are ever tracked, nothing else.
        </p>
        <button
          type="button"
          className={`${PRIMARY} mt-6`}
          onClick={() => useUi.setState({ view: "picker" })}
        >
          Choose apps
        </button>
      </div>
    ) : (
      <p className="grid h-full place-items-center text-meta text-fg-3">
        Nothing was tracked on this day.
      </p>
    );
  }

  // Bars share one scale across both groups, so lengths compare across categories.
  const longest = Math.max(...data.items.map((i) => i.secs), 1);
  // Everything tracked and nothing counted yet: right after first run, or early in the Day.
  const fresh = data.isToday && data.totals.productiveSecs + data.totals.entertainmentSecs === 0;
  return (
    <div className="py-1.5">
      {fresh && (
        <p className="px-4 pt-2.5 pb-0.5 text-meta text-fg-3">
          Tracking started. Time shows up here as you use these.
        </p>
      )}
      {CATEGORIES.map(({ category, label, color }) => {
        const items = data.items.filter((i) => i.category === category);
        if (items.length === 0) return null;
        const total =
          category === "productive" ? data.totals.productiveSecs : data.totals.entertainmentSecs;
        const shown = shownSecs(items, total);
        return (
          <section
            key={category}
            aria-label={label}
            className="not-first:mt-2 not-first:border-t not-first:border-border not-first:pt-2"
          >
            <h2 className="flex items-center justify-between px-4 pt-2 pb-1 text-meta font-semibold text-fg-2">
              <span className="flex items-center gap-2">
                <span className="size-2 rounded-full" style={{ background: color }} />
                {label}
              </span>
              <Duration secs={total} />
            </h2>
            {items.map((item, i) => (
              <Row
                key={`${item.target.kind}:${item.target.id}`}
                item={item}
                secs={shown[i] ?? item.secs}
                color={color}
                longest={longest}
              />
            ))}
          </section>
        );
      })}
    </div>
  );
}

/**
 * Display seconds per item whose whole minutes add up to the category total's minutes. Each item
 * floors on its own, so six rows can read 2m short of the header; the lost minutes go to the
 * items with the largest leftover seconds (largest remainder). Items left at 0 minutes keep their
 * real seconds, so they still read <1m.
 */
function shownSecs(items: DayItem[], total: number): number[] {
  const mins = items.map((i) => Math.floor(i.secs / 60));
  let missing = Math.floor(total / 60) - mins.reduce((a, b) => a + b, 0);
  const byRemainder = items
    .map((item, i) => ({ i, rem: item.secs % 60 }))
    .filter((r) => r.rem > 0)
    .sort((a, b) => b.rem - a.rem);
  for (const { i } of byRemainder) {
    if (missing <= 0) break;
    mins[i] = (mins[i] ?? 0) + 1;
    missing--;
  }
  return items.map((item, i) => {
    const m = mins[i] ?? 0;
    return m === Math.floor(item.secs / 60) || m === 0 ? item.secs : m * 60;
  });
}

function Row({
  item,
  secs,
  color,
  longest,
}: {
  item: DayItem;
  secs: number;
  color: string;
  longest: number;
}) {
  const notes: string[] = [];
  if (item.isBrowser) notes.push("other sites");
  // Waiting credit (a minimized app counting while nothing Productive was in front) is part of
  // secs, never added to it. Floored on its own, it can't read more than the row's own (floored
  // or rounded-up) minutes.
  if (item.minimizedSecs >= 60) {
    notes.push(`incl. ${formatDuration(item.minimizedSecs)} waiting`);
  }
  return (
    <AppRow
      icon={<AppIcon iconPath={item.iconPath} name={item.name} />}
      name={item.name}
      suffix={notes.length > 0 ? notes.join(" · ") : undefined}
    >
      <span
        className="h-1 w-14 shrink-0 overflow-hidden rounded-full bg-surface-2 min-[900px]:w-22"
        aria-hidden="true"
      >
        <span
          className="block h-full rounded-full"
          style={{ width: `${(item.secs / longest) * 100}%`, background: color }}
        />
      </span>
      <Duration secs={secs} className="w-16 shrink-0 text-right" />
    </AppRow>
  );
}
