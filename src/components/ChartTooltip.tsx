import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import { useMotionTokens } from "../lib/motion";
import { Duration } from "./Duration";

export type TooltipAnchor = {
  /** px from the chart container's left edge */
  x: number;
  /** px from its top; the tooltip sits above this point */
  y: number;
  /** Which edge of the tooltip lines up with x, so it never leaves the chart near an edge. */
  align: "start" | "center" | "end";
};

const ALIGN = { start: "-8%", center: "-50%", end: "-92%" } as const;

/** Popover-styled readout for a hovered or focused mark: blur, hairline, fade + 4px rise. */
export function ChartTooltip({
  anchor,
  title,
  children,
}: {
  anchor: TooltipAnchor | null;
  title: string;
  children: ReactNode;
}) {
  const { popoverIn, popoverOut } = useMotionTokens();
  return (
    <AnimatePresence>
      {anchor && (
        <div
          className="pointer-events-none absolute z-20"
          style={{
            left: anchor.x,
            top: anchor.y,
            transform: `translate(${ALIGN[anchor.align]}, calc(-100% - 8px))`,
          }}
        >
          {/* Hidden from assistive tech: the focused mark's aria-label already says all of this. */}
          <motion.div
            aria-hidden="true"
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0, transition: popoverIn }}
            exit={{ opacity: 0, y: 4, transition: popoverOut }}
            className="min-w-40 rounded-card border border-border-strong bg-surface-3 px-3 py-2.5 shadow-popover [backdrop-filter:var(--surface-3-backdrop)]"
          >
            <p className="mb-1.5 text-meta whitespace-nowrap text-fg-2">{title}</p>
            <div className="flex flex-col gap-1">{children}</div>
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
}

/** Value first, label second; a short line key in the series color carries identity. */
export function TooltipRow({ color, secs, label }: { color: string; secs: number; label: string }) {
  return (
    <p className="flex items-center gap-2 text-meta whitespace-nowrap">
      <span className="h-0.5 w-3 rounded-full" style={{ background: color }} />
      <Duration secs={secs} className="font-medium text-fg-1" />
      <span className="text-fg-2">{label}</span>
    </p>
  );
}
