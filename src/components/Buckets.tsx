import { AnimatePresence, motion } from "motion/react";
import type { Category } from "../api/bindings";
import type { Target } from "../api/targets";
import { useMotionTokens } from "../lib/motion";
import { AppIcon } from "./AppIcon";
import { CATEGORY_LABEL } from "./Tile";

const SHOWN = 7;

const ACTIVE: Record<Category, string> = {
  productive: "border-productive/45",
  entertainment: "border-entertainment/55",
};

/**
 * First run's footer: what has been picked so far, per category. Each pick drops its icon in
 * (the one reward moment, and it carries real information); the current step's bucket is outlined.
 */
export function Buckets({ tracked, active }: { tracked: readonly Target[]; active: Category }) {
  return (
    <>
      <Bucket
        category="productive"
        items={tracked.filter((t) => t.category === "productive")}
        active={active === "productive"}
      />
      <Bucket
        category="entertainment"
        items={tracked.filter((t) => t.category === "entertainment")}
        active={active === "entertainment"}
      />
    </>
  );
}

function Bucket({
  category,
  items,
  active,
}: {
  category: Category;
  items: readonly Target[];
  active: boolean;
}) {
  const { pop } = useMotionTokens();
  const shown = items.slice(-SHOWN);
  const more = items.length - shown.length;
  return (
    <output
      aria-live="polite"
      aria-label={`${CATEGORY_LABEL[category]}: ${items.length}`}
      className={`flex h-10 min-w-[200px] items-center gap-2.5 rounded-segment border bg-surface-1 px-3 transition-colors duration-(--dur-fast) ease-standard ${
        active ? ACTIVE[category] : "border-border"
      }`}
    >
      <span
        className={`size-2 shrink-0 rounded-full ${category === "productive" ? "bg-productive" : "bg-entertainment"}`}
      />
      <span className="text-meta whitespace-nowrap text-fg-2">{CATEGORY_LABEL[category]}</span>
      <span className="flex min-w-0 items-center gap-1" aria-hidden="true">
        {/* Only icons that arrive after the bucket appears pop in. */}
        <AnimatePresence initial={false}>
          {shown.map((t) => (
            <motion.span
              key={t.key}
              initial={{ scale: 0.5, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              transition={pop}
            >
              <AppIcon iconPath={t.iconPath} name={t.name} size={18} />
            </motion.span>
          ))}
        </AnimatePresence>
        {more > 0 && <span className="text-meta tabular-nums text-fg-3">+{more}</span>}
      </span>
      <span
        className={`ml-auto min-w-[1.2em] text-right tabular-nums ${
          items.length ? "font-medium text-fg-1" : "text-fg-3"
        }`}
      >
        {items.length}
      </span>
    </output>
  );
}
