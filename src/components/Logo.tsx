// Placeholder app mark: a static ring echoing the goal ring. Swap the SVG here for the real logo.
const R = 8.75;
const C = 2 * Math.PI * R;

export function Logo() {
  return (
    <svg viewBox="0 0 20 20" className="size-5.5 overflow-visible" aria-hidden="true">
      <circle cx="10" cy="10" r={R} fill="none" stroke="var(--border-strong)" strokeWidth="2.5" />
      <circle
        cx="10"
        cy="10"
        r={R}
        fill="none"
        stroke="var(--productive)"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeDasharray={C}
        strokeDashoffset={C * 0.3}
        transform="rotate(-90 10 10)"
        style={{ filter: "drop-shadow(0 0 6px oklch(0.7 0.16 258 / 45%))" }}
      />
    </svg>
  );
}
