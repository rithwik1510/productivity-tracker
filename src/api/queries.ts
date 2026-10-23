// TanStack Query hooks over the generated bindings. Commands are called directly; the hooks only
// own cache keys, polling and the optimistic picker/settings updates.
import {
  keepPreviousData,
  type MutationScope,
  QueryClient,
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { useUi, type View } from "../state/ui";
import {
  type ApiError,
  type AppIdentity,
  type CatalogApp,
  type Category,
  commands,
  type ExportFormat,
  type Settings,
  type ShareRange,
} from "./bindings";

declare module "@tanstack/react-query" {
  // Commands reject with ApiError (ErrorHandlingMode::Throw), so that is every query's error type.
  interface Register {
    defaultError: ApiError;
  }
}

// Every query is local and small, and the cache dies with the window, so nothing is ever garbage
// collected: a view switched back to always has data to draw in its first frame.
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
    mutations: { retry: false },
  },
});

const catalogQuery = queryOptions({ queryKey: ["catalog"], queryFn: commands.listCatalog });
const sitesQuery = queryOptions({ queryKey: ["sites"], queryFn: commands.listSites });
const settingsQuery = queryOptions({ queryKey: ["settings"], queryFn: commands.getSettings });
const statusQuery = queryOptions({ queryKey: ["status"], queryFn: commands.getStatus });
const versionQuery = queryOptions({
  queryKey: ["version"],
  queryFn: getVersion,
  staleTime: Number.POSITIVE_INFINITY,
});
const dayQuery = (day: string | null) =>
  queryOptions({ queryKey: ["day", day], queryFn: () => commands.getDay(day) });
const weekQuery = (start: string | null) =>
  queryOptions({ queryKey: ["week", start], queryFn: () => commands.getWeek(start) });
const shareQuery = (range: ShareRange) =>
  queryOptions({ queryKey: ["share", range], queryFn: () => commands.getShare(range) });
const yearQuery = (year: number | null) =>
  queryOptions({ queryKey: ["year", year], queryFn: () => commands.getYear(year) });

/**
 * Loads what a rail view draws first, if it isn't cached yet, so switching to it never lands on
 * an empty frame. The app runs it for Today, History and Settings once onboarded, and the rail on
 * hover or focus; Apps waits for that, because its catalog listing is the one costly query.
 * Mounting the view refetches whatever is stale.
 */
export function prefetchView(view: View) {
  const ui = useUi.getState();
  const qc = queryClient;
  switch (view) {
    case "day":
      void qc.ensureQueryData(dayQuery(ui.selectedDay));
      void qc.ensureQueryData(statusQuery);
      break;
    case "history":
      void qc.ensureQueryData(shareQuery("all"));
      if (ui.historyChart === "bars") void qc.ensureQueryData(weekQuery(ui.weekStart));
      if (ui.historyChart === "pie") void qc.ensureQueryData(shareQuery(ui.shareRange));
      if (ui.historyChart === "heatmap") void qc.ensureQueryData(yearQuery(ui.year));
      break;
    case "picker":
      void qc.ensureQueryData(catalogQuery);
      void qc.ensureQueryData(sitesQuery);
      break;
    case "settings":
      void qc.ensureQueryData(settingsQuery);
      void qc.ensureQueryData(versionQuery);
      break;
  }
}

// Every query whose numbers depend on intervals, categories or the goal ("status" stops naming an
// app the moment it is switched Off).
const DATA_KEYS = new Set<unknown>(["day", "week", "share", "year", "status"]);
const invalidateData = (qc: QueryClient) =>
  qc.invalidateQueries({ predicate: (q) => DATA_KEYS.has(q.queryKey[0]) });

// A pause or resume changes the status line: both what is paused and what is counted.
const PAUSE_KEYS = new Set<unknown>(["pause", "status"]);
const invalidatePause = (qc: QueryClient) =>
  qc.invalidateQueries({ predicate: (q) => PAUSE_KEYS.has(q.queryKey[0]) });

const sameIdentity = (a: AppIdentity, b: AppIdentity) => a.kind === b.kind && a.key === b.key;

export const useDay = (day: string | null) =>
  useQuery({
    ...dayQuery(day),
    placeholderData: keepPreviousData,
    // Only today moves; past Days are final. Time accrues while other apps are in front, so the
    // poll keeps running while this window is unfocused.
    refetchInterval: (q) => (q.state.data?.isToday ? 5000 : false),
    refetchIntervalInBackground: true,
  });

/**
 * What the tracker is doing right now (the status line), or null before its first tick. Polled
 * like today's Day, and only while today is shown; the poll dies with the window, so nothing
 * watches while it is closed.
 */
export const useStatus = (enabled: boolean) =>
  useQuery({
    ...statusQuery,
    enabled,
    refetchInterval: 5000,
    refetchIntervalInBackground: true,
  });

// History's numbers that include today move while the window is open, more slowly than the Day
// view's, and like it they keep polling while the window is unfocused.
const HISTORY_POLL_MS = 30_000;

/**
 * Today's Day string, for charts that must tell past from future. Shares the get_day(None) cache,
 * and polls it every 30 s so the 4am rollover shows up in every view (`useDayRollover`).
 */
export const useToday = () =>
  useQuery({
    ...dayQuery(null),
    select: (d) => d.totals.day,
    refetchInterval: HISTORY_POLL_MS,
    refetchIntervalInBackground: true,
  }).data;

/**
 * The 4am rollover, as the Day view sees it: today's Day string changes. Every data query then
 * refetches, so History's current week, share and year move on with it.
 */
export function useDayRollover() {
  const qc = useQueryClient();
  const today = useToday();
  const seen = useRef(today);
  useEffect(() => {
    if (today === undefined) return;
    if (seen.current !== undefined && seen.current !== today) void invalidateData(qc);
    seen.current = today;
  }, [qc, today]);
}

export const useWeek = (start: string | null) =>
  useQuery({
    ...weekQuery(start),
    placeholderData: keepPreviousData,
    refetchInterval: (q) => (q.state.data?.isCurrent ? HISTORY_POLL_MS : false),
    refetchIntervalInBackground: true,
  });

/** Every range (this week, this month, all time) includes today. */
export const useShare = (range: ShareRange) =>
  useQuery({
    ...shareQuery(range),
    placeholderData: keepPreviousData,
    refetchInterval: HISTORY_POLL_MS,
    refetchIntervalInBackground: true,
  });

export const useYear = (year: number | null) =>
  useQuery({
    ...yearQuery(year),
    placeholderData: keepPreviousData,
    refetchInterval: (q) => (q.state.data?.isCurrent ? HISTORY_POLL_MS : false),
    refetchIntervalInBackground: true,
  });

/**
 * Installed and running apps. A listing returns before every icon is extracted (those come
 * without iconPath), and the backend emits "catalog-icons" once the rest are on disk.
 */
export function useCatalog() {
  const qc = useQueryClient();
  useEffect(() => {
    const stop = listen("catalog-icons", () =>
      qc.invalidateQueries({ queryKey: catalogQuery.queryKey }),
    );
    return () => void stop.then((unlisten) => unlisten());
  }, [qc]);
  return useQuery(catalogQuery);
}

export const useSites = () => useQuery(sitesQuery);
export const useSettings = () => useQuery(settingsQuery);

/**
 * Optimistic: the row flips at once and rolls back alone if the backend refuses. One scope sends
 * toggles in click order, and a rollback only applies while the row still shows the refused
 * value, so a failure never overwrites a later click.
 */
export function useSetAppCategory() {
  const qc = useQueryClient();
  const patch = (identity: AppIdentity, category: Category | null, onlyIf?: Category | null) =>
    qc.setQueryData(catalogQuery.queryKey, (apps) =>
      apps?.map((a) =>
        sameIdentity(a.identity, identity) && (onlyIf === undefined || a.category === onlyIf)
          ? { ...a, category }
          : a,
      ),
    );
  return useMutation({
    scope: { id: "catalog" },
    mutationFn: ({ app, category }: { app: CatalogApp; category: Category | null }) =>
      commands.setAppCategory(app.identity, app.name, category),
    onMutate: async ({ app, category }) => {
      await qc.cancelQueries({ queryKey: catalogQuery.queryKey });
      patch(app.identity, category);
    },
    onError: (_error, { app, category }) => patch(app.identity, app.category, category),
    onSettled: () => invalidateData(qc),
  });
}

type SiteInput = { domain: string; keyword: string | null; category: Category | null };

/**
 * Category flips are optimistic; keyword edits and new sites land when the backend accepts them.
 * `keyword: null` keeps the stored keyword. A site row passes its own scope, so a keyword commit
 * on blur and the category click that caused the blur reach the backend in that order.
 */
export function useUpsertSite(scope?: MutationScope) {
  const qc = useQueryClient();
  return useMutation({
    scope,
    mutationFn: ({ domain, keyword, category }: SiteInput) =>
      commands.upsertSite(domain, keyword, category),
    onMutate: async ({ domain, category }) => {
      await qc.cancelQueries({ queryKey: sitesQuery.queryKey });
      const previous = qc.getQueryData(sitesQuery.queryKey)?.find((s) => s.domain === domain);
      if (previous) {
        qc.setQueryData(sitesQuery.queryKey, (sites) =>
          sites?.map((s) => (s.id === previous.id ? { ...s, category } : s)),
        );
      }
      return { previous };
    },
    // As with apps: only undo while the row still shows the refused category.
    onError: (_error, { category }, ctx) => {
      const previous = ctx?.previous;
      if (previous) {
        qc.setQueryData(sitesQuery.queryKey, (sites) =>
          sites?.map((s) => (s.id === previous.id && s.category === category ? previous : s)),
        );
      }
    },
    // New sites go first so they appear right under the input; the order stays put otherwise.
    onSuccess: (site) =>
      qc.setQueryData(sitesQuery.queryKey, (sites = []) =>
        sites.some((s) => s.id === site.id)
          ? sites.map((s) => (s.id === site.id ? site : s))
          : [site, ...sites],
      ),
    onSettled: () => invalidateData(qc),
  });
}

/**
 * One setting, saved optimistically: the control moves at once and rolls back alone if the backend
 * refuses. Each setting has its own scope, so changes are sent in click order, and a rollback only
 * applies while the setting still shows the refused value, so it never undoes a later change.
 */
function useSetSetting<K extends keyof Settings>(
  key: K,
  command: (value: Settings[K]) => Promise<null>,
  { affectsData = false } = {},
) {
  const qc = useQueryClient();
  return useMutation({
    scope: { id: key },
    mutationFn: command,
    onMutate: async (value: Settings[K]) => {
      await qc.cancelQueries({ queryKey: settingsQuery.queryKey });
      const previous = qc.getQueryData(settingsQuery.queryKey)?.[key];
      qc.setQueryData(settingsQuery.queryKey, (s) => s && { ...s, [key]: value });
      return { previous };
    },
    onError: (_error, value, ctx) =>
      qc.setQueryData(settingsQuery.queryKey, (s) =>
        s && ctx?.previous !== undefined && s[key] === value ? { ...s, [key]: ctx.previous } : s,
      ),
    onSettled: () => (affectsData ? invalidateData(qc) : undefined),
  });
}

// The goal moves the ring and History's shading, so it refreshes the data queries.
export const useSetDailyGoal = () =>
  useSetSetting("dailyGoalMinutes", commands.setDailyGoal, { affectsData: true });
export const useSetLaunchAtLogin = () => useSetSetting("launchAtLogin", commands.setLaunchAtLogin);
/** The surface follows at once: lib/surface.ts reads the cached setting. */
export const useSetAppearance = () => useSetSetting("appearance", commands.setAppearance);
export const useSetAwayAfter = () => useSetSetting("awayAfterMinutes", commands.setAwayAfter);
export const useSetWaitingCredit = () => useSetSetting("waitingCredit", commands.setWaitingCredit);

export function useCompleteOnboarding() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: commands.completeOnboarding,
    onSuccess: () => qc.invalidateQueries({ queryKey: settingsQuery.queryKey }),
  });
}

/**
 * When the tray's pause ends ("3:40 PM"), or null. Polled with "status" while today is shown, and
 * refetched at once when the tray pauses or resumes.
 */
export function usePause(enabled: boolean) {
  const qc = useQueryClient();
  useEffect(() => {
    const stop = listen("pause-changed", () => invalidatePause(qc));
    return () => void stop.then((unlisten) => unlisten());
  }, [qc]);
  return useQuery({
    queryKey: ["pause"],
    queryFn: commands.getPause,
    enabled,
    refetchInterval: 5000,
    refetchIntervalInBackground: true,
  });
}

export function useResume() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: commands.resumeTracking,
    onSettled: () => invalidatePause(qc),
  });
}

/** Resolves to the saved file's path, or null when the save dialog was cancelled. */
export const useExport = () =>
  useMutation({ mutationFn: (format: ExportFormat) => commands.exportData(format) });

export const useOpenDataFolder = () => useMutation({ mutationFn: commands.openDataFolder });

/**
 * Wipes everything. Every cached query starts over, so settings come back not onboarded and the
 * app shows first run; the view state starts over too, so onboarding lands on today.
 */
export function useDeleteAllData() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: commands.deleteAllData,
    onSuccess: async () => {
      useUi.setState(useUi.getInitialState());
      await qc.resetQueries();
    },
  });
}

/** The app version Tauri reports, so About matches the exe. */
export const useVersion = () => useQuery(versionQuery);
