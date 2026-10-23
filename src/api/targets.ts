// The picker's one list: catalog apps, saved sites and the preset sites not added yet, as Targets
// with one category setter, so the tile grid never branches on app vs site.
import type { ApiError, AppIdentity, CatalogApp, Category } from "./bindings";
import { useCatalog, useSetAppCategory, useSites, useUpsertSite } from "./queries";

// Curated keywords: "/ x" matches X's "… / X" titles without matching "Inbox" or "Excel". A keyword
// may list several, comma-separated: Reddit's home is "Reddit - …" and a post is "Title : r/sub".
const PRESETS = [
  { domain: "youtube.com", keyword: "youtube" },
  { domain: "netflix.com", keyword: "netflix" },
  { domain: "github.com", keyword: "github" },
  { domain: "x.com", keyword: "/ x" },
  { domain: "reddit.com", keyword: "reddit, : r/" },
  { domain: "twitch.tv", keyword: "twitch" },
  { domain: "linkedin.com", keyword: "linkedin" },
  { domain: "instagram.com", keyword: "instagram" },
] as const;

type Base = {
  key: string;
  name: string;
  /** Lowercase text the picker's search matches: the name, plus an app's identity (see below). */
  search: string;
  iconPath: string | null;
  category: Category | null;
};
type AppTarget = Base & { kind: "app"; app: CatalogApp };
type SiteTarget = Base & {
  kind: "site";
  domain: string;
  keyword: string | null;
  /** false = a preset shown as a tile but not stored yet; picking a category adds it. */
  saved: boolean;
};
export type Target = AppTarget | SiteTarget;

const byName = (a: Base, b: Base) => a.name.localeCompare(b.name);

/**
 * The searchable part of an App Identity: the exe name ("winword" for Word; "chrome helium" for a
 * Chromium fork keyed chrome.exe|helium), or a packaged app's name from its AUMID
 * ("windowsterminal" from Microsoft.WindowsTerminal_8wekyb3d8bbwe!App). The publisher and the
 * hash are left out, so "micro" doesn't match every Store app.
 */
function identityWords({ kind, key }: AppIdentity) {
  const lower = key.toLowerCase();
  return kind === "exe"
    ? lower.replace(/\.exe(\||$)/, " ").trim()
    : (lower.split("_")[0]?.split(".").at(-1) ?? "");
}

/**
 * Apps by name, then sites: the presets in their curated order (stored or not), then every other
 * stored site by domain. `apps` is undefined until the catalog arrives (its first call extracts
 * icons), while sites show at once.
 */
export function usePickerTargets() {
  const catalog = useCatalog();
  const stored = useSites();

  const apps = catalog.data
    ?.map(
      (app): AppTarget => ({
        kind: "app",
        key: `app:${app.identity.kind}:${app.identity.key}`,
        name: app.name,
        search: `${app.name.toLowerCase()} ${identityWords(app.identity)}`,
        iconPath: app.iconPath,
        category: app.category,
        app,
      }),
    )
    .sort(byName);

  const site = (domain: string, keyword: string | null): SiteTarget => {
    const row = stored.data?.find((s) => s.domain === domain);
    return {
      kind: "site",
      key: `site:${domain}`,
      name: domain,
      search: domain,
      iconPath: null,
      domain,
      category: row?.category ?? null,
      keyword: row ? row.keyword : keyword,
      saved: row !== undefined,
    };
  };
  const sites = [
    ...PRESETS.map((p) => site(p.domain, p.keyword)),
    ...(stored.data ?? [])
      .filter((s) => !PRESETS.some((p) => p.domain === s.domain))
      .map((s) => site(s.domain, s.keyword)),
  ];

  return {
    apps,
    sites,
    ready: catalog.data !== undefined && stored.data !== undefined,
    error: catalog.error ?? stored.error,
  };
}

/**
 * Sets any Target's category. Apps flip optimistically; sites share one mutation scope so a
 * keyword commit and the category click that blurred it reach the backend in that order.
 */
export function useSetTargetCategory() {
  const setApp = useSetAppCategory();
  const upsert = useUpsertSite({ id: "sites" });
  return (target: Target, category: Category | null, onError: (e: ApiError) => void) => {
    if (target.kind === "app") setApp.mutate({ app: target.app, category }, { onError });
    else
      upsert.mutate(
        // A stored site sends keyword null, which keeps its keyword; a new preset sends its own.
        { domain: target.domain, keyword: target.saved ? null : target.keyword, category },
        { onError },
      );
  };
}
