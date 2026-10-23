import { Fragment } from "react";

/** [number, unit] pairs: 0m · <1m · 42m · 5h 07m · 1,234h (from 100 h up, minutes stop mattering). */
function durationParts(secs: number): [string, string][] {
  if (secs <= 0) return [["0", "m"]];
  if (secs < 60) return [["<1", "m"]];
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h >= 100) return [[h.toLocaleString("en-US"), "h"]];
  if (h === 0) return [[String(m), "m"]];
  // Padded minutes keep the width steady while a count-up crosses 5h 09m → 5h 10m.
  return [
    [String(h), "h"],
    [String(m).padStart(2, "0"), "m"],
  ];
}

/** Plain text for aria-labels and titles. */
export const formatDuration = (secs: number) =>
  durationParts(secs)
    .map(([n, u]) => n + u)
    .join(" ");

/** Goal values read as 6h · 6h 30m · 30m: they are always whole half-hours, so nothing is padded. */
function goalParts(minutes: number): [string, string][] {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h === 0) return [[String(m), "m"]];
  return m === 0
    ? [[String(h), "h"]]
    : [
        [String(h), "h"],
        [String(m), "m"],
      ];
}

export const formatGoal = (minutes: number) =>
  goalParts(minutes)
    .map(([n, u]) => n + u)
    .join(" ");

/**
 * A time value in tabular figures with units in --fg-2 at half the number's size. The unit floor
 * of 11px keeps row-sized values legible (half of 14px would be unreadable). `goal` switches to
 * the goal format (6h, not 6h 00m).
 */
export function Duration({
  secs,
  goal = false,
  className = "",
}: {
  secs: number;
  goal?: boolean;
  className?: string;
}) {
  const parts = goal ? goalParts(Math.round(secs / 60)) : durationParts(secs);
  return (
    <span className={`whitespace-nowrap tabular-nums ${className}`}>
      {parts.map(([n, u], i) => (
        <Fragment key={u}>
          {i > 0 && " "}
          {n}
          <span className="ml-[0.08em] text-[length:max(0.5em,11px)] font-normal text-fg-2 tracking-normal">
            {u}
          </span>
        </Fragment>
      ))}
    </span>
  );
}
