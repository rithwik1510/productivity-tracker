import {
  createContext,
  type KeyboardEvent,
  type ReactNode,
  useContext,
  useRef,
  useState,
} from "react";

const ActiveKey = createContext<string | undefined>(undefined);

/** A tile's tabIndex: only the grid's active tile is a tab stop. */
export const useRovingTabIndex = (key: string) => (useContext(ActiveKey) === key ? 0 : -1);

/**
 * A tile section's grid (auto-fill, 160px minimum, 8px gaps). It is one tab stop: arrows move
 * across and down the rendered columns, Home/End jump to the ends. `keys` lists the tiles in
 * order, each rendered with a matching `data-key` and `useRovingTabIndex`.
 */
export function TileGrid({
  label,
  keys,
  children,
}: {
  label: string;
  keys: readonly string[];
  children: ReactNode;
}) {
  const ref = useRef<HTMLFieldSetElement>(null);
  const [focused, setFocused] = useState<string>();
  // A tile that left the grid (search, a new section) hands the tab stop back to the first.
  const active = focused !== undefined && keys.includes(focused) ? focused : keys[0];

  const onKeyDown = (e: KeyboardEvent) => {
    const grid = ref.current;
    const at = active === undefined ? -1 : keys.indexOf(active);
    // The add-site field lives in the grid; its arrows move the caret, not the focus.
    if (!grid || at < 0 || e.target instanceof HTMLInputElement) return;
    const cols = getComputedStyle(grid).gridTemplateColumns.split(" ").length;
    const step: Record<string, number> = {
      ArrowRight: at + 1,
      ArrowLeft: at - 1,
      ArrowDown: at + cols,
      ArrowUp: at - cols,
      Home: 0,
      End: keys.length - 1,
    };
    const next = keys[Math.min(Math.max(step[e.key] ?? -1, 0), keys.length - 1)];
    if (!(e.key in step) || next === undefined) return;
    e.preventDefault();
    setFocused(next);
    grid.querySelector<HTMLElement>(`[data-key="${CSS.escape(next)}"]`)?.focus();
  };

  return (
    <ActiveKey value={active}>
      <fieldset
        ref={ref}
        aria-label={label}
        onKeyDown={onKeyDown}
        onFocus={(e) => {
          const key = e.target instanceof HTMLElement ? e.target.closest("[data-key]") : null;
          if (key instanceof HTMLElement && key.dataset.key) setFocused(key.dataset.key);
        }}
        className="grid min-w-0 grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2"
      >
        {children}
      </fieldset>
    </ActiveKey>
  );
}
