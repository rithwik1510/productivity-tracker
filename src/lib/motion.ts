// Motion tokens from docs/design/reference.md section 5, in one place so reduced motion zeroes all
// of them together. Only the allowlisted motions use these: ring fill, count-up, segmented and rail
// thumbs, chart crossfades, chart data changes, popovers, first-run steps and buckets.
import { animate, type Transition, useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";

const EASE_OUT_QUINT = [0.23, 1, 0.32, 1] as const;
export const EASE_DRAWER = [0.32, 0.72, 0, 1] as const;

export function useMotionTokens() {
  const reduce = useReducedMotion() ?? false;
  const t = (transition: Transition): Transition => (reduce ? { duration: 0 } : transition);
  return {
    reduce,
    /** Segmented and category thumbs: no overshoot. */
    spring: t({ type: "spring", visualDuration: 0.25, bounce: 0 }),
    /** Chart crossfade on toggle: opacity only. */
    fade: t({ duration: 0.15, ease: [0.4, 0, 0.2, 1] }),
    popoverIn: t({ duration: 0.15, ease: [0.4, 0, 0.2, 1] }),
    popoverOut: t({ duration: 0.1, ease: [0.4, 0, 0.2, 1] }),
    /** Donut re-sweep and similar in-chart data changes. */
    chart: t({ duration: 0.25, ease: EASE_DRAWER }),
    /** First-run step change: fade plus a 6px rise, the one honest spatial cue (steps are a sequence). */
    step: t({ duration: 0.22, ease: EASE_OUT_QUINT }),
    /** A first-run bucket icon landing: scale 0.5 → 1, no overshoot. */
    pop: t({ duration: 0.22, ease: EASE_OUT_QUINT }),
  };
}

// Last key filled per slot, kept across mounts: coming back to a tab must not replay the fill,
// while a new Day (arrows, heatmap, 4am rollover) or a fresh window always does.
const lastFilled = new Map<string, string>();

/**
 * The ring fill and count-up tween. The first time `key` is shown in `slot` it runs 0 → target over
 * 900 ms; later target changes (live ticks) take 400 ms. `counting` is true only during that first
 * fill, so numbers count up then and simply show the live value afterwards.
 */
export function useFill(target: number, slot: string, key: string) {
  const reduce = useReducedMotion() ?? false;
  const [value, setValue] = useState(() => (reduce || lastFilled.get(slot) === key ? target : 0));
  const [counting, setCounting] = useState(false);
  const valueRef = useRef(value);
  const keyRef = useRef<string | null>(null);
  // Survives StrictMode's effect replay and live ticks that land mid-fill.
  const fillingRef = useRef(false);

  useEffect(() => {
    const newKey = keyRef.current !== key && lastFilled.get(slot) !== key;
    // A fill interrupted by a live tick (or StrictMode) keeps filling from where it got to.
    const filling = newKey || fillingRef.current;
    keyRef.current = key;
    lastFilled.set(slot, key);
    if (reduce) {
      valueRef.current = target;
      setValue(target);
      return;
    }
    fillingRef.current = filling;
    setCounting(filling);
    const controls = animate(newKey ? 0 : valueRef.current, target, {
      duration: filling ? 0.9 : 0.4,
      ease: EASE_OUT_QUINT,
      onUpdate: (v) => {
        valueRef.current = v;
        setValue(v);
      },
      onComplete: () => {
        fillingRef.current = false;
        setCounting(false);
      },
    });
    return () => controls.stop();
  }, [target, slot, key, reduce]);

  return { value, counting };
}
