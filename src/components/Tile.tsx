import type { KeyboardEvent } from "react";
import type { Category } from "../api/bindings";
import type { Target } from "../api/targets";
import { AppIcon } from "./AppIcon";
import { Check, Plus } from "./Icons";
import { useRovingTabIndex } from "./TileGrid";

/** The 40px tile shell, shared with the add-site tiles. Press feedback is the buttons' 0.97. */
export const TILE =
  "relative flex h-10 min-w-0 items-center gap-2.5 rounded-segment border pr-2.5 pl-2 text-left transition-[background-color,border-color,color,scale] duration-(--dur-fast) ease-standard active:scale-[0.97]";

const LOOK: Record<Category | "off", string> = {
  off: "border-border bg-surface-1 hover:border-border-strong hover:bg-surface-2",
  productive: "border-productive/45 bg-productive/14 hover:bg-productive/20",
  entertainment: "border-entertainment/55 bg-entertainment/20 hover:bg-entertainment/27",
};

export const CATEGORY_LABEL: Record<Category | "off", string> = {
  off: "Not tracked",
  productive: "Productive",
  entertainment: "Entertainment",
};

// Direct keys, in both picker modes: P and E set (or, pressed again, clear) a category.
const DIRECT: Record<string, Category | null> = {
  p: "productive",
  e: "entertainment",
  backspace: null,
  delete: null,
};

type Props = {
  target: Target;
  onClick: () => void;
  /** P / E / Backspace on the focused tile. */
  onSetCategory: (category: Category | null) => void;
  /** First run: whether the tile already has this step's category. */
  pressed?: boolean;
  /** Apps & sites: the tile opens a category popover. */
  expanded?: boolean;
};

/**
 * One app or site: icon, name, and the category as a tint plus a check badge on the icon's
 * corner, so color is never the only signal.
 */
export function Tile({ target, onClick, onSetCategory, pressed, expanded }: Props) {
  const tabIndex = useRovingTabIndex(target.key);
  const { category } = target;
  const browser = target.kind === "app" && target.app.isBrowser;

  const onKeyDown = (e: KeyboardEvent) => {
    const key = e.key.toLowerCase();
    if (e.ctrlKey || e.altKey || e.metaKey || !(key in DIRECT)) return;
    e.preventDefault();
    const next = DIRECT[key] ?? null;
    onSetCategory(next !== null && next === category ? null : next);
  };

  return (
    <button
      type="button"
      data-key={target.key}
      tabIndex={tabIndex}
      onClick={onClick}
      onKeyDown={onKeyDown}
      aria-label={`${target.name}, ${CATEGORY_LABEL[category ?? "off"]}${browser ? ", browser" : ""}`}
      aria-pressed={pressed}
      aria-haspopup={expanded === undefined ? undefined : "dialog"}
      aria-expanded={expanded}
      title={target.name}
      className={`${TILE} ${LOOK[category ?? "off"]} text-fg-1 shadow-glass`}
    >
      <AppIcon iconPath={target.iconPath} name={target.name} />
      <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{target.name}</span>
      <CheckBadge category={category} />
    </button>
  );
}

function CheckBadge({ category }: { category: Category | null }) {
  return (
    <span
      aria-hidden="true"
      className={`absolute top-[23px] left-6 grid size-[13px] place-items-center rounded-full text-fg-1 shadow-[0_0_0_2px_var(--surface-3)] transition-[opacity,scale,background-color] duration-(--dur-fast) ease-spring ${
        category === null
          ? "scale-50 bg-productive opacity-0"
          : category === "productive"
            ? "bg-productive"
            : "bg-[oklch(0.62_0.05_75)]"
      }`}
    >
      <Check width={10} height={10} strokeWidth={2.5} />
    </span>
  );
}

/** A dashed tile for adding: "Add a site…" or a search result's "Add hn.algolia.com". */
export function AddTile({
  rovingKey,
  label,
  onClick,
}: {
  rovingKey: string;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      data-key={rovingKey}
      tabIndex={useRovingTabIndex(rovingKey)}
      onClick={onClick}
      className={`${TILE} border-dashed border-border text-fg-2 hover:bg-ghost-hover hover:text-fg-1`}
    >
      <span className="grid size-6 shrink-0 place-items-center text-fg-3">
        <Plus />
      </span>
      <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{label}</span>
    </button>
  );
}
