import type { HTMLAttributes } from "react";

/** A frosted surface: white-alpha fill over Acrylic (solid in fallback), hairline, top highlight. */
export function GlassCard({ className = "", ...props }: HTMLAttributes<HTMLElement>) {
  return (
    <section
      className={`rounded-card border border-border bg-surface-1 shadow-glass ${className}`}
      {...props}
    />
  );
}
