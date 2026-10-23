import type { ApiError } from "../api/bindings";

/** The one quiet line a failed query gets: no banner, no retry loop. */
export function QuietError({ error, className = "" }: { error: ApiError; className?: string }) {
  return (
    <p role="alert" className={`text-meta text-fg-3 ${className}`}>
      Couldn't load this. {error.message}
    </p>
  );
}
