import { ICON } from "./buttons";
import { Duration } from "./Duration";
import { Minus, Plus } from "./Icons";

// Matches set_daily_goal's accepted range.
const MIN = 30;
const MAX = 960;
const STEP = 30;

/** Daily Goal in 30-minute steps, 30 min to 16 h. */
export function GoalStepper({
  minutes,
  onChange,
  large = false,
}: {
  minutes: number;
  onChange: (minutes: number) => void;
  large?: boolean;
}) {
  return (
    <fieldset aria-label="Daily goal" className="flex min-w-0 items-center gap-2">
      <button
        type="button"
        aria-label="Lower the goal by 30 minutes"
        disabled={minutes <= MIN}
        onClick={() => onChange(Math.max(MIN, minutes - STEP))}
        className={`${ICON} border border-border-strong`}
      >
        <Minus />
      </button>
      <output
        aria-live="polite"
        className={`text-center font-medium ${large ? "w-40 text-stat" : "w-20 text-body"}`}
      >
        <Duration secs={minutes * 60} goal />
      </output>
      <button
        type="button"
        aria-label="Raise the goal by 30 minutes"
        disabled={minutes >= MAX}
        onClick={() => onChange(Math.min(MAX, minutes + STEP))}
        className={`${ICON} border border-border-strong`}
      >
        <Plus />
      </button>
    </fieldset>
  );
}
