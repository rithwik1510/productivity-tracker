// Browser-only stand-in for the Tauri backend (plan section 7). main.tsx imports it dynamically in
// Vite dev when there is no Tauri runtime, so the UI and Playwright run without Rust.
//   ?fixture=default|empty|firstRun|categorySplit|manyApps|longNames|browserAndSite  picks the data
//   ?surface=glass|solid  sets glass_available (default glass)
//   ?appearance=solid     starts with the Solid appearance setting (default glass)
//   ?paused=1             starts paused from the tray, until 3:40 PM
//   ?waiting=1            get_status reports its app as waiting in (minimized, ADR-0007)
//   ?status=saveFailed|away|awayBrief|untracked  get_status reports that state instead of
//                         tracking (a pause still wins over all but saveFailed, as in the backend)
//   ?fail=<command>       makes that command throw, to exercise optimistic rollbacks
//   ?slow=<command>       makes that command never answer, to hold a loading state for a screenshot
import { emit } from "@tauri-apps/api/event";
import { mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type {
  ApiError,
  Appearance,
  AppIdentity,
  CatalogApp,
  Category,
  DayItem,
  DaySummary,
  DayTotals,
  ExportFormat,
  NowTracking,
  Settings,
  Share,
  ShareRange,
  Site,
  TargetRef,
  TrackerStatus,
  WeekSummary,
  YearHeatmap,
} from "../api/bindings";
import { addDays, weekday } from "../lib/day";

const params = new URLSearchParams(location.search);
// A fixed Wednesday, so screenshots never drift. Only the mock backend may know "today".
const TODAY = "2026-10-21";
const ICONS = "C:\\Users\\you\\AppData\\Local\\ProductivityTracker\\icons\\";

// ---- Fixtures ----------------------------------------------------------------------------------

type FixtureApp = {
  name: string;
  identity: AppIdentity;
  category?: Category;
  browser?: boolean;
  /** false = no extracted icon, so the UI shows a monogram */
  icon?: boolean;
};

// Open on this PC "now", as list_catalog reports it; everything else is merely installed.
const RUNNING = new Set([
  "Visual Studio Code",
  "Claude",
  "Google Chrome",
  "Figma",
  "Discord",
  "Spotify",
  "Slack",
  "Windows Terminal",
  "Notion",
  "Steam",
  "Microsoft Edge",
  "Outlook",
  "Microsoft Excel",
]);

type Fixture = {
  onboarded: boolean;
  apps: FixtureApp[];
  sites: { domain: string; keyword: string | null; category: Category | null }[];
  /**
   * Today's time per target (app name or site domain) and category, in minutes, and how many of
   * those minutes were waiting credit (a minimized agent or render counting while nothing
   * Productive was in front, at most 30 minutes per minimize).
   */
  today: [name: string, category: Category, minutes: number, waitingMinutes?: number][];
  /** Typical weekday and weekend minutes per target; jittered per Day from `since` to yesterday. */
  history: [name: string, weekdayMinutes: number, weekendMinutes: number][];
  since: string;
};

const exe = (key: string): AppIdentity => ({ kind: "exe", key });
const aumid = (key: string): AppIdentity => ({ kind: "aumid", key });

const UNTRACKED: FixtureApp[] = [
  { name: "Google Chrome", identity: exe("chrome.exe"), browser: true },
  { name: "Microsoft Edge", identity: exe("msedge.exe"), browser: true },
  { name: "Discord", identity: exe("discord.exe") },
  { name: "OBS Studio", identity: exe("obs64.exe") },
  { name: "Blender", identity: exe("blender.exe") },
  { name: "Microsoft Excel", identity: exe("excel.exe") },
  { name: "Microsoft Word", identity: exe("winword.exe") },
  { name: "Outlook", identity: exe("olk.exe") },
  { name: "Obsidian", identity: exe("obsidian.exe") },
  { name: "Zoom Workplace", identity: exe("zoom.exe") },
  { name: "Telegram", identity: exe("telegram.exe"), icon: false },
  { name: "VLC media player", identity: exe("vlc.exe") },
  { name: "Calculator", identity: aumid("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App") },
  { name: "Paint", identity: aumid("Microsoft.Paint_8wekyb3d8bbwe!App") },
];

const TRACKED: FixtureApp[] = [
  { name: "Visual Studio Code", identity: exe("code.exe"), category: "productive" },
  { name: "Claude", identity: exe("claude.exe"), category: "productive" },
  {
    name: "Windows Terminal",
    identity: aumid("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"),
    category: "productive",
  },
  { name: "Figma", identity: exe("figma.exe"), category: "productive" },
  { name: "Notion", identity: exe("notion.exe"), category: "productive" },
  { name: "Slack", identity: exe("slack.exe"), category: "productive" },
  {
    name: "Spotify",
    identity: aumid("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"),
    category: "entertainment",
  },
  { name: "Steam", identity: exe("steam.exe"), category: "entertainment" },
];

const DEFAULT: Fixture = {
  onboarded: true,
  apps: [...TRACKED, ...UNTRACKED],
  sites: [
    { domain: "github.com", keyword: "github", category: "productive" },
    { domain: "youtube.com", keyword: "youtube", category: "entertainment" },
  ],
  today: [
    ["Visual Studio Code", "productive", 151],
    // Agents that waited in, minimized, while YouTube or Spotify was in front.
    ["Claude", "productive", 52, 41],
    ["Windows Terminal", "productive", 48, 17],
    ["Figma", "productive", 34],
    ["github.com", "productive", 22],
    ["Notion", "productive", 12],
    // Under a minute of waiting: no "incl." note.
    ["Slack", "productive", 5, 0],
    ["youtube.com", "entertainment", 41],
    ["Spotify", "entertainment", 24],
  ],
  history: [
    ["Visual Studio Code", 170, 40],
    ["Claude", 45, 15],
    ["Windows Terminal", 50, 10],
    ["Figma", 45, 5],
    ["github.com", 25, 10],
    ["Notion", 20, 5],
    ["Slack", 25, 0],
    ["youtube.com", 35, 80],
    ["Spotify", 20, 30],
    ["Steam", 10, 90],
  ],
  since: "2026-02-02",
};

const untrack = (apps: FixtureApp[]) => apps.map(({ category: _, ...a }) => a);

const NOTHING: Fixture = {
  onboarded: true,
  apps: untrack(DEFAULT.apps),
  sites: [],
  today: [],
  history: [],
  since: TODAY,
};

const MANY_NAMES = [
  "Affinity Designer 2",
  "Android Studio",
  "Audacity",
  "Battle.net",
  "Bitwarden",
  "Brave",
  "Canva",
  "Cursor",
  "DaVinci Resolve",
  "DBeaver",
  "Docker Desktop",
  "Epic Games Launcher",
  "Everything",
  "File Explorer",
  "Firefox",
  "GitHub Desktop",
  "GOG Galaxy",
  "HandBrake",
  "IntelliJ IDEA",
  "Inkscape",
  "Krita",
  "Linear",
  "Logseq",
  "Microsoft PowerPoint",
  "Microsoft Teams",
  "Miro",
  "MongoDB Compass",
  "Netflix",
  "Notepad++",
  "Obsidian Canary",
  "OneNote",
  "Parsec",
  "Postman",
  "PowerToys",
  "Prime Video",
  "Rider",
  "Signal",
  "Sublime Text",
  "Thunderbird",
  "Todoist",
  "Ubisoft Connect",
  "Unity Hub",
  "WhatsApp",
  "Windsurf",
  "Xbox",
];

const MANY: Fixture = {
  ...DEFAULT,
  apps: [
    ...DEFAULT.apps,
    ...MANY_NAMES.map((name, i) => ({
      name,
      identity: exe(`${name.toLowerCase().replace(/[^a-z0-9]+/g, "")}.exe`),
      category:
        i % 4 === 0
          ? ("productive" as const)
          : i % 7 === 0
            ? ("entertainment" as const)
            : undefined,
    })),
  ],
  today: [
    ...DEFAULT.today,
    ["Affinity Designer 2", "productive", 26],
    ["DBeaver", "productive", 19],
    ["Everything", "productive", 3],
    ["IntelliJ IDEA", "productive", 44],
    ["Linear", "productive", 16],
    ["Microsoft PowerPoint", "productive", 9],
    ["Parsec", "entertainment", 31],
    ["Notepad++", "productive", 1],
  ],
};

const LONG_APP =
  "Microsoft Visual Studio Enterprise 2026 Preview with an Unreasonably Long Window Name (Administrator)";
const LONG_SITE = "very-long-subdomain-for-internal-dashboards.engineering.example-corporation.com";

const LONG: Fixture = {
  ...DEFAULT,
  apps: [
    { name: LONG_APP, identity: exe("devenv.exe"), category: "productive" },
    {
      name: "Hyper-Realistic Flight Simulator: Collector's Anniversary Edition Launcher",
      identity: exe("flightsimlauncher.exe"),
      category: "entertainment",
      icon: false,
    },
    ...DEFAULT.apps,
  ],
  sites: [...DEFAULT.sites, { domain: LONG_SITE, keyword: "dashboards", category: "productive" }],
  today: [
    [LONG_APP, "productive", 222],
    [LONG_SITE, "productive", 37],
    [
      "Hyper-Realistic Flight Simulator: Collector's Anniversary Edition Launcher",
      "entertainment",
      64,
    ],
    ...DEFAULT.today,
  ],
};

const FIXTURES: Record<string, Fixture> = {
  default: DEFAULT,
  empty: NOTHING,
  // A realistic catalog: a dozen running apps and a long tail of installed ones.
  firstRun: { ...NOTHING, onboarded: false, apps: untrack(MANY.apps) },
  // Discord was Entertainment this morning and is Productive now: two rows (ADR-0005).
  categorySplit: {
    ...DEFAULT,
    apps: DEFAULT.apps.map((a) => (a.name === "Discord" ? { ...a, category: "productive" } : a)),
    today: [...DEFAULT.today, ["Discord", "productive", 80], ["Discord", "entertainment", 35]],
  },
  manyApps: MANY,
  longNames: LONG,
  browserAndSite: {
    ...DEFAULT,
    apps: DEFAULT.apps.map((a) =>
      a.name === "Google Chrome" ? { ...a, category: "productive" } : a,
    ),
    sites: [
      ...DEFAULT.sites,
      { domain: "docs.google.com", keyword: "google docs", category: "productive" },
      { domain: "twitch.tv", keyword: "twitch", category: "entertainment" },
    ],
    today: [
      ["github.com", "productive", 70],
      ["Google Chrome", "productive", 52],
      ["docs.google.com", "productive", 40],
      ["Visual Studio Code", "productive", 96],
      ["youtube.com", "entertainment", 30],
      ["twitch.tv", "entertainment", 18],
    ],
  },
};

const fixture = FIXTURES[params.get("fixture") ?? "default"] ?? DEFAULT;
const glass = params.get("surface") !== "solid";
const failing = params.get("fail");
const slow = params.get("slow");

// ---- Mutable state -----------------------------------------------------------------------------

/** The window's maximized state. Any change also fires a resize, as the real window does. */
let maximized = false;

type MockApp = CatalogApp & { id: number };

const slug = (identity: AppIdentity) =>
  identity.key
    .toLowerCase()
    .replace(/\.exe$/, "")
    .replace(/[^a-z0-9]+/g, "-");

const apps: MockApp[] = fixture.apps.map((a, i) => ({
  id: i + 1,
  identity: a.identity,
  name: a.name,
  iconPath: a.icon === false ? null : `${ICONS}${slug(a.identity)}.png`,
  isBrowser: a.browser ?? false,
  isRunning: RUNNING.has(a.name),
  category: a.category ?? null,
}));
const sites: Site[] = fixture.sites.map((s, i) => ({ id: i + 1, ...s }));
const settings: Settings = {
  dailyGoalMinutes: 360,
  launchAtLogin: fixture.onboarded,
  onboarded: fixture.onboarded,
  appearance: params.get("appearance") === "solid" ? "solid" : "glass",
  awayAfterMinutes: 5,
  // Off on a new install; an onboarded fixture stands for an install that had it on.
  waitingCredit: fixture.onboarded,
};
/** When the tray's pause ends, as get_pause formats it; null when not paused. */
let pausedUntil: string | null = params.get("paused") ? "3:40 PM" : null;
/** Set by delete_all_data: no history is left on any Day. */
let wiped = false;

const apiError = (message: string): ApiError => ({ message });

function targetByName(name: string): TargetRef {
  const app = apps.find((a) => a.name === name);
  if (app) return { kind: "app", id: app.id };
  const site = sites.find((s) => s.domain === name);
  if (site) return { kind: "site", id: site.id };
  throw new Error(`fixture names an unknown target: ${name}`);
}

function toItem(target: TargetRef, category: Category, secs: number, minimizedSecs = 0): DayItem {
  const app = target.kind === "app" ? apps.find((a) => a.id === target.id) : undefined;
  const site = target.kind === "site" ? sites.find((s) => s.id === target.id) : undefined;
  return {
    target,
    category,
    secs,
    minimizedSecs,
    name: app?.name ?? site?.domain ?? "Unknown",
    iconPath: app?.iconPath ?? null,
    isBrowser: app?.isBrowser ?? false,
  };
}

// ---- Usage -------------------------------------------------------------------------------------

/** Deterministic 0..1 from a string (FNV-1a, then a mulberry32 step). */
function rand(seed: string) {
  let h = 2166136261;
  for (let i = 0; i < seed.length; i++) h = Math.imul(h ^ seed.charCodeAt(i), 16777619);
  h = Math.imul(h ^ (h >>> 15), h | 1);
  h ^= h + Math.imul(h ^ (h >>> 7), h | 61);
  return ((h ^ (h >>> 14)) >>> 0) / 4294967296;
}

const recorded = new Map<string, DayItem[]>();

/** Time recorded on a Day, keyed by (target, category). History keeps the category it had. */
function recordedOn(day: string): DayItem[] {
  const cached = recorded.get(day);
  if (cached) return cached;
  let items: DayItem[] = [];
  if (wiped) {
    // Nothing recorded anywhere.
  } else if (day === TODAY) {
    // Both jitter by under a minute; fixtures keep waiting minutes below the total's minutes.
    items = fixture.today.map(([name, category, minutes, waiting]) =>
      toItem(
        targetByName(name),
        category,
        minutes * 60 + Math.round(rand(name) * 50),
        waiting === undefined ? 0 : waiting * 60 + Math.round(rand(`${name}:min`) * 50),
      ),
    );
  } else if (day >= fixture.since && day < TODAY) {
    const weekend = weekday(day) >= 5;
    const off = rand(`${day}:off`) < (weekend ? 0.25 : 0.08);
    // A gentle upward drift over the year, so the heatmap has a shape.
    const drift = 0.75 + 0.4 * ((Date.parse(day) - Date.parse(fixture.since)) / 3.2e10);
    items = off
      ? []
      : fixture.history.flatMap(([name, weekdayMin, weekendMin]) => {
          const target = targetByName(name);
          const category =
            fixture.apps.find((a) => a.name === name)?.category ??
            fixture.sites.find((s) => s.domain === name)?.category ??
            "productive";
          const base = (weekend ? weekendMin : weekdayMin) * 60 * drift;
          const secs = Math.round(base * (0.3 + rand(`${day}:${name}`) * 1.2));
          return secs >= 60 ? [toItem(target, category, secs)] : [];
        });
  }
  recorded.set(day, items);
  return items;
}

function totalsOf(day: string, items: DayItem[]): DayTotals {
  const sum = (c: Category) =>
    items.filter((i) => i.category === c).reduce((s, i) => s + i.secs, 0);
  return { day, productiveSecs: sum("productive"), entertainmentSecs: sum("entertainment") };
}

const keyOf = (i: { target: TargetRef; category: Category }) =>
  `${i.target.kind}:${i.target.id}:${i.category}`;
const bySecs = (a: DayItem, b: DayItem) => b.secs - a.secs;

function getDay(day: string | null): DaySummary {
  const d = day ?? TODAY;
  const items = [...recordedOn(d)];
  if (d === TODAY) {
    // Every tracked target appears today under its current Category, even at 0s.
    const seen = new Set(items.map(keyOf));
    const current = [
      ...apps.flatMap((a) =>
        a.category ? [toItem({ kind: "app", id: a.id }, a.category, 0)] : [],
      ),
      ...sites.flatMap((s) =>
        s.category ? [toItem({ kind: "site", id: s.id }, s.category, 0)] : [],
      ),
    ];
    items.push(...current.filter((i) => !seen.has(keyOf(i))));
  }
  return {
    totals: totalsOf(d, items),
    goalSecs: settings.dailyGoalMinutes * 60,
    isToday: d === TODAY,
    items: items.sort(bySecs),
  };
}

const currentMonday = addDays(TODAY, -weekday(TODAY));

function getWeek(start: string | null): WeekSummary {
  const s = start ?? currentMonday;
  if (weekday(s) !== 0) throw apiError(`${s} is not a Monday`);
  return {
    start: s,
    isCurrent: s === currentMonday,
    goalSecs: settings.dailyGoalMinutes * 60,
    days: Array.from({ length: 7 }, (_, i) => {
      const day = addDays(s, i);
      return totalsOf(day, recordedOn(day));
    }),
  };
}

function getShare(range: ShareRange): Share {
  const from =
    range === "week" ? currentMonday : range === "month" ? `${TODAY.slice(0, 8)}01` : fixture.since;
  const merged = new Map<string, DayItem>();
  for (let day = from; day <= TODAY; day = addDays(day, 1)) {
    for (const item of day === TODAY ? getDay(TODAY).items : recordedOn(day)) {
      const prev = merged.get(keyOf(item));
      merged.set(
        keyOf(item),
        prev
          ? {
              ...prev,
              secs: prev.secs + item.secs,
              minimizedSecs: prev.minimizedSecs + item.minimizedSecs,
            }
          : item,
      );
    }
  }
  const items = [...merged.values()].filter((i) => i.secs > 0).sort(bySecs);
  const totals = totalsOf("", items);
  return {
    productiveSecs: totals.productiveSecs,
    entertainmentSecs: totals.entertainmentSecs,
    items,
  };
}

function getYear(year: number | null): YearHeatmap {
  const y = year ?? Number(TODAY.slice(0, 4));
  const days: DayTotals[] = [];
  for (let day = `${y}-01-01`; day.startsWith(String(y)); day = addDays(day, 1)) {
    days.push(totalsOf(day, recordedOn(day)));
  }
  return {
    year: y,
    isCurrent: TODAY.startsWith(String(y)),
    goalSecs: settings.dailyGoalMinutes * 60,
    days,
  };
}

// ---- Mutations ---------------------------------------------------------------------------------

function normalizeDomain(input: string): string {
  const host = (
    input
      .trim()
      .toLowerCase()
      .replace(/^[a-z][a-z0-9+.-]*:\/\//, "")
      .split(/[/?#]/)[0] ?? ""
  )
    .replace(/:\d+$/, "")
    .replace(/^www\./, "")
    .replace(/\.$/, "");
  if (!/^([a-z0-9-]+\.)+[a-z]{2,}$/.test(host)) {
    throw apiError(`“${input.trim()}” isn't a web address. Try one like youtube.com.`);
  }
  return host;
}

function upsertSite(domain: string, keyword: string | null, category: Category | null): Site {
  const normalized = normalizeDomain(domain);
  // As the backend: several comma-separated keywords, each of at least 3 characters.
  const keywords = keyword?.split(",").map((k) => k.trim()) ?? null;
  if (keywords?.some((k) => k.length < 3)) {
    throw apiError("A keyword needs at least 3 characters.");
  }
  const trimmed = keywords?.join(", ") ?? null;
  const existing = sites.find((s) => s.domain === normalized);
  if (existing) {
    // Assumed backend rule: keyword None on update keeps the current keyword.
    existing.keyword = trimmed ?? existing.keyword;
    existing.category = category;
    return { ...existing };
  }
  const label = normalized.split(".")[0] ?? "";
  const site: Site = {
    id: sites.length + 1,
    domain: normalized,
    keyword: trimmed ?? (label.length >= 3 ? label : null),
    category,
  };
  sites.push(site);
  return { ...site };
}

function setAppCategory(identity: AppIdentity, name: string, category: Category | null) {
  const app = apps.find(
    (a) => a.identity.kind === identity.kind && a.identity.key === identity.key,
  );
  if (app) app.category = category;
  else
    apps.push({
      id: apps.length + 1,
      identity,
      name,
      iconPath: null,
      isBrowser: false,
      isRunning: false,
      category,
    });
  // Today's 0s rows depend on what is tracked now.
  recorded.delete(TODAY);
  return null;
}

/**
 * The tracker is counting the fixture's first item of today, while it is still tracked. With
 * ?waiting=1 it is the first app that waited in today instead (Claude), counting as waiting credit.
 */
function tracking(): NowTracking | null {
  const waiting = params.get("waiting") === "1";
  const name = (waiting ? fixture.today.find(([, , , w]) => w) : fixture.today[0])?.[0];
  const app = apps.find((x) => x.name === name);
  if (app?.category) {
    const { name, iconPath, category, isBrowser } = app;
    return { name, iconPath, category, isBrowser, waiting };
  }
  const site = sites.find((x) => x.domain === name);
  if (site?.category) {
    return {
      name: site.domain,
      iconPath: null,
      category: site.category,
      isBrowser: false,
      waiting: false,
    };
  }
  return null;
}

/** The status line's state, in the backend's order: a failed save, a pause, away, tracking. */
function getStatus(): TrackerStatus {
  const forced = params.get("status");
  if (forced === "saveFailed") return { state: "saveFailed" };
  if (pausedUntil) return { state: "paused" };
  // The Away trim just removed 6m 12s of idle time, or under a minute of it.
  if (forced === "away") return { state: "away", removedSecs: 372 };
  if (forced === "awayBrief") return { state: "away", removedSecs: 40 };
  const now = forced === "untracked" ? null : tracking();
  return now ? { state: "tracking", ...now } : { state: "untracked" };
}

/** Back to first run, as the backend's wipe: nothing tracked, no history, default settings. */
function deleteAllData() {
  wiped = true;
  recorded.clear();
  for (const app of apps) app.category = null;
  sites.length = 0;
  pausedUntil = null;
  Object.assign(settings, {
    dailyGoalMinutes: 360,
    launchAtLogin: false,
    onboarded: false,
    appearance: "glass",
    awayAfterMinutes: 5,
    waitingCredit: false,
  } satisfies Settings);
  return null;
}

// ---- IPC ---------------------------------------------------------------------------------------

type Args = Record<string, unknown>;

const str = (v: unknown) => (typeof v === "string" ? v : null);
const num = (v: unknown) => (typeof v === "number" ? v : null);
const category = (v: unknown): Category | null =>
  v === "productive" || v === "entertainment" ? v : null;
function identity(v: unknown): AppIdentity {
  if (
    typeof v === "object" &&
    v !== null &&
    "kind" in v &&
    "key" in v &&
    typeof v.key === "string" &&
    (v.kind === "exe" || v.kind === "aumid")
  ) {
    return { kind: v.kind, key: v.key };
  }
  throw apiError("identity must be { kind, key }");
}
const range = (v: unknown): ShareRange => (v === "month" || v === "all" ? v : "week");
const appearance = (v: unknown): Appearance => (v === "solid" ? "solid" : "glass");
const exportFormat = (v: unknown): ExportFormat => (v === "json" ? "json" : "csv");

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function handle(cmd: string, a: Args): Promise<unknown> {
  if (cmd === failing) throw apiError(`Mock failure for ${cmd}.`);
  if (cmd === slow) return new Promise(() => {});
  switch (cmd) {
    case "get_day":
      return getDay(str(a.day)) satisfies DaySummary;
    case "get_week":
      return getWeek(str(a.start)) satisfies WeekSummary;
    case "get_share":
      return getShare(range(a.range)) satisfies Share;
    case "get_year":
      return getYear(num(a.year)) satisfies YearHeatmap;
    case "list_catalog":
      // The real first listing extracts icons; a short wait shows the static loading state.
      await wait(250);
      return apps.map(({ id: _, ...app }) => app) satisfies CatalogApp[];
    case "set_app_category":
      return setAppCategory(identity(a.identity), str(a.name) ?? "", category(a.category));
    case "list_sites":
      return [...sites].sort((x, y) => x.domain.localeCompare(y.domain)) satisfies Site[];
    case "upsert_site":
      return upsertSite(str(a.domain) ?? "", str(a.keyword), category(a.category)) satisfies Site;
    case "get_settings":
      return { ...settings } satisfies Settings;
    case "set_daily_goal": {
      const minutes = num(a.minutes) ?? 0;
      if (minutes < 30 || minutes > 960) throw apiError("The goal must be 30 minutes to 16 hours.");
      settings.dailyGoalMinutes = minutes;
      return null;
    }
    case "set_launch_at_login":
      settings.launchAtLogin = a.enabled === true;
      return null;
    case "complete_onboarding":
      settings.onboarded = true;
      settings.launchAtLogin = true;
      return null;
    case "get_status":
      return getStatus() satisfies TrackerStatus | null;
    case "set_appearance":
      settings.appearance = appearance(a.appearance);
      return null;
    case "glass_available":
      return glass satisfies boolean;
    case "set_away_after": {
      const minutes = num(a.minutes) ?? 0;
      if (![2, 5, 10, 15].includes(minutes)) {
        throw apiError("Away can start after 2, 5, 10 or 15 minutes.");
      }
      settings.awayAfterMinutes = minutes;
      return null;
    }
    case "set_waiting_credit":
      settings.waitingCredit = a.enabled === true;
      return null;
    case "get_pause":
      return pausedUntil satisfies string | null;
    case "resume_tracking":
      pausedUntil = null;
      void emit("pause-changed");
      return null;
    case "export_data":
      // The user picked a file in the save dialog.
      return `C:\\Users\\you\\Documents\\productivity-tracker-${TODAY}.${exportFormat(a.format)}` satisfies
        | string
        | null;
    case "open_data_folder":
      return null;
    case "delete_all_data":
      return deleteAllData();
    case "plugin:app|version":
      return "0.1.0";
    case "plugin:window|destroy":
      // The real window is gone; leave an empty canvas.
      document.body.replaceChildren();
      return null;
    case "plugin:window|toggle_maximize":
      maximized = !maximized;
      void emit("tauri://resize", { width: innerWidth, height: innerHeight });
      return null;
    case "plugin:window|is_maximized":
      return maximized;
    case "plugin:window|minimize":
    case "plugin:window|start_dragging":
      return null;
    default:
      throw apiError(`mock: unknown command ${cmd}`);
  }
}

mockWindows("main");
mockConvertFileSrc("windows");
mockIPC(
  (cmd, payload) => {
    const args: Args =
      payload &&
      !Array.isArray(payload) &&
      !(payload instanceof ArrayBuffer) &&
      !(payload instanceof Uint8Array)
        ? payload
        : {};
    return handle(cmd, args);
  },
  { shouldMockEvents: true },
);

// Browser focus stands in for window focus, so the surface switch can be tried in dev.
window.addEventListener("focus", () => void emit("tauri://focus"));
window.addEventListener("blur", () => void emit("tauri://blur"));
document.documentElement.dataset.wallpaper = "";
