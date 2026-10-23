export type IconSize = 18 | 24 | 32;

/** Box, corner and letter size per icon size; shared with AppIcon so both stay in step. */
export const ICON_BOX: Record<IconSize, string> = {
  18: "size-4.5 rounded-[5px] text-[10px]",
  24: "size-6 rounded-[6px] text-meta",
  32: "size-8 rounded-control text-[15px]",
};

/** Stand-in for a missing app icon, and the only "icon" a site gets (no network, ever). */
export function Monogram({ name, size = 24 }: { name: string; size?: IconSize }) {
  const letter = name.match(/[\p{L}\p{N}]/u)?.[0]?.toUpperCase() ?? "?";
  return (
    <span
      aria-hidden="true"
      className={`grid shrink-0 place-items-center border border-border-strong bg-surface-2 font-medium text-fg-2 ${ICON_BOX[size]}`}
    >
      {letter}
    </span>
  );
}
