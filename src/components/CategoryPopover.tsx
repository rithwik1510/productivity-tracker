import { motion } from "motion/react";
import { type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Category } from "../api/bindings";
import { useUpsertSite } from "../api/queries";
import type { Target } from "../api/targets";
import { useMotionTokens } from "../lib/motion";
import { AppIcon } from "./AppIcon";
import { INPUT } from "./buttons";
import { CategoryControl } from "./CategoryControl";

const WIDTH = 312;
const GAP = 6;
const EDGE = 12;

type Props = {
  target: Target;
  /** True when the app is open on this PC now; shown as the hint line. */
  running: boolean;
  /** The positioned element the tiles scroll inside, and its scrolling viewport. */
  container: RefObject<HTMLElement | null>;
  viewport: RefObject<HTMLElement | null>;
  onCategory: (category: Category | null) => void;
  /** Must be stable: the outside-press listener depends on it. */
  onClose: () => void;
};

/**
 * Apps & sites: one tile's category (the Off · Productive · Entertainment control) and, for a
 * site, its title keyword. Anchored under the tile, flipped above it near the bottom; it scrolls
 * with the tiles. Picking an app's category closes it (a site stays open for its keyword); so do
 * Escape and a press outside.
 */
export function CategoryPopover({
  target,
  running,
  container,
  viewport,
  onCategory,
  onClose,
}: Props) {
  const { popoverIn, popoverOut } = useMotionTokens();
  const ref = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const tile = () =>
    container.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(target.key)}"]`);
  const close = () => {
    tile()?.focus();
    onClose();
  };

  // Measured before paint, so it never shows at the wrong spot.
  useLayoutEffect(() => {
    const box = container.current?.getBoundingClientRect();
    const view = viewport.current?.getBoundingClientRect();
    const anchor = container.current
      ?.querySelector(`[data-key="${CSS.escape(target.key)}"]`)
      ?.getBoundingClientRect();
    const height = ref.current?.offsetHeight ?? 0;
    if (!box || !view || !anchor) return;
    const below = anchor.bottom + GAP + height <= view.bottom - EDGE;
    setAt({
      x: Math.max(0, Math.min(anchor.left - box.left, box.width - WIDTH)),
      y: below ? anchor.bottom - box.top + GAP : anchor.top - box.top - GAP - height,
    });
  }, [container, viewport, target.key]);

  // Focus moves in once placed: a hidden element can't take focus.
  const placed = at !== null;
  useLayoutEffect(() => {
    if (placed)
      ref.current?.querySelector<HTMLElement>('[role="radio"][aria-checked="true"]')?.focus();
  }, [placed]);

  useEffect(() => {
    const outside = (e: PointerEvent) => {
      // Tiles toggle the popover themselves.
      if (e.target instanceof Element && !e.target.closest("[data-popover], [data-key]")) onClose();
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [onClose]);

  const hint =
    target.kind === "site"
      ? "Site"
      : target.app.isBrowser
        ? "Browser · sites you add override this"
        : running
          ? "Running now"
          : undefined;

  return (
    <motion.div
      ref={ref}
      role="dialog"
      aria-label={target.name}
      data-popover=""
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          close();
        }
      }}
      className="absolute z-40 rounded-card border border-border-strong bg-surface-3 p-3 shadow-popover backdrop-blur-[24px] backdrop-saturate-[1.4]"
      style={{
        width: WIDTH,
        left: at?.x ?? 0,
        top: at?.y ?? 0,
        visibility: at ? "visible" : "hidden",
      }}
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0, transition: popoverIn }}
      exit={{ opacity: 0, y: 4, transition: popoverOut }}
    >
      <div className="mb-2.5 flex items-center gap-2.5">
        <AppIcon iconPath={target.iconPath} name={target.name} size={32} />
        <p className="min-w-0 flex-1">
          <span className="block truncate font-medium" title={target.name}>
            {target.name}
          </span>
          {hint && <span className="block text-meta text-fg-3">{hint}</span>}
        </p>
      </div>
      <CategoryControl
        name={target.name}
        value={target.category}
        onChange={(category) => {
          onCategory(category);
          if (target.kind === "app") close();
        }}
      />
      {target.kind === "site" && (
        <KeywordField
          key={target.key}
          domain={target.domain}
          keyword={target.keyword}
          category={target.category}
          onDone={close}
        />
      )}
    </motion.div>
  );
}

/**
 * The site's title keyword: commits on Enter (then closes) or blur; Escape drops the edit and
 * closes.
 * A refused keyword keeps the field open with the backend's reason.
 */
function KeywordField({
  domain,
  keyword,
  category,
  onDone,
}: {
  domain: string;
  keyword: string | null;
  category: Category | null;
  onDone: () => void;
}) {
  const upsert = useUpsertSite({ id: "sites" });
  const [draft, setDraft] = useState(keyword ?? "");
  // Set once Enter or Escape has settled the field, so the blur that closing causes is ignored.
  const settled = useRef(false);

  const commit = (close: boolean) => {
    const finish = () => {
      if (!close) return;
      settled.current = true;
      onDone();
    };
    const next = draft.trim();
    if (next === (keyword ?? "")) {
      upsert.reset();
      finish();
      return;
    }
    upsert.mutate(
      { domain, keyword: next, category },
      {
        onSuccess: (saved) => {
          setDraft(saved.keyword ?? "");
          finish();
        },
      },
    );
  };

  return (
    <>
      <label className="mt-2.5 flex items-center gap-2.5 text-meta text-fg-3">
        Title keyword
        <input
          aria-label={`Title keyword for ${domain}`}
          spellCheck={false}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            if (!settled.current) commit(false);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              // Without this, the keypress lands on the tile that focus returns to and reopens it.
              e.preventDefault();
              commit(true);
            }
            // The dialog's own Escape handler closes it after this.
            if (e.key === "Escape") settled.current = true;
          }}
          aria-invalid={upsert.isError}
          className={`${INPUT} h-7.5 min-w-0 flex-1 text-meta ${upsert.isError ? "border-border-focus" : ""}`}
        />
      </label>
      {upsert.error ? (
        <p role="alert" className="mt-2 text-meta text-fg-1">
          {upsert.error.message}
        </p>
      ) : (
        <p className="mt-2 text-meta leading-[1.4] text-fg-3">
          Matched as a whole word in the window title when the browser can't report the address
          (fullscreen video, for example).
        </p>
      )}
    </>
  );
}
