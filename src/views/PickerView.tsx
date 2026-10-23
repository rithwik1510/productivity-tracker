import { AnimatePresence } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiError, Category } from "../api/bindings";
import { useUpsertSite } from "../api/queries";
import { type Target, usePickerTargets, useSetTargetCategory } from "../api/targets";
import { AddSiteTile } from "../components/AddSiteTile";
import { INPUT } from "../components/buttons";
import { CategoryPopover } from "../components/CategoryPopover";
import { ChevronDown, Search } from "../components/Icons";
import { QuietError } from "../components/QuietError";
import { AddTile, Tile } from "../components/Tile";
import { TileGrid } from "../components/TileGrid";
import { WindowTitle } from "../components/TitleBar";

export function PickerView() {
  const { apps, sites } = usePickerTargets();
  const tracked = [...(apps ?? []), ...sites].filter((t) => t.category).length;
  return (
    <>
      <WindowTitle title="Apps & sites" meta={apps ? `${tracked} tracked` : undefined} />
      <Picker
        mode={{ kind: "manage" }}
        hint="Tap a tile to change its category or stop tracking it. History is always kept."
      />
    </>
  );
}

/**
 * First run paints: each tap gives a tile the step's category, and tapping again clears it.
 * Apps & sites manages: a tap opens the tile's category popover.
 */
export type PickerMode =
  | { kind: "paint"; category: Category; onPaint: (key: string) => void }
  | { kind: "manage" };

/**
 * The domain the backend would store for typed input (scheme, www., port and path dropped,
 * lowercased), or null if it doesn't look like one; the backend still validates and reports errors.
 */
function siteDomain(input: string): string | null {
  const text = input.trim();
  try {
    const url = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(text) ? text : `https://${text}`);
    const host = url.hostname.replace(/\.$/, "").replace(/^www\./, "");
    return /^([a-z0-9-]+\.)+[a-z]{2,}$/.test(host) ? host : null;
  } catch {
    return null;
  }
}

// Tracked first in Apps & sites: Productive, then Entertainment, each by name.
const trackedOrder = (a: Target, b: Target) =>
  Number(a.category === "entertainment") - Number(b.category === "entertainment") ||
  a.name.localeCompare(b.name);

/**
 * The tile picker: search, then sections (Tracked · Running now · Sites · All apps), or one
 * Results section while searching. Typing a domain offers to add it as a site.
 */
export function Picker({
  mode,
  hint,
  autoFocus = false,
}: {
  mode: PickerMode;
  hint: string;
  autoFocus?: boolean;
}) {
  const { apps, sites, ready, error: loadError } = usePickerTargets();
  const setCategory = useSetTargetCategory();
  const add = useUpsertSite({ id: "sites" });
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState(false);
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [addInvalid, setAddInvalid] = useState(false);
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const closePopover = useCallback(() => setOpenKey(null), []);

  // Apps & sites freezes what was tracked on arrival, so a tile switched Off stays put (neutral)
  // until the screen is left, and nothing jumps under the pointer.
  const snapshot = useRef<string[] | null>(null);
  if (mode.kind === "manage" && snapshot.current === null && ready && apps) {
    snapshot.current = [...apps, ...sites]
      .filter((t) => t.category)
      .sort(trackedOrder)
      .map((t) => t.key);
  }
  const pinned = new Set(snapshot.current);
  const all = [...(apps ?? []), ...sites];

  // Moves focus to a tile once it has rendered: a just-added site, the first of "All apps".
  useEffect(() => {
    if (!focusKey) return;
    content.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(focusKey)}"]`)?.focus();
    setFocusKey(null);
  }, [focusKey]);

  const failed = (name: string) => (e: ApiError) =>
    setError(`Couldn't change ${name}. ${e.message}`);

  const set = (target: Target, category: Category | null) => {
    setError(null);
    setCategory(target, category, failed(target.name));
    if (mode.kind === "paint" && category) mode.onPaint(target.key);
  };

  const onTile = (target: Target) => {
    if (mode.kind === "paint")
      set(target, target.category === mode.category ? null : mode.category);
    else setOpenKey((open) => (open === target.key ? null : target.key));
  };

  // A site added while painting takes the step's category; in Apps & sites it opens at Off in
  // its popover. Typing one that exists just selects it.
  const addSite = (text: string, done: () => void) => {
    setError(null);
    const landed = (key: string) => {
      done();
      setQuery("");
      setAddInvalid(false);
      if (mode.kind === "manage") setOpenKey(key);
      else setFocusKey(key);
    };
    const domain = siteDomain(text);
    const existing = sites.find((s) => s.domain === domain);
    if (existing) {
      if (mode.kind === "paint" && existing.category !== mode.category)
        set(existing, mode.category);
      landed(existing.key);
      return;
    }
    add.mutate(
      { domain: text, keyword: null, category: mode.kind === "paint" ? mode.category : null },
      {
        onSuccess: (site) => {
          if (mode.kind === "paint") mode.onPaint(`site:${site.domain}`);
          landed(`site:${site.domain}`);
        },
        onError: (e) => {
          setError(e.message);
          setAddInvalid(true);
        },
      },
    );
  };

  const tile = (t: Target) => (
    <Tile
      key={t.key}
      target={t}
      onClick={() => onTile(t)}
      onSetCategory={(category) => {
        setOpenKey(null);
        set(t, category);
      }}
      pressed={mode.kind === "paint" ? t.category === mode.category : undefined}
      expanded={mode.kind === "manage" ? openKey === t.key : undefined}
    />
  );

  const needle = query.trim().toLowerCase();
  const offered = needle ? siteDomain(query) : null;
  const addable = offered !== null && !sites.some((s) => s.domain === offered);
  const running = (apps ?? []).filter((a) => a.app.isRunning && !pinned.has(a.key));
  const rest = (apps ?? []).filter((a) => !a.app.isRunning && !pinned.has(a.key));
  const siteTiles = sites.filter((s) => !pinned.has(s.key));
  const openTarget = all.find((t) => t.key === openKey);

  let body: React.JSX.Element;
  if (needle) {
    // Names and app identities, so "winword" finds Microsoft Word (winword.exe).
    const hits = all.filter((t) => t.search.includes(needle));
    const addKey = `add:${offered}`;
    body = (
      <Section title="Results" count={hits.length + (addable ? 1 : 0)}>
        {hits.length || addable ? (
          <TileGrid
            label="Results"
            keys={[...hits.map((t) => t.key), ...(addable ? [addKey] : [])]}
          >
            {hits.map(tile)}
            {addable && (
              <AddTile
                rovingKey={addKey}
                label={`Add ${offered}`}
                onClick={() => addSite(offered, () => {})}
              />
            )}
          </TileGrid>
        ) : (
          <Empty>
            No apps or sites match “{query.trim()}”. Don't see an app? Open it and come back.
          </Empty>
        )}
      </Section>
    );
  } else {
    const tracked = mode.kind === "manage" && snapshot.current ? snapshot.current : null;
    const pinnedTargets = (tracked ?? []).flatMap((key) => all.find((t) => t.key === key) ?? []);
    body = (
      <>
        {tracked && (
          <Section title="Tracked" count={tracked.length}>
            {pinnedTargets.length ? (
              <TileGrid label="Tracked" keys={pinnedTargets.map((t) => t.key)}>
                {pinnedTargets.map(tile)}
              </TileGrid>
            ) : (
              <Empty>Nothing is tracked yet. Tap an app below to start.</Empty>
            )}
          </Section>
        )}
        {!apps && !loadError && <Empty>Looking for your apps…</Empty>}
        {running.length > 0 && (
          <Section title="Running now" why="open on this PC right now">
            <TileGrid label="Running now" keys={running.map((t) => t.key)}>
              {running.map(tile)}
            </TileGrid>
          </Section>
        )}
        <Section title="Sites" why="a site keeps its own category, whatever the browser's">
          <TileGrid label="Sites" keys={[...siteTiles.map((t) => t.key), "add-site"]}>
            {siteTiles.map(tile)}
            <AddSiteTile onAdd={addSite} invalid={addInvalid} />
          </TileGrid>
        </Section>
        {apps && rest.length > 0 && (
          <Section title="All apps" count={rest.length} why="from the Start Menu">
            {expanded ? (
              <TileGrid label="All apps" keys={rest.map((t) => t.key)}>
                {rest.map(tile)}
              </TileGrid>
            ) : (
              <button
                type="button"
                onClick={() => {
                  setExpanded(true);
                  setFocusKey(rest[0]?.key ?? null);
                }}
                className="flex h-9 w-full items-center gap-2 rounded-control border border-dashed border-border-strong px-3 text-meta text-fg-2 transition-colors duration-(--dur-fast) ease-standard hover:bg-ghost-hover hover:text-fg-1"
              >
                Show all {rest.length} installed apps
                <span className="text-fg-3">· or search above</span>
                <span className="ml-auto">
                  <ChevronDown />
                </span>
              </button>
            )}
          </Section>
        )}
      </>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-3.5">
      <div className="flex shrink-0 items-center gap-3.5">
        <label className="relative w-[300px] shrink-0">
          <span className="pointer-events-none absolute inset-y-0 left-2.75 grid place-items-center text-fg-3">
            <Search />
          </span>
          <input
            type="search"
            // biome-ignore lint/a11y/noAutofocus: first run opens on this field; it is the screen's first control.
            autoFocus={autoFocus}
            aria-label="Search apps and sites"
            placeholder={apps ? `Search ${all.length} apps and sites` : "Search apps and sites"}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setOpenKey(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") setQuery("");
            }}
            autoComplete="off"
            className={`${INPUT} h-8.5 w-full pl-8.5 text-[13px]`}
          />
        </label>
        {error ? (
          <p role="alert" className="min-w-0 truncate text-meta text-fg-1" title={error}>
            {error}
          </p>
        ) : (
          <p className="min-w-0 truncate text-meta text-fg-3" title={hint}>
            {hint}
          </p>
        )}
      </div>
      {/* The 12px scrollbar sits in the right margin, beside the tiles rather than over them. */}
      <div
        ref={viewport}
        className="-mr-3 min-h-0 flex-1 overflow-y-auto pb-2 [scrollbar-gutter:stable]"
      >
        <div ref={content} className="relative">
          {loadError ? <QuietError error={loadError} /> : body}
          <AnimatePresence>
            {openTarget && (
              <CategoryPopover
                key={openTarget.key}
                target={openTarget}
                running={openTarget.kind === "app" && openTarget.app.isRunning}
                container={content}
                viewport={viewport}
                onCategory={(category) => set(openTarget, category)}
                onClose={closePopover}
              />
            )}
          </AnimatePresence>
        </div>
      </div>
    </div>
  );
}

function Section({
  title,
  count,
  why,
  children,
}: {
  title: string;
  count?: number;
  why?: string;
  children: React.ReactNode;
}) {
  return (
    <section aria-label={title} className="mb-4.5 last:mb-0">
      <h2 className="mb-2 flex items-baseline gap-2 text-meta font-medium text-fg-2">
        {title}
        {count !== undefined && <span className="font-normal tabular-nums text-fg-3">{count}</span>}
        {why && <span className="ml-auto truncate font-normal text-fg-3">{why}</span>}
      </h2>
      {children}
    </section>
  );
}

const Empty = ({ children }: { children: React.ReactNode }) => (
  <p className="px-1 py-4.5 text-meta text-fg-3">{children}</p>
);
