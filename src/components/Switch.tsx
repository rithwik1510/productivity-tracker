/** An on/off switch: the inverted primary fill when on, a surface-2 track with a hairline when off. */
export function Switch({
  label,
  checked,
  onChange,
}: {
  /** Accessible name. */
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-10 shrink-0 rounded-pill border transition-colors duration-(--dur-fast) ease-standard ${
        checked ? "border-transparent bg-button-primary" : "border-border-strong bg-surface-2"
      }`}
    >
      <span
        className={`absolute top-1/2 left-0.5 size-4.5 -translate-y-1/2 rounded-full transition-[translate,background-color] duration-(--dur-fast) ease-standard ${
          checked ? "translate-x-4 bg-button-primary-fg" : "bg-fg-2"
        }`}
      />
    </button>
  );
}
