import type { ReactNode } from "react";

type Props = {
  icon: ReactNode;
  name: string;
  /** Muted text after the name, e.g. "other sites" for a browser's own Category, or "incl. 18m waiting". */
  suffix?: string;
  /** Trailing content: a bar and a time. */
  children?: ReactNode;
};

/** 44px list row: 24px icon, 12px gap, a name that truncates with its full text as a tooltip. */
export function AppRow({ icon, name, suffix, children }: Props) {
  const full = suffix ? `${name} · ${suffix}` : name;
  return (
    <div className="flex h-11 items-center gap-3 px-4">
      {icon}
      <p className="min-w-0 flex-1 truncate font-medium text-fg-1" title={full}>
        {name}
        {suffix && <span className="font-normal text-fg-3"> · {suffix}</span>}
      </p>
      {children}
    </div>
  );
}
