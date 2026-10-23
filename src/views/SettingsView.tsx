import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import type { Appearance } from "../api/bindings";
import {
  useDeleteAllData,
  useExport,
  useOpenDataFolder,
  useSetAppearance,
  useSetAwayAfter,
  useSetDailyGoal,
  useSetLaunchAtLogin,
  useSettings,
  useSetWaitingCredit,
  useVersion,
} from "../api/queries";
import { DANGER, SECONDARY } from "../components/buttons";
import { GlassCard } from "../components/GlassCard";
import { GoalStepper } from "../components/GoalStepper";
import { ChevronRight } from "../components/Icons";
import { QuietError } from "../components/QuietError";
import { SegmentedControl } from "../components/SegmentedControl";
import { Switch } from "../components/Switch";
import { WindowTitle } from "../components/TitleBar";
import { useMotionTokens } from "../lib/motion";
import { useUi } from "../state/ui";

const APPEARANCES = [
  { value: "glass", label: "Glass" },
  { value: "solid", label: "Solid" },
] as const satisfies readonly { value: Appearance; label: string }[];

// The "Count as away after" choices the backend accepts, as segment values.
const AWAY_AFTER = [2, 5, 10, 15].map((m) => ({ value: String(m), label: `${m} min` }));

/** A headed group of rows in one card, with an optional error and a quiet line under it. */
function Section({
  title,
  error,
  note,
  children,
}: {
  title: string;
  /** Shown in place of nothing when a change in this section fails. */
  error?: string | null;
  note?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section aria-label={title} className="flex flex-col gap-2">
      <h2 className="px-1 text-meta font-semibold text-fg-2">{title}</h2>
      <GlassCard className="divide-y divide-border">{children}</GlassCard>
      {error && (
        <p role="alert" className="px-5 text-meta text-fg-1">
          {error}
        </p>
      )}
      {note && <p className="px-5 text-meta text-fg-3">{note}</p>}
    </section>
  );
}

function SettingRow({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div
      className={`flex items-center justify-between gap-6 px-5 py-4 ${description ? "min-h-18" : ""}`}
    >
      <div className="min-w-0">
        <h3 className="font-medium text-fg-1">{title}</h3>
        {description && <p className="mt-0.5 text-meta text-fg-2">{description}</p>}
      </div>
      {children}
    </div>
  );
}

const saveFailed = (error: { message: string } | null) =>
  error && `Couldn't save that. ${error.message}`;

export function SettingsView() {
  const settings = useSettings();
  const version = useVersion().data;
  const setGoal = useSetDailyGoal();
  const setAwayAfter = useSetAwayAfter();
  const setWaitingCredit = useSetWaitingCredit();
  const setAppearance = useSetAppearance();
  const setLaunch = useSetLaunchAtLogin();
  const exporter = useExport();
  const openFolder = useOpenDataFolder();
  const [confirmDelete, setConfirmDelete] = useState(false);

  if (!settings.data) return settings.error ? <QuietError error={settings.error} /> : null;
  const { dailyGoalMinutes, awayAfterMinutes, waitingCredit, appearance, launchAtLogin } =
    settings.data;

  return (
    <div className="flex flex-col gap-6 pb-2">
      <WindowTitle title="Settings" />

      <Section title="Goal" error={saveFailed(setGoal.error)}>
        <SettingRow
          title="Daily goal"
          description="Productive time to aim for each day. Days run from 4am to 4am."
        >
          <GoalStepper minutes={dailyGoalMinutes} onChange={(m) => setGoal.mutate(m)} />
        </SettingRow>
      </Section>

      <Section title="Tracking" error={saveFailed(setAwayAfter.error ?? setWaitingCredit.error)}>
        <SettingRow
          title="Count as away after"
          description="With no keyboard, mouse or sound for this long, you're away, and the time since your last activity doesn't count. Reading without touching anything? Raise this."
        >
          <SegmentedControl
            label="Count as away after"
            options={AWAY_AFTER}
            value={String(awayAfterMinutes)}
            onChange={(m) => setAwayAfter.mutate(Number(m))}
            className="w-68 shrink-0"
          />
        </SettingRow>
        <SettingRow
          title="Keep counting minimized apps"
          description="A Productive app you minimize keeps counting for up to 30 min while nothing Productive is in front."
        >
          <Switch
            label="Keep counting minimized apps"
            checked={waitingCredit}
            onChange={(on) => setWaitingCredit.mutate(on)}
          />
        </SettingRow>
        <button
          type="button"
          onClick={() => useUi.setState({ view: "picker" })}
          className="flex min-h-18 w-full items-center justify-between gap-6 rounded-b-card px-5 py-4 text-left transition-colors duration-(--dur-fast) ease-standard hover:bg-ghost-hover"
        >
          <span className="min-w-0">
            <span className="block font-medium text-fg-1">Tracked apps & sites</span>
            <span className="mt-0.5 block text-meta text-fg-2">
              Choose what counts, and as which category. Removing one keeps its history.
            </span>
          </span>
          <span className="shrink-0 text-fg-2">
            <ChevronRight />
          </span>
        </button>
      </Section>

      <Section title="Appearance" error={saveFailed(setAppearance.error)}>
        <SettingRow
          title="Window"
          description="Glass lets your desktop show through. Solid is opaque and lighter on battery."
        >
          <SegmentedControl
            label="Appearance"
            options={APPEARANCES}
            value={appearance}
            onChange={(a) => setAppearance.mutate(a)}
            className="w-40 shrink-0"
          />
        </SettingRow>
      </Section>

      <Section
        title="General"
        error={saveFailed(setLaunch.error)}
        note="Closing the window keeps tracking in the tray."
      >
        <SettingRow
          title="Launch at login"
          description="Start quietly in the tray when you sign in to Windows."
        >
          <Switch
            label="Launch at login"
            checked={launchAtLogin}
            onChange={(on) => setLaunch.mutate(on)}
          />
        </SettingRow>
      </Section>

      <Section
        title="Data"
        error={(exporter.error ?? openFolder.error)?.message}
        note={
          exporter.data ? (
            <span role="status">Saved to {exporter.data}</span>
          ) : (
            "Everything stays on this PC."
          )
        }
      >
        <SettingRow
          title="Export"
          description="Daily totals as CSV for a spreadsheet, or every recorded interval as JSON."
        >
          <div className="flex shrink-0 gap-2">
            <button
              type="button"
              aria-label="Export CSV"
              className={SECONDARY}
              disabled={exporter.isPending}
              onClick={() => exporter.mutate("csv")}
            >
              CSV
            </button>
            <button
              type="button"
              aria-label="Export JSON"
              className={SECONDARY}
              disabled={exporter.isPending}
              onClick={() => exporter.mutate("json")}
            >
              JSON
            </button>
          </div>
        </SettingRow>
        <SettingRow title="Data folder" description="Where your history is kept on this PC.">
          <button type="button" className={SECONDARY} onClick={() => openFolder.mutate()}>
            Open folder
          </button>
        </SettingRow>
        <SettingRow
          title="Delete all data"
          description="Erase your history, tracked apps and sites, and settings, and start over."
        >
          <button type="button" className={SECONDARY} onClick={() => setConfirmDelete(true)}>
            Delete…
          </button>
        </SettingRow>
      </Section>

      <Section title="About">
        <SettingRow title="Version">
          <span className="text-fg-2 tabular-nums">{version}</span>
        </SettingRow>
        <SettingRow title="License">
          <span className="text-fg-2">MIT License</span>
        </SettingRow>
      </Section>

      <AnimatePresence>
        {confirmDelete && <DeleteAllDialog onClose={() => setConfirmDelete(false)} />}
      </AnimatePresence>
    </div>
  );
}

/**
 * The confirmation for Delete all data: a modal in the popover style (it fades and rises 4px).
 * Cancel takes focus first, so Enter or Escape never deletes. On success every query starts over and
 * the app shows first run, which unmounts this.
 */
function DeleteAllDialog({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const remove = useDeleteAllData();
  const { popoverIn, popoverOut } = useMotionTokens();

  // showModal puts it in the top layer, traps focus and focuses the first button (Cancel). Guarded,
  // because StrictMode runs the effect twice.
  useEffect(() => {
    if (ref.current?.open === false) ref.current.showModal();
  }, []);

  return (
    <motion.dialog
      ref={ref}
      aria-labelledby="delete-all-title"
      aria-describedby="delete-all-body"
      // Escape closes through state, so the exit fade runs.
      onCancel={(e) => {
        e.preventDefault();
        if (!remove.isPending) onClose();
      }}
      className="m-auto w-[400px] max-w-[calc(100vw-48px)] rounded-card border border-border-strong bg-surface-3 p-5 text-fg-1 shadow-popover backdrop-blur-[24px] backdrop-saturate-[1.4] backdrop:bg-[rgb(0_0_0/45%)]"
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0, transition: popoverIn }}
      exit={{ opacity: 0, y: 4, transition: popoverOut }}
    >
      <h2 id="delete-all-title" className="text-title">
        Delete all data?
      </h2>
      <p id="delete-all-body" className="mt-2 text-fg-2">
        Your history, tracked apps and sites, and settings are erased from this PC, and the app
        starts over from the beginning. This can't be undone.
      </p>
      {remove.error && (
        <p role="alert" className="mt-3 text-meta text-fg-1">
          Couldn't delete everything. {remove.error.message}
        </p>
      )}
      <div className="mt-5 flex justify-end gap-2">
        <button type="button" className={SECONDARY} onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className={DANGER}
          disabled={remove.isPending}
          onClick={() => remove.mutate()}
        >
          Delete everything
        </button>
      </div>
    </motion.dialog>
  );
}
