// Stroke icons at 1.5px, so they sit at the same weight as Inter's text: a 16px grid for controls,
// 20px for the rail.
import type { ReactNode, SVGProps } from "react";

function Icon({
  children,
  size = 16,
  ...props
}: SVGProps<SVGSVGElement> & { children: ReactNode; size?: 16 | 20 }) {
  return (
    <svg
      viewBox={`0 0 ${size} ${size}`}
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {children}
    </svg>
  );
}

export const ChevronLeft = () => (
  <Icon>
    <path d="M10 3.5 5.5 8l4.5 4.5" />
  </Icon>
);

export const ChevronRight = () => (
  <Icon>
    <path d="M6 3.5 10.5 8 6 12.5" />
  </Icon>
);

export const ChevronDown = () => (
  <Icon>
    <path d="M3.5 6 8 10.5 12.5 6" />
  </Icon>
);

export const Plus = () => (
  <Icon>
    <path d="M8 3.5v9M3.5 8h9" />
  </Icon>
);

export const Minus = () => (
  <Icon>
    <path d="M3.5 8h9" />
  </Icon>
);

// Caption glyphs, Windows 11 style: a rounded square, and two offset squares once maximized.
export const Maximize = () => (
  <Icon>
    <rect x="4" y="4" width="8" height="8" rx="1.5" />
  </Icon>
);

export const Restore = () => (
  <Icon>
    <rect x="4" y="6" width="6" height="6" rx="1.25" />
    <path d="M6 6v-.5A1.5 1.5 0 0 1 7.5 4h3A1.5 1.5 0 0 1 12 5.5v3a1.5 1.5 0 0 1-1.5 1.5H10" />
  </Icon>
);

export const Close = () => (
  <Icon>
    <path d="m4 4 8 8M12 4l-8 8" />
  </Icon>
);

export const Search = () => (
  <Icon>
    <circle cx="7" cy="7" r="4.25" />
    <path d="m10.25 10.25 3 3" />
  </Icon>
);

export const Check = (props: SVGProps<SVGSVGElement>) => (
  <Icon {...props}>
    <path d="m3.5 8.5 3 3 6-7" />
  </Icon>
);

// Rail icons (20px).

export const TodayIcon = () => (
  <Icon size={20}>
    <circle cx="10" cy="10" r="7" opacity=".35" />
    <path d="M10 3a7 7 0 0 1 7 7 7 7 0 0 1-3.5 6.06" />
  </Icon>
);

export const HistoryIcon = () => (
  <Icon size={20}>
    <path d="M4.5 16V9.5M10 16V4M15.5 16v-5.5" />
  </Icon>
);

export const AppsIcon = () => (
  <Icon size={20}>
    <rect x="3" y="3" width="5.5" height="5.5" rx="1.5" />
    <rect x="11.5" y="3" width="5.5" height="5.5" rx="1.5" />
    <rect x="3" y="11.5" width="5.5" height="5.5" rx="1.5" />
    <rect x="11.5" y="11.5" width="5.5" height="5.5" rx="1.5" />
  </Icon>
);

export const SettingsIcon = () => (
  <Icon size={20}>
    <path d="M3 6h6.5M14.5 6H17M3 14h3.5M10.5 14H17" />
    <circle cx="12" cy="6" r="2" />
    <circle cx="8" cy="14" r="2" />
  </Icon>
);
