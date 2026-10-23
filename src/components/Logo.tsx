import { useId } from "react";

// The app mark without its tile (docs/design/reference.md, "The mark"): the app's surfaces are
// already dark, and the tile's tinted fill read as a muddy square on the rail. Same paths as
// src-tauri/icons/source/logo.svg, with the viewBox cropped to the glyph.
export function Logo() {
  // Unique per instance: a repeated gradient id would resolve to the first one in the page.
  const bowl = `${useId()}-bowl`;
  return (
    <svg viewBox="10 10.75 29 29" className="size-5 shrink-0" aria-hidden="true">
      <defs>
        <linearGradient id={bowl} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#7db3ff" />
          <stop offset="1" stopColor="#3d6fd6" />
        </linearGradient>
      </defs>
      <path d="M17 37V14" stroke="#f2f4f8" strokeWidth="5.5" strokeLinecap="round" />
      <path
        d="M17 13.5h7a8 8 0 0 1 0 16h-1.5"
        fill="none"
        stroke={`url(#${bowl})`}
        strokeWidth="5.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
