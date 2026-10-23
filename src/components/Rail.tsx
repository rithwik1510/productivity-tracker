import { motion } from "motion/react";
import { type KeyboardEvent, type ReactNode, useRef } from "react";
import { prefetchView } from "../api/queries";
import { useMotionTokens } from "../lib/motion";
import { useUi, type View } from "../state/ui";
import { AppsIcon, HistoryIcon, SettingsIcon, TodayIcon } from "./Icons";
import { Logo } from "./Logo";

type Item = { view: View; label: string; icon: ReactNode };

const MAIN: readonly Item[] = [
  { view: "day", label: "Today", icon: <TodayIcon /> },
  { view: "history", label: "History", icon: <HistoryIcon /> },
  { view: "picker", label: "Apps", icon: <AppsIcon /> },
];
const SETTINGS: Item = { view: "settings", label: "Settings", icon: <SettingsIcon /> };
const ALL = [...MAIN, SETTINGS];

/**
 * The 72px navigation rail (Windows NavigationView, compact). A neutral thumb marks the current
 * view and slides to the next one on the segmented spring, Settings included. The thumb lives
 * inside the selected button and Motion's layoutId animates between them, so it always lands on
 * the button's real box whatever the DPI, font or gaps. Arrow keys move focus along the rail.
 * Not a drag region.
 */
export function Rail() {
  const view = useUi((s) => s.view);
  const { spring } = useMotionTokens();
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);

  const onKeyDown = (e: KeyboardEvent) => {
    const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
    const at = e.target instanceof HTMLButtonElement ? buttons.current.indexOf(e.target) : -1;
    if (step === undefined || at < 0) return;
    e.preventDefault();
    buttons.current[Math.min(Math.max(at + step, 0), ALL.length - 1)]?.focus();
  };

  const item = (it: Item, i: number) => (
    <button
      key={it.view}
      ref={(el) => {
        buttons.current[i] = el;
      }}
      type="button"
      aria-current={view === it.view ? "page" : undefined}
      // Its data loads while the pointer or focus is on its way, if it isn't cached yet.
      onPointerEnter={() => prefetchView(it.view)}
      onFocus={() => prefetchView(it.view)}
      // "Today" always means today, even when a past Day is open.
      onClick={() =>
        useUi.setState(it.view === "day" ? { view: "day", selectedDay: null } : { view: it.view })
      }
      className={`relative flex h-13 w-14 flex-col items-center justify-center gap-[3px] rounded-segment text-micro leading-none font-medium transition-colors duration-(--dur-fast) ease-standard ${
        view === it.view ? "text-fg-1" : "text-fg-2 hover:text-fg-1"
      }`}
    >
      {view === it.view && (
        <motion.span
          aria-hidden="true"
          layoutId="rail-thumb"
          className="absolute inset-0 rounded-segment border border-border-strong bg-thumb"
          transition={spring}
        />
      )}
      {/* Lifted over every thumb: mid-slide, the thumb in this button passes over the others. */}
      <span className="relative z-10 grid">{it.icon}</span>
      <span className="relative z-10">{it.label}</span>
    </button>
  );

  return (
    <nav
      aria-label="Views"
      onKeyDown={onKeyDown}
      className="flex w-18 shrink-0 flex-col items-center border-r border-border bg-rail pb-2.5"
    >
      <div
        className="grid h-(--titlebar-height) w-full place-items-center"
        title="Productivity Tracker"
      >
        <Logo />
      </div>
      <div className="mt-1.5 flex flex-col gap-1">{MAIN.map((it, i) => item(it, i))}</div>
      <div className="mt-auto">{item(SETTINGS, MAIN.length)}</div>
    </nav>
  );
}
