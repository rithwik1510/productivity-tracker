import { animate } from "motion/react";
import { useEffect, useRef, useState } from "react";
import type { Category, DayItem, Share } from "../api/bindings";
import { EASE_DRAWER, useMotionTokens } from "../lib/motion";
import { AppIcon } from "./AppIcon";
import { Duration, formatDuration } from "./Duration";

// viewBox units: the donut is drawn at 232 and scaled to its box (see Donut).
const SIZE = 232;
const STROKE = 16;
const R = (SIZE - STROKE) / 2;
const C = 2 * Math.PI * R;
const GAP = 2; // px of surface between arcs
const TOP_ITEMS = 8;

const COLOR: Record<Category, string> = {
  productive: "var(--productive)",
  entertainment: "var(--entertainment)",
};

type Slice = {
  key: string;
  label: string;
  suffix?: string;
  secs: number;
  color: string;
  category: Category | null;
  item?: DayItem;
};

const itemKey = (i: DayItem) => `${i.target.kind}:${i.target.id}:${i.category}`;

/**
 * Top items, grouped Productive then Entertainment, plus one neutral "N more" slice. Two hues
 * only: same-category arcs share a color, so the hue blocks read as the category split and the
 * 2px gaps as the items inside it.
 */
function slices(share: Share): Slice[] {
  const top = share.items.slice(0, TOP_ITEMS);
  const rest = share.items.slice(TOP_ITEMS);
  const byCategory = (c: Category) =>
    top
      .filter((i) => i.category === c)
      .map<Slice>((i) => ({
        key: itemKey(i),
        label: i.name,
        suffix: i.isBrowser ? "other sites" : undefined,
        secs: i.secs,
        color: COLOR[c],
        category: c,
        item: i,
      }));
  const more: Slice[] = rest.length
    ? [
        {
          key: "more",
          label: `${rest.length} more`,
          secs: rest.reduce((s, i) => s + i.secs, 0),
          color: "var(--fg-4)",
          category: null,
        },
      ]
    : [];
  return [...byCategory("productive"), ...byCategory("entertainment"), ...more];
}

function percent(secs: number, total: number) {
  if (total <= 0 || secs <= 0) return "0%";
  const p = (secs / total) * 100;
  return p < 1 ? "<1%" : `${Math.round(p)}%`;
}

export function Donut({ share, rangeLabel }: { share: Share; rangeLabel: string }) {
  const { reduce } = useMotionTokens();
  const total = share.productiveSecs + share.entertainmentSecs;
  const all = slices(share);
  const [hovered, setHovered] = useState<string | null>(null);
  const active = all.find((s) => s.key === hovered);

  // Arcs re-sweep when a new range's data lands. Never on mount (the toggle only crossfades), and
  // never on a refetch of the same range, which would replay it every few seconds.
  const [sweep, setSweep] = useState(1);
  const shown = useRef({ share, rangeLabel });
  useEffect(() => {
    const prev = shown.current;
    // Same object: still the previous range's data as a placeholder while the new one loads.
    if (prev.share === share) return;
    shown.current = { share, rangeLabel };
    if (prev.rangeLabel === rangeLabel) return;
    const controls = animate(0, 1, {
      duration: reduce ? 0 : 0.25,
      ease: EASE_DRAWER,
      onUpdate: setSweep,
    });
    return () => controls.stop();
  }, [share, rangeLabel, reduce]);

  let start = 0;
  const arcs = all.map((s) => {
    const len = (s.secs / total) * C;
    const arc = { ...s, start, len };
    start += len;
    return arc;
  });

  return (
    // Donut and legend are one composition, centered in the card: on a wide card the legend stops at
    // 480px rather than pushing each row's time far from its name.
    <div className="flex h-full items-center justify-center gap-10 px-8 py-5">
      {/* About 232px at the default size, from 208 up to 300px as the view column widens. */}
      <div
        role="img"
        aria-label={`${rangeLabel}: Productive ${formatDuration(share.productiveSecs)} (${percent(share.productiveSecs, total)}), Entertainment ${formatDuration(share.entertainmentSecs)} (${percent(share.entertainmentSecs, total)})`}
        className="relative size-[clamp(208px,26cqw,300px)] shrink-0"
      >
        <svg
          viewBox={`0 0 ${SIZE} ${SIZE}`}
          className="size-full -rotate-90 overflow-visible"
          aria-hidden="true"
        >
          <circle
            cx={SIZE / 2}
            cy={SIZE / 2}
            r={R}
            fill="none"
            stroke="var(--border-strong)"
            strokeWidth={STROKE}
          />
          {total > 0 &&
            arcs.map((a) => (
              <circle
                key={a.key}
                cx={SIZE / 2}
                cy={SIZE / 2}
                r={R}
                fill="none"
                stroke={a.color}
                strokeWidth={STROKE}
                // A lone arc is the whole ring and needs no gap.
                strokeDasharray={`${Math.max(0, (arcs.length > 1 ? a.len - GAP : a.len) * sweep)} ${C}`}
                strokeDashoffset={-a.start * sweep}
                className="transition-opacity duration-(--dur-fast) ease-standard"
                style={{
                  opacity: hovered && hovered !== a.key ? 0.3 : 1,
                  filter: a.category === "productive" ? "var(--productive-glow)" : undefined,
                }}
                onPointerEnter={() => setHovered(a.key)}
                onPointerLeave={() => setHovered(null)}
              />
            ))}
        </svg>
        <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center px-10 text-center">
          <p className="max-w-full truncate text-meta text-fg-2">
            {active ? active.label : "Total"}
          </p>
          <Duration secs={active ? active.secs : total} className="mt-1 text-stat" />
          <p className="mt-1.5 text-meta tabular-nums text-fg-3">
            {active ? percent(active.secs, total) : rangeLabel}
          </p>
        </div>
      </div>

      <div className="flex max-h-full min-w-0 max-w-[480px] flex-1 flex-col py-1">
        <dl className="grid grid-cols-2 gap-4 border-b border-border pb-4">
          {(["productive", "entertainment"] as const).map((c) => {
            const secs = c === "productive" ? share.productiveSecs : share.entertainmentSecs;
            return (
              <div key={c}>
                <dt className="flex items-center gap-2 text-meta text-fg-2">
                  <span className="size-2 rounded-[2px]" style={{ background: COLOR[c] }} />
                  {c === "productive" ? "Productive" : "Entertainment"}
                </dt>
                <dd className="mt-1 flex items-baseline gap-2 pl-4">
                  <Duration secs={secs} className="text-title font-medium" />
                  <span className="text-meta tabular-nums text-fg-3">{percent(secs, total)}</span>
                </dd>
              </div>
            );
          })}
        </dl>

        {total === 0 ? (
          <p className="pt-4 text-meta text-fg-3">No tracked time in this range yet.</p>
        ) : (
          <ul className="-mx-2 mt-2 min-h-0 overflow-y-auto" aria-label="Share by app and site">
            {all.map((s) => (
              <li
                key={s.key}
                onPointerEnter={() => setHovered(s.key)}
                onPointerLeave={() => setHovered(null)}
                className={`flex h-9 items-center gap-3 rounded-control px-2 transition-colors duration-(--dur-fast) ease-standard ${
                  hovered === s.key ? "bg-ghost-hover" : ""
                }`}
              >
                <span className="size-2 shrink-0 rounded-[2px]" style={{ background: s.color }} />
                {s.item ? (
                  <AppIcon iconPath={s.item.iconPath} name={s.item.name} />
                ) : (
                  <span className="size-6 shrink-0" />
                )}
                <span
                  className="min-w-0 flex-1 truncate text-fg-1"
                  title={s.suffix ? `${s.label} · ${s.suffix}` : s.label}
                >
                  {s.label}
                  {s.suffix && <span className="text-fg-3"> · {s.suffix}</span>}
                </span>
                <span className="w-10 text-right text-meta tabular-nums text-fg-3">
                  {percent(s.secs, total)}
                </span>
                <Duration secs={s.secs} className="w-16 text-right" />
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
