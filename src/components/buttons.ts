// Button looks from the design reference, as class strings so any element can wear them.
// Press feedback is T3's scale(0.97); transitions are color-only plus that press.
const BASE =
  "inline-flex items-center justify-center gap-2 rounded-control font-medium whitespace-nowrap transition-[color,background-color,border-color,scale,opacity] duration-(--dur-fast) ease-standard active:scale-[0.97] disabled:pointer-events-none disabled:opacity-40";

/** Inverted primary: at most one per screen. Its focus ring sits outside: inset, it vanishes on white. */
export const PRIMARY = `${BASE} h-9 bg-button-primary px-4 text-button-primary-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-fg-1`;

/** The one destructive confirm ("Delete everything"): Windows' Close red, used only in its dialog. */
export const DANGER = `${BASE} h-8 bg-danger px-3 text-meta text-fg-1 hover:opacity-90 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-fg-1`;

/** Hairline secondary for real actions that aren't the screen's main one. */
export const SECONDARY = `${BASE} h-8 border border-border-strong px-3 text-meta text-fg-1 hover:bg-ghost-hover`;

// Sizes are separate from looks: two utilities for one property don't override by class order.
const GHOST_LOOK = `${BASE} px-3 text-meta text-fg-2 hover:bg-ghost-hover hover:text-fg-1`;
const ICON_LOOK = `${BASE} text-fg-2 hover:bg-ghost-hover hover:text-fg-1`;

export const GHOST = `${GHOST_LOOK} h-8`;
export const ICON = `${ICON_LOOK} size-8`;

/** Controls beside a title in the 40px titlebar are 28px, so they clear its edges. */
export const TITLEBAR_GHOST = `${GHOST_LOOK} h-7`;
export const TITLEBAR_ICON = `${ICON_LOOK} size-7`;

// Focused inputs wear the global 2px ring, laid over the hairline border. Callers set the height
// and text size.
export const INPUT =
  "rounded-control border border-border-strong bg-surface-1 px-3 text-fg-1 placeholder:text-fg-3 transition-colors duration-(--dur-fast) ease-standard -outline-offset-1";

/** A toggle chip (quick goals): pressed reads as a surface-2 fill, never the accent. */
export const CHIP = `${BASE} h-7 border border-border-strong px-3 text-meta text-fg-2 hover:bg-ghost-hover hover:text-fg-1 aria-pressed:bg-surface-2 aria-pressed:text-fg-1`;
