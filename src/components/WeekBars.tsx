import { type FocusEvent, type PointerEvent, useRef, useState } from "react";
import type { DayTotals, WeekSummary } from "../api/bindings";
import { formatLongDay, formatWeekday, formatWeekRange } from "../lib/day";
import { ChartTooltip, type TooltipAnchor, TooltipRow } from "./ChartTooltip";
import { Duration, formatDuration, formatGoal } from "./Duration";

const HOUR = 3600;

/** A clean hour scale with at most four steps that clears the goal line and the tallest bar. */
function scale(maxSecs: number) {
  const step = [1, 2, 3, 4, 6, 8, 12].find((h) => maxSecs / (h * HOUR) <= 4) ?? 12;
  const top = Math.max(1, Math.ceil(maxSecs / (step * HOUR))) * step * HOUR;
  const ticks = Array.from({ length: top / (step * HOUR) + 1 }, (_, i) => i * step * HOUR);
  return { top, ticks };
}

const pct = (secs: number, top: number) => `${(secs / top) * 100}%`;

type Props = {
  week: WeekSummary;
  /** Today's Day, so future days in the current week read as not-yet. */
  today: string | undefined;
  onOpenDay: (day: string) => void;
};

/**
 * One week, a column per Day: Productive from the baseline with Entertainment stacked above it
 * behind a 2px gap, against a dashed Daily Goal line. Bars ease to new heights when paging weeks.
 */
export function WeekBars({ week, today, onOpenDay }: Props) {
  const plotRef = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<{ day: DayTotals; anchor: TooltipAnchor } | null>(null);

  const productive = week.days.reduce((s, d) => s + d.productiveSecs, 0);
  const entertainment = week.days.reduce((s, d) => s + d.entertainmentSecs, 0);
  const tallest = Math.max(...week.days.map((d) => d.productiveSecs + d.entertainmentSecs));
  const { top, ticks } = scale(Math.max(tallest, week.goalSecs * 1.15, HOUR));
  const isFuture = (day: string) => today !== undefined && day > today;

  const show = (day: DayTotals, index: number) => (e: PointerEvent | FocusEvent) => {
    const plot = plotRef.current?.getBoundingClientRect();
    const col = e.currentTarget.getBoundingClientRect();
    if (!plot) return;
    const total = day.productiveSecs + day.entertainmentSecs;
    setHover({
      day,
      anchor: {
        x: col.left - plot.left + col.width / 2,
        y: plot.height * (1 - total / top),
        align: index === 0 ? "start" : index === 6 ? "end" : "center",
      },
    });
  };

  return (
    <div className="flex h-full flex-col px-5 pt-4.5 pb-4">
      <header className="flex items-start justify-between gap-6">
        <dl className="flex gap-8">
          <LegendStat color="var(--productive)" label="Productive" secs={productive} />
          <LegendStat color="var(--entertainment)" label="Entertainment" secs={entertainment} />
        </dl>
        <p className="flex items-center gap-2 pt-0.5 text-meta text-fg-3">
          <span className="w-4 border-t border-dashed border-fg-3" />
          Daily goal {formatGoal(Math.round(week.goalSecs / 60))}
        </p>
      </header>

      <div className="mt-6.5 flex min-h-0 flex-1 flex-col">
        <div ref={plotRef} className="relative ml-9 min-h-0 flex-1">
          {ticks.map((t) => (
            <div
              key={t}
              className="absolute inset-x-0 border-t border-border"
              style={{ bottom: pct(t, top) }}
            >
              <span className="absolute -left-9 w-7 -translate-y-1/2 text-right text-meta tabular-nums text-fg-3">
                {t === 0 ? "0" : `${t / HOUR}h`}
              </span>
            </div>
          ))}
          <div
            aria-hidden="true"
            className="absolute inset-x-0 border-t border-dashed border-fg-3/70 transition-[bottom] duration-(--dur-chart) ease-drawer"
            style={{ bottom: pct(week.goalSecs, top) }}
          />

          <fieldset
            aria-label={`Week of ${formatWeekRange(week.start)}: Productive ${formatDuration(productive)}, Entertainment ${formatDuration(entertainment)}`}
            className="absolute inset-0 grid min-w-0 grid-cols-7"
            onPointerLeave={() => setHover(null)}
          >
            {week.days.map((d, i) => {
              const future = isFuture(d.day);
              const hasEntertainment = d.entertainmentSecs > 0;
              const p = d.productiveSecs > 0 ? `max(2px, ${pct(d.productiveSecs, top)})` : "0px";
              return (
                <button
                  key={d.day}
                  type="button"
                  disabled={future}
                  onClick={() => onOpenDay(d.day)}
                  onPointerEnter={future ? undefined : show(d, i)}
                  onFocus={show(d, i)}
                  onBlur={() => setHover(null)}
                  aria-label={`${formatLongDay(d.day)}: Productive ${formatDuration(d.productiveSecs)}, Entertainment ${formatDuration(d.entertainmentSecs)}`}
                  className="group relative mx-1 rounded-control outline-offset-0 transition-colors duration-(--dur-fast) ease-standard enabled:hover:bg-ghost-hover"
                >
                  {/* A quarter of its column, 20 to 40px: about 26px at the default size. */}
                  <span className="absolute inset-y-0 left-1/2 w-[clamp(20px,25%,40px)] -translate-x-1/2 transition-[filter] duration-(--dur-fast) group-hover:brightness-115">
                    <span
                      className={`absolute inset-x-0 bottom-0 bg-productive transition-[height] duration-(--dur-chart) ease-drawer ${hasEntertainment ? "" : "rounded-t-[4px]"}`}
                      style={{ height: p, filter: "var(--productive-glow)" }}
                    />
                    <span
                      className="absolute inset-x-0 rounded-t-[4px] bg-entertainment transition-[height,bottom] duration-(--dur-chart) ease-drawer"
                      style={{
                        bottom: d.productiveSecs > 0 ? `calc(${p} + 2px)` : "0px",
                        height: hasEntertainment
                          ? `max(2px, ${pct(d.entertainmentSecs, top)})`
                          : "0px",
                      }}
                    />
                  </span>
                </button>
              );
            })}
          </fieldset>

          {tallest === 0 && (
            <p className="absolute inset-x-0 top-1/3 text-center text-meta text-fg-3">
              No tracked time this week
            </p>
          )}

          <ChartTooltip
            anchor={hover?.anchor ?? null}
            title={hover ? formatLongDay(hover.day.day) : ""}
          >
            {hover && (
              <>
                <TooltipRow
                  color="var(--productive)"
                  secs={hover.day.productiveSecs}
                  label="Productive"
                />
                <TooltipRow
                  color="var(--entertainment)"
                  secs={hover.day.entertainmentSecs}
                  label="Entertainment"
                />
              </>
            )}
          </ChartTooltip>
        </div>

        <div className="mt-2.5 ml-9 grid grid-cols-7">
          {week.days.map((d) => (
            <p
              key={d.day}
              className={`text-center text-meta ${
                d.day === today
                  ? "font-medium text-fg-1"
                  : isFuture(d.day)
                    ? "text-fg-4"
                    : "text-fg-3"
              }`}
            >
              {formatWeekday(d.day)} <span className="tabular-nums">{Number(d.day.slice(8))}</span>
            </p>
          ))}
        </div>
      </div>
    </div>
  );
}

function LegendStat({ color, label, secs }: { color: string; label: string; secs: number }) {
  return (
    <div>
      <dt className="flex items-center gap-2 text-meta text-fg-2">
        <span className="size-2 rounded-[2px]" style={{ background: color }} />
        {label}
      </dt>
      <dd className="mt-1 pl-4 text-title font-medium">
        <Duration secs={secs} />
      </dd>
    </div>
  );
}
