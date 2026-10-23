import { create } from "zustand";
import type { ShareRange } from "../api/bindings";

export type View = "day" | "history" | "picker" | "settings";
export type HistoryChart = "bars" | "pie" | "heatmap";

type UiState = {
  view: View;
  /** null = today. Kept null whenever today is shown, so the 4am rollover follows. */
  selectedDay: string | null;
  /** History position lives here (not in the view) so opening a Day and coming back keeps it. */
  historyChart: HistoryChart;
  /** null = current week / year, for the same rollover reason as selectedDay. */
  weekStart: string | null;
  year: number | null;
  shareRange: ShareRange;
};

// Components write with useUi.setState directly; there are no action wrappers.
export const useUi = create<UiState>(() => ({
  view: "day",
  selectedDay: null,
  historyChart: "bars",
  weekStart: null,
  year: null,
  shareRange: "week",
}));
