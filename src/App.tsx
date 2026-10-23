import { useEffect, useState } from "react";
import { prefetchView, useDayRollover, useSettings } from "./api/queries";
import { QuietError } from "./components/QuietError";
import { Rail } from "./components/Rail";
import { TitleBar, TitleSlotContext } from "./components/TitleBar";
import { useSurface } from "./lib/surface";
import { useUi, type View } from "./state/ui";
import { DayView } from "./views/DayView";
import { FirstRun } from "./views/FirstRun";
import { HistoryView } from "./views/HistoryView";
import { PickerView } from "./views/PickerView";
import { SettingsView } from "./views/SettingsView";

const VIEWS: Record<View, () => React.JSX.Element | null> = {
  day: DayView,
  history: HistoryView,
  picker: PickerView,
  settings: SettingsView,
};

export function App() {
  useSurface();
  useDayRollover();
  const settings = useSettings();
  const view = useUi((s) => s.view);
  const [titleSlot, setTitleSlot] = useState<HTMLDivElement | null>(null);
  const onboarded = settings.data?.onboarded;
  const Current = VIEWS[view];

  // Every view but Apps has its first frame's data before the rail is ever pressed.
  useEffect(() => {
    if (onboarded) for (const v of ["day", "history", "settings"] as const) prefetchView(v);
  }, [onboarded]);

  return (
    <TitleSlotContext value={titleSlot}>
      {/* data-view picks the column width (--view-max) that <main> and the titlebar share. */}
      <div className="flex h-screen" data-view={onboarded ? view : "first-run"}>
        {onboarded && <Rail />}
        <div className="flex min-w-0 flex-1 flex-col">
          <TitleBar slotRef={setTitleSlot} brand={onboarded === false} />
          {/* The 12px scrollbar always has its gutter, inside the 24px right margin (12px padding +
              12px gutter), so a view that starts scrolling doesn't shift, and the content box
              matches the titlebar's for the title to line up. */}
          <main className="min-h-0 flex-1 overflow-y-auto pt-2 pr-3 pb-6 pl-6 [scrollbar-gutter:stable]">
            {/* The view column, centered. It is the container the views' @container queries read,
                so a layout follows the room it has rather than the window. */}
            <div className="@container mx-auto h-full max-w-(--view-max)">
              {settings.error ? (
                <QuietError error={settings.error} />
              ) : onboarded === undefined ? null : onboarded ? (
                // View switch: an instant swap, so no frame is blank or half-faded and the body
                // changes in the same frame as the title it portals into the titlebar.
                <div key={view} className="h-full">
                  <Current />
                </div>
              ) : (
                <FirstRun />
              )}
            </div>
          </main>
        </div>
      </div>
    </TitleSlotContext>
  );
}
