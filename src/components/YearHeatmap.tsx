import { type FocusEvent, type KeyboardEvent, type PointerEvent, useRef, useState } from "react";
import type { DayTotals, YearHeatmap as YearData } from "../api/bindings";
import { formatLongDay, formatMonth, weekday } from "../lib/day";
import { ChartTooltip, type TooltipAnchor, TooltipRow } from "./ChartTooltip";
import { Duration, formatDuration } from "./Duration";

// Five steps relative to the Daily Goal: empty · <25% · <50% · <100% · goal met.
const HEAT = ["bg-heat-0", "bg-heat-1", "bg-heat-2", "bg-heat-3", "bg-heat-4"] as const;
const LEVEL_LABELS = ["None", "Under 25% of goal", "Under 50%", "Under 100%", "Goal met"];

function level(secs: number, goalSecs: number) {
  if (secs <= 0 || goalSecs <= 0) return 0;
  const r = secs / goalSecs;
  return r < 0.25 ? 1 : r < 0.5 ? 2 : r < 1 ? 3 : 4;
}

const ROW_LABELS: Record<number, string> = { 0: "Mon", 2: "Wed", 4: "Fri" };

type Props = {
  heat: YearData;
  today: string | undefined;
  onOpenDay: (day: string) => void;
};

/**
 * The year, one cell per Day (columns are Monday-first weeks), shaded by Productive time against
 * the goal. Cells never animate. Arrow keys move a single roving focus, so the year is one tab stop.
 */
export function YearHeatmap({ heat, today, onOpenDay }: Props) {
  const gridRef = useRef<HTMLFieldSetElement>(null);
  const cells = useRef<(HTMLButtonElement | null)[]>([]);
  const [hover, setHover] = useState<{ day: DayTotals; anchor: TooltipAnchor } | null>(null);

  const offset = heat.days[0] ? weekday(heat.days[0].day) : 0;
  const cols = Math.ceil((offset + heat.days.length) / 7);
  const isFuture = (day: string) => today !== undefined && day > today;
  // Last Day that can be opened: today in the current year, else the year's last Day.
  const lastOpen = heat.days.findLastIndex((d) => !isFuture(d.day));
  const [focusIndex, setFocusIndex] = useState(Math.max(0, lastOpen));
  const productive = heat.days.reduce((s, d) => s + d.productiveSecs, 0);
  const goalDays = heat.days.filter((d) => level(d.productiveSecs, heat.goalSecs) === 4).length;

  const show = (day: DayTotals, i: number) => (e: PointerEvent | FocusEvent) => {
    const grid = gridRef.current?.getBoundingClientRect();
    const cell = e.currentTarget.getBoundingClientRect();
    if (!grid) return;
    const col = Math.floor((i + offset) / 7);
    setHover({
      day,
      anchor: {
        x: cell.left - grid.left + cell.width / 2,
        y: cell.top - grid.top,
        align: col < 6 ? "start" : col > cols - 7 ? "end" : "center",
      },
    });
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const step = { ArrowUp: -1, ArrowDown: 1, ArrowLeft: -7, ArrowRight: 7 }[e.key];
    if (step === undefined) return;
    e.preventDefault();
    const next = Math.min(Math.max(focusIndex + step, 0), Math.max(0, lastOpen));
    setFocusIndex(next);
    cells.current[next]?.focus();
  };

  return (
    <div className="flex h-full flex-col px-5 pt-4.5 pb-4">
      <header className="flex items-start justify-between gap-6">
        <dl className="flex gap-8">
          <div>
            <dt className="text-meta text-fg-2">Goal met</dt>
            <dd className="mt-1 text-title font-medium tabular-nums">
              {goalDays}{" "}
              <span className="text-meta font-normal text-fg-2">
                {goalDays === 1 ? "day" : "days"}
              </span>
            </dd>
          </div>
          <div>
            <dt className="text-meta text-fg-2">Productive in {heat.year}</dt>
            <dd className="mt-1 text-title font-medium">
              <Duration secs={productive} />
            </dd>
          </div>
        </dl>
        <div className="flex items-center gap-2 pt-0.5 text-meta text-fg-3" aria-hidden="true">
          Less
          {HEAT.map((cls, i) => (
            <span key={cls} title={LEVEL_LABELS[i]} className={`size-3 rounded-[3px] ${cls}`} />
          ))}
          Goal met
        </div>
      </header>

      {/* At the card's optical centre: true centre, nudged up a little by the bottom padding. */}
      <div className="flex min-h-0 flex-1 items-center pb-[5%]">
        <fieldset
          ref={gridRef}
          aria-label={`${heat.year}: goal met on ${goalDays} days, ${formatDuration(productive)} Productive`}
          className="relative grid w-full min-w-0 gap-[3px]"
          style={{ gridTemplateColumns: `28px repeat(${cols}, minmax(0, 1fr))` }}
          onKeyDown={onKeyDown}
          onPointerLeave={() => setHover(null)}
        >
          {heat.days.map((d, i) =>
            d.day.endsWith("-01") ? (
              <span
                key={`m${d.day}`}
                aria-hidden="true"
                className="mb-1 text-micro whitespace-nowrap text-fg-3"
                style={{ gridRow: 1, gridColumn: Math.floor((i + offset) / 7) + 2 }}
              >
                {formatMonth(d.day)}
              </span>
            ) : null,
          )}
          {Object.entries(ROW_LABELS).map(([row, label]) => (
            <span
              key={label}
              aria-hidden="true"
              className="self-center text-micro leading-none text-fg-3"
              style={{ gridRow: Number(row) + 2, gridColumn: 1 }}
            >
              {label}
            </span>
          ))}
          {heat.days.map((d, i) => {
            const future = isFuture(d.day);
            return (
              <button
                key={d.day}
                ref={(el) => {
                  cells.current[i] = el;
                }}
                type="button"
                disabled={future}
                tabIndex={i === focusIndex ? 0 : -1}
                onClick={() => onOpenDay(d.day)}
                onFocus={(e) => {
                  setFocusIndex(i);
                  show(d, i)(e);
                }}
                onBlur={() => setHover(null)}
                onPointerEnter={future ? undefined : show(d, i)}
                // Everything the (aria-hidden) tooltip shows.
                aria-label={`${formatLongDay(d.day)}: Productive ${formatDuration(d.productiveSecs)}, Entertainment ${formatDuration(d.entertainmentSecs)}`}
                // The focus variant comes after today's resting outline, so it wins on that cell too.
                className={`aspect-square rounded-[3px] outline-offset-1 ${
                  future
                    ? "border border-border"
                    : `${HEAT[level(d.productiveSecs, heat.goalSecs)]} hover:brightness-125`
                } ${d.day === today ? "outline outline-1 outline-fg-3" : ""} focus-visible:outline-2 focus-visible:outline-fg-1`}
                style={{
                  gridRow: ((i + offset) % 7) + 2,
                  gridColumn: Math.floor((i + offset) / 7) + 2,
                }}
              />
            );
          })}
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
        </fieldset>
      </div>
    </div>
  );
}
