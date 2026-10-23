import { motion } from "motion/react";
import { type KeyboardEvent, useRef } from "react";
import { useMotionTokens } from "../lib/motion";

type Option<T extends string> = { value: T; label: string };

type Props<T extends string> = {
  /** Accessible name of the group. */
  label: string;
  options: readonly Option<T>[];
  value: T;
  onChange: (value: T) => void;
  /** Thumb fill and hairline for the selected value. */
  thumbClass?: (value: T) => string;
  className?: string;
};

export const NEUTRAL_THUMB = "border-border-strong bg-thumb";

// Radio-group keys: arrows wrap around, Home/End jump to the ends.
const STEP: Record<string, (i: number, n: number) => number> = {
  ArrowLeft: (i, n) => (i - 1 + n) % n,
  ArrowUp: (i, n) => (i - 1 + n) % n,
  ArrowRight: (i, n) => (i + 1) % n,
  ArrowDown: (i, n) => (i + 1) % n,
  Home: () => 0,
  End: (_, n) => n - 1,
};

/**
 * T3's segmented toggle: a surface-2 track with a sliding thumb. Segments share one width, so the
 * thumb moves as a transform on the reference spring; nothing is measured, which keeps dozens of
 * these cheap inside a scrolling list.
 *
 * Semantically a radio group: only the selected segment is a tab stop, and arrows move and select,
 * so a list of these (the Apps picker) costs one tab stop per row.
 */
export function SegmentedControl<T extends string>({
  label,
  options,
  value,
  onChange,
  thumbClass = () => NEUTRAL_THUMB,
  className = "",
}: Props<T>) {
  const { spring } = useMotionTokens();
  const segments = useRef<(HTMLButtonElement | null)[]>([]);
  const index = Math.max(
    0,
    options.findIndex((o) => o.value === value),
  );

  const onKeyDown = (e: KeyboardEvent) => {
    const next = STEP[e.key]?.(index, options.length);
    if (next === undefined) return;
    e.preventDefault();
    segments.current[next]?.focus();
    const option = options[next];
    if (option && option.value !== value) onChange(option.value);
  };

  return (
    <div
      role="radiogroup"
      aria-label={label}
      onKeyDown={onKeyDown}
      className={`relative grid min-w-0 auto-cols-fr grid-flow-col rounded-segment bg-surface-2 p-0.5 ${className}`}
    >
      <motion.span
        aria-hidden="true"
        className={`absolute inset-y-0.5 left-0.5 rounded-control border transition-colors duration-(--dur-fast) ease-standard ${thumbClass(value)}`}
        style={{ width: `calc((100% - 4px) / ${options.length})` }}
        initial={false}
        animate={{ x: `${index * 100}%` }}
        transition={spring}
      />
      {options.map((o, i) => (
        // biome-ignore lint/a11y/useSemanticElements: a button segment under the shared sliding thumb; the radio keyboard model is implemented above.
        <button
          key={o.value}
          ref={(el) => {
            segments.current[i] = el;
          }}
          type="button"
          role="radio"
          aria-checked={i === index}
          tabIndex={i === index ? 0 : -1}
          onClick={() => onChange(o.value)}
          className={`relative h-7 rounded-control px-3 text-meta font-medium whitespace-nowrap transition-colors duration-(--dur-fast) ease-standard ${
            i === index ? "text-fg-1" : "text-fg-2 hover:text-fg-1"
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
