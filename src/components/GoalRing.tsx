import { useFill } from "../lib/motion";
import { Duration, formatDuration, formatGoal } from "./Duration";

const SIZE = 264;
const STROKE = 10;
const R = (SIZE - STROKE) / 2;
const C = 2 * Math.PI * R;

type Props = {
  productiveSecs: number;
  goalSecs: number;
  /** The shown Day: a new one replays the fill, a live tick only eases to the new value. */
  day: string;
};

/**
 * The Daily Goal ring with today's Productive time as the hero number. The ring and the number run
 * on one tween so they finish together. Meeting the goal changes the label only: same color,
 * no extra glow, no pulse. It fits its box (a size container) up to 380px and the number scales
 * with it.
 */
export function GoalRing({ productiveSecs, goalSecs, day }: Props) {
  const { value, counting } = useFill(productiveSecs, "day-ring", day);
  const progress = goalSecs > 0 ? Math.min(value / goalSecs, 1) : 0;
  const met = goalSecs > 0 && productiveSecs >= goalSecs;
  const goal = formatGoal(Math.round(goalSecs / 60));

  return (
    <div
      role="img"
      aria-label={`Productive ${formatDuration(productiveSecs)} of a ${goal} goal${met ? ", goal met" : ""}`}
      className="@container relative aspect-square w-[min(100cqw,100cqh,380px)]"
    >
      <svg
        viewBox={`0 0 ${SIZE} ${SIZE}`}
        className="block size-full overflow-visible"
        aria-hidden="true"
      >
        <circle
          cx={SIZE / 2}
          cy={SIZE / 2}
          r={R}
          fill="none"
          stroke="var(--border-strong)"
          strokeWidth={STROKE}
        />
        {progress > 0 && (
          <circle
            cx={SIZE / 2}
            cy={SIZE / 2}
            r={R}
            fill="none"
            stroke="var(--productive)"
            strokeWidth={STROKE}
            strokeLinecap="round"
            strokeDasharray={C}
            strokeDashoffset={C * (1 - progress)}
            transform={`rotate(-90 ${SIZE / 2} ${SIZE / 2})`}
            style={{ filter: "var(--productive-glow)" }}
          />
        )}
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <p className="text-meta text-fg-2">Productive</p>
        {/* 19% of the ring: about 38px in the narrow strip, 52px at the default size, capped
            at --text-hero (72px) from about 1440×900 up. */}
        <p className="mt-1.5 text-hero" style={{ fontSize: "min(var(--text-hero), 19cqw)" }}>
          <Duration secs={counting ? Math.round(value) : productiveSecs} />
        </p>
        <p className="mt-2.5 text-meta text-fg-3">{met ? "Goal met" : `of ${goal} goal`}</p>
      </div>
    </div>
  );
}
