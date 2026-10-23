import { AnimatePresence, motion } from "motion/react";
import type { Category, ShareRange } from "../api/bindings";
import { useShare, useToday, useWeek, useYear } from "../api/queries";
import { ICON } from "../components/buttons";
import { Donut } from "../components/Donut";
import { Duration } from "../components/Duration";
import { GlassCard } from "../components/GlassCard";
import { ChevronLeft, ChevronRight } from "../components/Icons";
import { QuietError } from "../components/QuietError";
import { SegmentedControl } from "../components/SegmentedControl";
import { WindowTitle } from "../components/TitleBar";
import { WeekBars } from "../components/WeekBars";
import { YearHeatmap } from "../components/YearHeatmap";
import { addDays, formatWeekRange } from "../lib/day";
import { useFill, useMotionTokens } from "../lib/motion";
import { type HistoryChart, useUi } from "../state/ui";

const CHARTS = [
  { value: "bars", label: "Bars" },
  { value: "pie", label: "Pie" },
  { value: "heatmap", label: "Heatmap" },
] as const satisfies readonly { value: HistoryChart; label: string }[];

const RANGES = [
  { value: "week", label: "Week" },
  { value: "month", label: "Month" },
  { value: "all", label: "All time" },
] as const satisfies readonly { value: ShareRange; label: string }[];

const RANGE_LABEL: Record<ShareRange, string> = {
  week: "This week",
  month: "This month",
  all: "All time",
};

const openDay = (day: string) => useUi.setState({ view: "day", selectedDay: day });

// Records rather than ternaries, so a new HistoryChart fails to compile until it has both.
const CONTROLS: Record<HistoryChart, () => React.JSX.Element | null> = {
  bars: WeekPager,
  pie: RangePicker,
  heatmap: YearPager,
};

const PANELS: Record<HistoryChart, () => React.JSX.Element | null> = {
  bars: BarsPanel,
  pie: PiePanel,
  heatmap: HeatmapPanel,
};

export function HistoryView() {
  const chart = useUi((s) => s.historyChart);
  const { fade } = useMotionTokens();
  const Control = CONTROLS[chart];
  const Panel = PANELS[chart];

  return (
    // Fills the height up to --view-max-h, like the Day grid, so charts don't float in a tall card.
    <div className="flex h-full max-h-(--view-max-h) flex-col gap-4">
      <WindowTitle title="History" />
      <Totals />
      <GlassCard className="flex min-h-0 flex-1 flex-col">
        {/* The toggle belongs to the chart it toggles, so it sits in the chart card's header. */}
        <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border px-3 py-2.5">
          <SegmentedControl
            label="Chart"
            options={CHARTS}
            value={chart}
            onChange={(historyChart) => useUi.setState({ historyChart })}
            className="w-60"
          />
          <Control />
        </div>
        <div className="grid min-h-0 flex-1 grid-cols-1 grid-rows-1">
          {/* Both charts share one minmax(0, 1fr) cell during the 150ms opacity crossfade (no
              slide, no scale); the zero minimum keeps long legend names truncating instead of
              widening it. */}
          <AnimatePresence initial={false}>
            <motion.div
              key={chart}
              className="min-h-0 [grid-area:1/1]"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={fade}
            >
              <Panel />
            </motion.div>
          </AnimatePresence>
        </div>
      </GlassCard>
    </div>
  );
}

/** All-time totals, counting up once per window session (never again on a tab switch). */
function Totals() {
  const { data, error } = useShare("all");
  if (!data) return error ? <QuietError error={error} /> : <div className="h-[92px]" />;
  const total = data.productiveSecs + data.entertainmentSecs;
  return (
    <GlassCard className="grid grid-cols-2">
      <Stat category="productive" secs={data.productiveSecs} total={total} />
      <Stat category="entertainment" secs={data.entertainmentSecs} total={total} />
    </GlassCard>
  );
}

function Stat({ category, secs, total }: { category: Category; secs: number; total: number }) {
  const { value, counting } = useFill(secs, `history-${category}`, "all");
  const label = category === "productive" ? "Productive" : "Entertainment";
  return (
    <div className="px-5 py-4 not-first:border-l not-first:border-border">
      <p className="flex items-center gap-2 text-meta text-fg-2">
        <span
          className={`size-2 rounded-full ${category === "productive" ? "bg-productive" : "bg-entertainment"}`}
        />
        {label}, all time
      </p>
      <p className="mt-2 flex items-baseline gap-3">
        <Duration secs={counting ? Math.round(value) : secs} className="text-stat" />
        {total > 0 && (
          <span className="text-meta tabular-nums text-fg-3">
            {Math.round((secs / total) * 100)}% of tracked time
          </span>
        )}
      </p>
    </div>
  );
}

function Pager({
  label,
  onPrev,
  onNext,
  nextDisabled,
  unit,
}: {
  label: string;
  onPrev: () => void;
  onNext: () => void;
  nextDisabled: boolean;
  unit: string;
}) {
  return (
    <div className="flex items-center gap-1">
      <button
        type="button"
        aria-label={`Previous ${unit}`}
        title={`Previous ${unit}`}
        className={ICON}
        onClick={onPrev}
      >
        <ChevronLeft />
      </button>
      <span
        className="min-w-28 text-center text-meta font-medium tabular-nums text-fg-1"
        aria-live="polite"
      >
        {label}
      </span>
      <button
        type="button"
        aria-label={`Next ${unit}`}
        title={`Next ${unit}`}
        className={ICON}
        onClick={onNext}
        disabled={nextDisabled}
      >
        <ChevronRight />
      </button>
    </div>
  );
}

function WeekPager() {
  const { data } = useWeek(useUi((s) => s.weekStart));
  if (!data) return null;
  return (
    <Pager
      unit="week"
      label={data.isCurrent ? "This week" : formatWeekRange(data.start)}
      onPrev={() => useUi.setState({ weekStart: addDays(data.start, -7) })}
      onNext={() => useUi.setState({ weekStart: addDays(data.start, 7) })}
      nextDisabled={data.isCurrent}
    />
  );
}

function YearPager() {
  const { data } = useYear(useUi((s) => s.year));
  if (!data) return null;
  return (
    <Pager
      unit="year"
      label={String(data.year)}
      onPrev={() => useUi.setState({ year: data.year - 1 })}
      onNext={() => useUi.setState({ year: data.year + 1 })}
      nextDisabled={data.isCurrent}
    />
  );
}

function RangePicker() {
  const range = useUi((s) => s.shareRange);
  return (
    <SegmentedControl
      label="Range"
      options={RANGES}
      value={range}
      onChange={(shareRange) => useUi.setState({ shareRange })}
      className="w-60"
    />
  );
}

function BarsPanel() {
  const { data, error } = useWeek(useUi((s) => s.weekStart));
  const today = useToday();
  if (!data) return error ? <QuietError error={error} className="p-5" /> : null;
  return <WeekBars week={data} today={today} onOpenDay={openDay} />;
}

function PiePanel() {
  const range = useUi((s) => s.shareRange);
  const { data, error } = useShare(range);
  if (!data) return error ? <QuietError error={error} className="p-5" /> : null;
  return <Donut share={data} rangeLabel={RANGE_LABEL[range]} />;
}

function HeatmapPanel() {
  const { data, error } = useYear(useUi((s) => s.year));
  const today = useToday();
  if (!data) return error ? <QuietError error={error} className="p-5" /> : null;
  return <YearHeatmap key={data.year} heat={data} today={today} onOpenDay={openDay} />;
}
