import { motion } from "motion/react";
import { useState } from "react";
import type { Category } from "../api/bindings";
import { useCompleteOnboarding, useSetDailyGoal, useSettings } from "../api/queries";
import { type Target, usePickerTargets } from "../api/targets";
import { Buckets } from "../components/Buckets";
import { CHIP, GHOST, PRIMARY } from "../components/buttons";
import { formatGoal } from "../components/Duration";
import { GlassCard } from "../components/GlassCard";
import { GoalStepper } from "../components/GoalStepper";
import { useMotionTokens } from "../lib/motion";
import { Picker } from "./PickerView";

type Step = "work" | "fun" | "goal";

const PAINT_STEPS = {
  work: {
    category: "productive",
    title: "Tap the apps you work in",
    lead: "Only what you tap is ever tracked. Everything else is ignored, not recorded.",
    hint: "Tap again to undo. Sites keep their own category, whatever the browser's.",
  },
  fun: {
    category: "entertainment",
    title: "Now tap the ones that are for fun",
    lead: "Entertainment has no goal. It is only counted, so you can see the split.",
    hint: "Tapping a blue tile moves it here. Tap again to undo.",
  },
} as const satisfies Record<
  Exclude<Step, "goal">,
  { category: Category; title: string; lead: string; hint: string }
>;

const ORDER: readonly Step[] = ["work", "fun", "goal"];
const QUICK_GOALS = [240, 360, 480];

/**
 * Two painting passes (work apps, then fun ones), then the Daily Goal, then the Day view. Picks
 * save as they're made, so closing midway reopens here with them kept (onboarded stays false
 * until the last step).
 */
export function FirstRun() {
  const [step, setStep] = useState<Step>("work");
  const { step: rise } = useMotionTokens();
  const { apps, sites } = usePickerTargets();
  // Tap order, so each bucket's newest icon lands at its end.
  const [order, setOrder] = useState<readonly string[]>([]);
  const rank = (t: Target) => order.indexOf(t.key);
  const tracked = [...(apps ?? []), ...sites]
    .filter((t) => t.category)
    .sort((a, b) => rank(a) - rank(b));
  const go = (offset: number) => setStep((s) => ORDER[ORDER.indexOf(s) + offset] ?? s);

  return (
    // Step change: fade plus a 6px rise (the one place a spatial cue is honest).
    <motion.div
      key={step}
      className="h-full"
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={rise}
    >
      {step === "goal" ? (
        <GoalStep tracked={tracked} onBack={() => go(-1)} />
      ) : (
        // The 880px first-run column (--view-max) holds the heading, the picker and the footer
        // card, so the Continue button sits under the tiles it confirms, not at the window's edge.
        <div className="flex h-full min-h-0 flex-col">
          <div className="flex min-h-0 flex-1 flex-col pt-1.5">
            <header className="mb-4 shrink-0">
              <StepProgress step={step} />
              <h1 className="mt-2 mb-1 text-heading">{PAINT_STEPS[step].title}</h1>
              <p className="text-fg-2">{PAINT_STEPS[step].lead}</p>
            </header>
            <Picker
              autoFocus
              hint={PAINT_STEPS[step].hint}
              mode={{
                kind: "paint",
                category: PAINT_STEPS[step].category,
                onPaint: (key) => setOrder((o) => [...o.filter((k) => k !== key), key]),
              }}
            />
          </div>
          <footer className="mt-3 flex shrink-0 items-center gap-3 rounded-card border border-border bg-surface-1 px-3 py-3 shadow-glass">
            <Buckets tracked={tracked} active={PAINT_STEPS[step].category} />
            <span className="flex-1" />
            {step === "fun" && (
              <button type="button" className={GHOST} onClick={() => go(-1)}>
                Back
              </button>
            )}
            <button
              type="button"
              className={PRIMARY}
              disabled={tracked.length === 0}
              onClick={() => go(1)}
            >
              Continue
            </button>
          </footer>
        </div>
      )}
    </motion.div>
  );
}

function StepProgress({ step }: { step: Step }) {
  const at = ORDER.indexOf(step);
  return (
    <p className="flex items-center gap-2.5 text-meta text-fg-3">
      {ORDER.map((s, i) => (
        <span
          key={s}
          aria-hidden="true"
          className={`h-[3px] w-5 rounded-full ${i <= at ? "bg-fg-1" : "bg-border-strong"}`}
        />
      ))}
      Step {at + 1} of {ORDER.length}
    </p>
  );
}

function GoalStep({ tracked, onBack }: { tracked: readonly Target[]; onBack: () => void }) {
  const saved = useSettings().data?.dailyGoalMinutes ?? 360;
  const [minutes, setMinutes] = useState(saved);
  const setGoal = useSetDailyGoal();
  const complete = useCompleteOnboarding();
  const busy = setGoal.isPending || complete.isPending;
  const error = setGoal.error ?? complete.error;
  const apps = tracked.filter((t) => t.kind === "app").length;
  const sites = tracked.length - apps;

  const finish = async () => {
    try {
      if (minutes !== saved) await setGoal.mutateAsync(minutes);
      await complete.mutateAsync();
    } catch {
      // Both mutations surface their error below.
    }
  };

  return (
    <div className="grid h-full place-items-center pb-6">
      <GlassCard className="flex w-[440px] flex-col items-center px-8 pt-7 pb-5.5 text-center">
        <StepProgress step="goal" />
        <h1 className="mt-2.5 mb-1 text-heading">Set a daily goal</h1>
        <p className="text-fg-2">
          Productive time you're aiming for each day.
          <br />
          You can change it later in Settings.
        </p>
        <div className="mt-6.5">
          <GoalStepper minutes={minutes} onChange={setMinutes} large />
        </div>
        <fieldset aria-label="Common goals" className="mt-3.5 flex gap-1.5">
          {QUICK_GOALS.map((m) => (
            <button
              key={m}
              type="button"
              aria-pressed={minutes === m}
              onClick={() => setMinutes(m)}
              className={CHIP}
            >
              {formatGoal(m)}
            </button>
          ))}
        </fieldset>
        <p className="mt-5.5 text-meta text-fg-3">
          Tracking {apps} {apps === 1 ? "app" : "apps"}
          {sites > 0 && ` and ${sites} ${sites === 1 ? "site" : "sites"}`}. Change this any time
          under Apps.
        </p>
        {/* What happens after "Start tracking" (it turns launch at login on), said before it. */}
        <p className="mt-1.5 text-meta text-balance text-fg-3">
          Starts with Windows and runs in the tray. Closing the window keeps tracking. Everything
          stays on this PC.
        </p>
        {error && (
          <p role="alert" className="mt-4 text-meta text-fg-1">
            Couldn't finish setup. {error.message}
          </p>
        )}
        <div className="mt-6 flex w-full items-center justify-between">
          <button type="button" className={GHOST} onClick={onBack}>
            Back
          </button>
          <button
            type="button"
            className={PRIMARY}
            disabled={tracked.length === 0 || busy}
            onClick={() => void finish()}
          >
            Start tracking
          </button>
        </div>
      </GlassCard>
    </div>
  );
}
