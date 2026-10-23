import { useState } from "react";
import { Check } from "./Icons";
import { AddTile, TILE } from "./Tile";
import { useRovingTabIndex } from "./TileGrid";

const KEY = "add-site";

/**
 * The dashed "Add a site…" tile, which turns into an inline domain field in place. `onAdd` gets
 * the typed text and calls `done` once the site exists, which closes the field; a refused domain
 * keeps it open, marked invalid, for a fix.
 */
export function AddSiteTile({
  onAdd,
  invalid,
}: {
  onAdd: (text: string, done: () => void) => void;
  invalid: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const tabIndex = useRovingTabIndex(KEY);
  const close = () => {
    setOpen(false);
    setDraft("");
  };

  if (!open) return <AddTile rovingKey={KEY} label="Add a site…" onClick={() => setOpen(true)} />;
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (draft.trim()) onAdd(draft, close);
      }}
      // Leaving an empty field puts the tile back; a typed domain stays for another try.
      onBlur={(e) => {
        if (!draft.trim() && !e.currentTarget.contains(e.relatedTarget)) close();
      }}
      className={`${TILE} border-border-focus bg-surface-1 pr-1 pl-2 active:scale-100`}
    >
      <input
        // biome-ignore lint/a11y/noAutofocus: the field appears because the user just asked for it.
        autoFocus
        data-key={KEY}
        tabIndex={tabIndex}
        aria-label="Site address"
        aria-invalid={invalid}
        placeholder="youtube.com"
        spellCheck={false}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
        }}
        className="h-7.5 min-w-0 flex-1 bg-transparent text-[13px] text-fg-1 outline-none placeholder:text-fg-3"
      />
      <button
        type="submit"
        aria-label="Add site"
        className="grid size-7 shrink-0 place-items-center rounded-[6px] text-fg-2 hover:bg-ghost-hover hover:text-fg-1"
      >
        <Check />
      </button>
    </form>
  );
}
