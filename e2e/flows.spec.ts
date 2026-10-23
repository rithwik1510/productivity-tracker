import type { Page } from "@playwright/test";
import { expect, open, test } from "./fixtures";

/** A picker tile by its name; its accessible name continues with the category. */
const tile = (page: Page, name: string) =>
  page.getByRole("button", { name: new RegExp(`^${name.replace(/\./g, "\\.")},`) });

const openApps = (page: Page) => page.getByRole("button", { name: "Apps", exact: true }).click();

test("first run: paint work apps, then fun ones, set the goal, land on today", async ({ page }) => {
  await open(page, "?fixture=firstRun");
  await expect(page.getByRole("heading", { name: "Tap the apps you work in" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Views" })).toHaveCount(0);

  const next = page.getByRole("button", { name: "Continue" });
  await expect(next).toBeDisabled();
  await tile(page, "Visual Studio Code").click();
  await tile(page, "Discord").click();
  await expect(tile(page, "Discord")).toHaveAttribute("aria-pressed", "true");
  // Tapping again undoes.
  await tile(page, "Discord").click();
  await expect(tile(page, "Discord")).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByRole("status", { name: "Productive: 1" })).toBeVisible();
  await next.click();

  await expect(
    page.getByRole("heading", { name: "Now tap the ones that are for fun" }),
  ).toBeVisible();
  // A blue tile tapped here moves to Entertainment; a preset site is added on its first tap.
  await tile(page, "youtube.com").click();
  await tile(page, "Visual Studio Code").click();
  await expect(page.getByRole("status", { name: "Entertainment: 2" })).toBeVisible();
  await tile(page, "Visual Studio Code").click();
  await expect(page.getByRole("status", { name: "Productive: 0" })).toBeVisible();
  await page.getByRole("button", { name: "Back" }).click();
  await tile(page, "Visual Studio Code").click();
  await next.click();
  await next.click();

  await expect(page.getByText("Tracking 1 app and 1 site.")).toBeVisible();
  await page.getByRole("button", { name: "8h" }).click();
  await expect(page.getByRole("group", { name: "Daily goal" })).toContainText("8h");
  await page.getByRole("button", { name: "Start tracking" }).click();

  await expect(page.getByRole("heading", { name: "Today" })).toBeVisible();
  await expect(page.getByRole("img", { name: /of a 8h goal/ })).toBeVisible();
  await expect(page.getByText("Tracking started.")).toBeVisible();
  // Newly tracked, so both are listed at 0m.
  await expect(page.getByRole("region", { name: "Productive" })).toContainText(
    "Visual Studio Code",
  );
  await expect(page.getByRole("region", { name: "Entertainment" })).toContainText("youtube.com");
});

test("a tile's popover changes its category, and a refusal rolls it back", async ({ page }) => {
  await open(page);
  await openApps(page);
  await tile(page, "Discord").click();
  await page
    .getByRole("radiogroup", { name: "Category for Discord" })
    .getByRole("radio", { name: "Entertainment" })
    .click();
  // An app's popover closes on a pick; the tile keeps its place, now tinted.
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(tile(page, "Discord")).toHaveAccessibleName("Discord, Entertainment");
  await expect(tile(page, "Discord")).toBeFocused();
  await page.getByRole("button", { name: "Today", exact: true }).click();
  await expect(page.getByRole("region", { name: "Entertainment" })).toContainText("Discord");

  await open(page, "?fail=set_app_category");
  await openApps(page);
  await tile(page, "Figma").click();
  await page
    .getByRole("radiogroup", { name: "Category for Figma" })
    .getByRole("radio", { name: "Off" })
    .click();
  await expect(page.getByRole("alert")).toContainText("Couldn't change Figma");
  await expect(tile(page, "Figma")).toHaveAccessibleName("Figma, Productive");
});

test("adding a site, with domain and keyword validation", async ({ page }) => {
  await open(page);
  await openApps(page);
  await page.getByRole("button", { name: "Add a site…" }).click();
  const address = page.getByRole("textbox", { name: "Site address" });
  await address.fill("not a domain");
  await address.press("Enter");
  await expect(page.getByRole("alert")).toContainText("isn't a web address");
  await expect(address).toHaveAttribute("aria-invalid", "true");

  // A new site lands at Off with its popover open on it.
  await address.fill("news.ycombinator.com");
  await address.press("Enter");
  const category = page.getByRole("radiogroup", { name: "Category for news.ycombinator.com" });
  await expect(category.getByRole("radio", { name: "Off" })).toBeChecked();
  const keyword = page.getByRole("textbox", { name: "Title keyword for news.ycombinator.com" });
  await expect(keyword).toHaveValue("news");
  await keyword.fill("hn");
  await keyword.press("Enter");
  await expect(page.getByRole("dialog")).toContainText("A keyword needs at least 3 characters.");
  await keyword.fill("hacker news");
  await keyword.press("Enter");
  await expect(page.getByRole("dialog")).toHaveCount(0);

  // Typing an added site's address doesn't re-add it at Off: it opens the existing one.
  await page.getByRole("button", { name: "Add a site…" }).click();
  await address.fill("https://www.GitHub.com/pulls");
  await address.press("Enter");
  await expect(
    page
      .getByRole("radiogroup", { name: "Category for github.com" })
      .getByRole("radio", { name: "Productive" }),
  ).toBeChecked();
});

test("search offers to add a typed domain", async ({ page }) => {
  await open(page);
  await openApps(page);
  const search = page.getByRole("searchbox", { name: "Search apps and sites" });
  await search.fill("micro");
  await expect(page.getByRole("region", { name: "Results" })).toContainText("Microsoft Edge");
  await search.fill("github.com");
  await expect(page.getByRole("button", { name: "Add github.com" })).toHaveCount(0);
  await search.fill("hn.algolia.com");
  await page.getByRole("button", { name: "Add hn.algolia.com" }).click();
  await expect(search).toHaveValue("");
  await expect(page.getByRole("dialog", { name: "hn.algolia.com" })).toBeVisible();
});

test("search also matches an app's exe or package name", async ({ page }) => {
  await open(page);
  await openApps(page);
  const search = page.getByRole("searchbox", { name: "Search apps and sites" });
  const results = page.getByRole("region", { name: "Results" });
  // winword.exe: the exe name the tracker knows Microsoft Word by.
  await search.fill("winword");
  await expect(tile(page, "Microsoft Word")).toBeVisible();
  // A Store app by its package name; its publisher and hash don't match.
  await search.fill("windowscalc");
  await expect(tile(page, "Calculator")).toBeVisible();
  await search.fill("8wekyb3d8bbwe");
  await expect(results).toContainText("No apps or sites match");
});

test("a keyword edit survives the category click that commits it", async ({ page }) => {
  await open(page);
  await openApps(page);
  await tile(page, "youtube.com").click();
  await page.getByRole("textbox", { name: "Title keyword for youtube.com" }).fill("youtube music");
  // Blur (the keyword commit) and the category click in one task, like a click that lands before
  // the commit's reply. Playwright's own click yields in between, which hides the race.
  await page.evaluate(() => {
    const radio = [
      ...document.querySelectorAll<HTMLElement>(
        '[aria-label="Category for youtube.com"] [role="radio"]',
      ),
    ].find((r) => r.textContent === "Productive");
    document.querySelector<HTMLElement>('[aria-label="Title keyword for youtube.com"]')?.blur();
    radio?.click();
  });

  // Reopen from what the backend stored.
  await page.getByRole("button", { name: "Today", exact: true }).click();
  await openApps(page);
  await tile(page, "youtube.com").click();
  await expect(page.getByRole("textbox", { name: "Title keyword for youtube.com" })).toHaveValue(
    "youtube music",
  );
  await expect(
    page
      .getByRole("radiogroup", { name: "Category for youtube.com" })
      .getByRole("radio", { name: "Productive" }),
  ).toBeChecked();
});

test("tiles are one tab stop per section; arrows move, P / E / Backspace set", async ({ page }) => {
  await open(page);
  await openApps(page);
  const running = page.getByRole("group", { name: "Running now" });
  await running.getByRole("button").first().focus();
  await expect(tile(page, "Discord")).toBeFocused();

  await page.keyboard.press("ArrowRight");
  await expect(tile(page, "Google Chrome")).toBeFocused();
  await page.keyboard.press("e");
  await expect(tile(page, "Google Chrome")).toHaveAccessibleName(
    "Google Chrome, Entertainment, browser",
  );
  await page.keyboard.press("p");
  await expect(tile(page, "Google Chrome")).toHaveAccessibleName(
    "Google Chrome, Productive, browser",
  );
  await page.keyboard.press("Backspace");
  await expect(tile(page, "Google Chrome")).toHaveAccessibleName(
    "Google Chrome, Not tracked, browser",
  );

  // Tab leaves the section from the tile it was on, and comes back to it.
  await page.keyboard.press("Tab");
  await expect(tile(page, "netflix.com")).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(tile(page, "Google Chrome")).toBeFocused();
});

test("Appearance switches the surface between glass and solid", async ({ page }) => {
  await open(page);
  const html = page.locator("html");
  await expect(html).toHaveAttribute("data-surface", "glass");
  await page.getByRole("button", { name: "Settings" }).click();
  const appearance = page.getByRole("radiogroup", { name: "Appearance" });
  await appearance.getByRole("radio", { name: "Solid" }).click();
  await expect(html).toHaveAttribute("data-surface", "solid");

  // A stored Solid holds even while glass is available and the window is focused.
  await open(page, "?appearance=solid");
  await expect(html).toHaveAttribute("data-surface", "solid");
  await page.getByRole("button", { name: "Settings" }).click();
  await appearance.getByRole("radio", { name: "Glass" }).click();
  await expect(html).toHaveAttribute("data-surface", "glass");
});

test("History toggles across bars, pie and heatmap", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "History" }).click();
  const chart = page.getByRole("radiogroup", { name: "Chart" });
  await expect(page.getByRole("group", { name: /^Week of/ })).toBeVisible();

  await chart.getByRole("radio", { name: "Pie" }).click();
  await expect(page.getByRole("img", { name: /^This week: Productive/ })).toBeVisible();
  await page
    .getByRole("radiogroup", { name: "Range" })
    .getByRole("radio", { name: "All time" })
    .click();
  await expect(page.getByRole("img", { name: /^All time: Productive/ })).toBeVisible();

  await chart.getByRole("radio", { name: "Heatmap" }).click();
  await expect(page.getByRole("group", { name: /^2026: goal met on \d+ days/ })).toBeVisible();
  await expect(page.getByRole("group", { name: /^Week of/ })).toHaveCount(0);
});

test("clicking a heatmap Day opens it in the Day view", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "History" }).click();
  await page
    .getByRole("radiogroup", { name: "Chart" })
    .getByRole("radio", { name: "Heatmap" })
    .click();
  await page.getByRole("button", { name: /^Wednesday, October 14:/ }).click();
  await expect(page.getByRole("heading", { name: "Wednesday, October 14" })).toBeVisible();
  await expect(page.getByRole("img", { name: /^Productive/ })).toBeVisible();
  // Only today shows what is being counted now.
  await expect(page.getByText(/^Tracking ·/)).toHaveCount(0);

  // History comes back where it was left.
  await page.getByRole("button", { name: "History" }).click();
  await expect(page.getByRole("group", { name: /^2026: goal met/ })).toBeVisible();
});

test("Day arrows page through Days and back to today", async ({ page }) => {
  await open(page);
  const prev = page.getByRole("button", { name: "Previous day" });
  const next = page.getByRole("button", { name: "Next day" });
  await expect(page.getByRole("heading", { name: "Today" })).toBeVisible();
  await expect(next).toBeDisabled();

  await prev.click();
  await prev.click();
  await expect(page.getByRole("heading", { name: "Monday, October 19" })).toBeVisible();
  await next.click();
  await expect(page.getByRole("heading", { name: "Tuesday, October 20" })).toBeVisible();
  await next.click();
  await expect(page.getByRole("heading", { name: "Today" })).toBeVisible();
  await expect(next).toBeDisabled();

  await prev.click();
  await page.getByRole("button", { name: "Back to today" }).click();
  await expect(page.getByRole("heading", { name: "Today" })).toBeVisible();
});

test("a Day row notes the waiting part of its time", async ({ page }) => {
  await open(page);
  const productive = page.getByRole("region", { name: "Productive" });
  // The full row text is the name's tooltip.
  await expect(productive.getByTitle("Claude · incl. 41m waiting")).toBeVisible();
  await expect(productive.getByTitle("Windows Terminal · incl. 17m waiting")).toBeVisible();
  // Under a minute of waiting, and none, both read plainly.
  await expect(productive.getByTitle("Slack", { exact: true })).toBeVisible();
  await expect(productive.getByTitle("Visual Studio Code", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Previous day" }).click();
  await expect(page.getByRole("button", { name: "Back to today" })).toBeVisible();
  await expect(page.getByText(/waiting/)).toHaveCount(0);
});

test("the live line says when its app is waiting in, minimized", async ({ page }) => {
  await open(page, "?waiting=1");
  await expect(page.getByText("Tracking · Claude · waiting")).toBeVisible();
  await open(page);
  await expect(page.getByText("Tracking · Visual Studio Code", { exact: true })).toBeVisible();
});

test("Maximize follows the window's state, however it changes", async ({ page }) => {
  await open(page);
  const maximize = page.getByRole("button", { name: "Maximize" });
  const restore = page.getByRole("button", { name: "Restore down" });
  await maximize.click();
  await expect(restore).toBeVisible();
  await restore.click();
  await expect(maximize).toBeVisible();

  // A double press on the bar's empty part toggles, like a native caption.
  await page.locator("header").dblclick({ position: { x: 600, y: 20 } });
  await expect(restore).toBeVisible();

  // Win+Up or Aero Snap: the state changes outside the UI and only a resize event says so.
  await page.evaluate(() =>
    (
      window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string) => Promise<unknown> } }
    ).__TAURI_INTERNALS__.invoke("plugin:window|toggle_maximize"),
  );
  await expect(maximize).toBeVisible();
});

test("the rail thumb sits exactly on the current view's item", async ({ page }) => {
  await open(page);
  const rail = page.getByRole("navigation", { name: "Views" });
  for (const name of ["Today", "History", "Apps", "Settings"]) {
    const item = rail.getByRole("button", { name, exact: true });
    await item.click();
    await expect(item).toHaveAttribute("aria-current", "page");
    const thumb = item.locator("> span[aria-hidden]");
    expect(await thumb.boundingBox()).toEqual(await item.boundingBox());
  }
});

const openSettings = (page: Page) => page.getByRole("button", { name: "Settings" }).click();

test("Settings: the Tracking choices save, and a refusal rolls back", async ({ page }) => {
  await open(page);
  await openSettings(page);
  const tracking = page.getByRole("region", { name: "Tracking" });
  const away = tracking.getByRole("radiogroup", { name: "Count as away after" });
  await expect(away.getByRole("radio", { name: "5 min", exact: true })).toBeChecked();
  await away.getByRole("radio", { name: "15 min", exact: true }).click();
  await expect(away.getByRole("radio", { name: "15 min", exact: true })).toBeChecked();
  const minimized = tracking.getByRole("switch", { name: "Keep counting minimized apps" });
  await expect(minimized).toBeChecked();
  await minimized.click();
  await expect(minimized).not.toBeChecked();
  const launch = page.getByRole("switch", { name: "Launch at login" });
  await launch.click();
  await expect(launch).not.toBeChecked();
  await expect(page.getByText("Closing the window keeps tracking in the tray.")).toBeVisible();

  // The settings came from the backend, not just the cache: a reload of the cache shows them.
  await page.getByRole("button", { name: "Today", exact: true }).click();
  await openSettings(page);
  await expect(away.getByRole("radio", { name: "15 min", exact: true })).toBeChecked();
  await expect(minimized).not.toBeChecked();

  // The row leads to Apps & sites.
  await tracking.getByRole("button", { name: /^Tracked apps & sites/ }).click();
  await expect(page.getByRole("searchbox", { name: "Search apps and sites" })).toBeVisible();

  await open(page, "?fail=set_away_after");
  await openSettings(page);
  await away.getByRole("radio", { name: "2 min", exact: true }).click();
  await expect(tracking.getByRole("alert")).toContainText("Couldn't save that.");
  await expect(away.getByRole("radio", { name: "5 min", exact: true })).toBeChecked();
});

test("Settings: export asks the backend for each format; About shows the version", async ({
  page,
}) => {
  await open(page);
  await openSettings(page);
  const data = page.getByRole("region", { name: "Data" });
  await expect(data).toContainText("Everything stays on this PC.");
  await data.getByRole("button", { name: "Export CSV" }).click();
  await expect(data.getByRole("status")).toHaveText(
    String.raw`Saved to C:\Users\you\Documents\productivity-tracker-2026-10-21.csv`,
  );
  await data.getByRole("button", { name: "Export JSON" }).click();
  await expect(data.getByRole("status")).toHaveText(/productivity-tracker-2026-10-21\.json$/);
  await data.getByRole("button", { name: "Open folder" }).click();
  await expect(data.getByRole("alert")).toHaveCount(0);

  const about = page.getByRole("region", { name: "About" });
  await expect(about).toContainText("0.1.0");
  await expect(about).toContainText("MIT License");

  await open(page, "?fail=export_data");
  await openSettings(page);
  await data.getByRole("button", { name: "Export CSV" }).click();
  await expect(data.getByRole("alert")).toContainText("Mock failure for export_data.");
});

test("Delete all data: cancel keeps everything, confirm starts over at first run", async ({
  page,
}) => {
  await open(page);
  await openSettings(page);
  const deleteButton = page.getByRole("button", { name: "Delete…" });
  const dialog = page.getByRole("dialog", { name: "Delete all data?" });

  await deleteButton.click();
  await expect(dialog).toBeVisible();
  // The safe choice has focus, so Enter can't delete.
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await deleteButton.click();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  await page.getByRole("button", { name: "Today", exact: true }).click();
  await expect(page.getByRole("region", { name: "Productive" })).toContainText("Claude");

  await openSettings(page);
  await deleteButton.click();
  await dialog.getByRole("button", { name: "Delete everything" }).click();
  await expect(page.getByRole("heading", { name: "Tap the apps you work in" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Views" })).toHaveCount(0);
  await expect(page.getByRole("status", { name: "Productive: 0" })).toBeVisible();

  // A fresh start lands on today with nothing from before.
  await tile(page, "Visual Studio Code").click();
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByRole("button", { name: "Start tracking" }).click();
  await expect(page.getByRole("heading", { name: "Today" })).toBeVisible();
  await expect(page.getByText("Tracking started.")).toBeVisible();
  await expect(page.getByRole("region", { name: "Productive" })).not.toContainText("Claude");
  await page.getByRole("button", { name: "Previous day" }).click();
  await expect(page.getByText("Nothing was tracked on this day.")).toBeVisible();
});

test("paused from the tray: today's live line says until when, and Resume ends it", async ({
  page,
}) => {
  await open(page, "?paused=1");
  await expect(page.getByText("Paused until 3:40 PM")).toBeVisible();
  await expect(page.getByText(/^Tracking ·/)).toHaveCount(0);
  // Only today shows it.
  await page.getByRole("button", { name: "Previous day" }).click();
  await expect(page.getByText(/^Paused until/)).toHaveCount(0);
  await page.getByRole("button", { name: "Back to today" }).click();

  await page.getByRole("button", { name: "Resume" }).click();
  await expect(page.getByText(/^Paused until/)).toHaveCount(0);
  await expect(page.getByText("Tracking · Visual Studio Code")).toBeVisible();
});

test("the status line shows one state at a time, in the backend's order", async ({ page }) => {
  const line = (text: string) => page.getByText(text, { exact: true });

  await open(page, "?status=saveFailed");
  await expect(page.getByRole("status")).toHaveText("Can't save, retrying");
  await expect(page.getByText(/^Tracking ·/)).toHaveCount(0);
  // A failed save outranks a pause: the time isn't being saved either way.
  await open(page, "?status=saveFailed&paused=1");
  await expect(page.getByRole("status")).toHaveText("Can't save, retrying");
  await expect(page.getByText(/^Paused until/)).toHaveCount(0);

  // Away says why a total just dropped, once the trim removed a minute or more.
  await open(page, "?status=away");
  await expect(line("Away · removed 6m idle time")).toBeVisible();
  await open(page, "?status=awayBrief");
  await expect(line("Away")).toBeVisible();
  await expect(page.getByText(/removed/)).toHaveCount(0);

  // There, with nothing tracked in front: never the untracked app's name.
  await open(page, "?status=untracked");
  await expect(line("Nothing tracked in front")).toBeVisible();
  await expect(page.getByText(/^Tracking ·/)).toHaveCount(0);
  // Only today has a status line.
  await page.getByRole("button", { name: "Previous day" }).click();
  await expect(page.getByRole("button", { name: "Back to today" })).toBeVisible();
  await expect(line("Nothing tracked in front")).toHaveCount(0);
});

test.describe("view switches, with motion on", () => {
  test.use({ contextOptions: { reducedMotion: "no-preference" } });

  test("never draw a blank, faded or half-loaded frame, and keep the title in step", async ({
    page,
  }) => {
    await open(page);
    const switches = [
      ["History", "History"],
      ["Settings", "Settings"],
      ["Today", "Today"],
      ["History", "History"],
    ] as const;
    for (const [button, title] of switches) {
      // The render right after the press, then the next frames: the view's opacity, how much it
      // shows, and the title.
      const frames = await page.evaluate(
        (button) =>
          new Promise<{ opacity: number; text: number; title: string }[]>((resolve) => {
            const rail = document.querySelector('nav[aria-label="Views"]');
            const press = [...(rail?.querySelectorAll("button") ?? [])].find(
              (b) => b.textContent === button,
            );
            const out: { opacity: number; text: number; title: string }[] = [];
            const sample = () => {
              const view = document.querySelector<HTMLElement>("main > div > div");
              out.push({
                opacity: view ? Number(getComputedStyle(view).opacity) : 0,
                text: (view?.textContent ?? "").trim().length,
                title: document.querySelector("header")?.textContent ?? "",
              });
            };
            const step = () => {
              sample();
              if (out.length < 20) requestAnimationFrame(step);
              else resolve(out);
            };
            press?.click();
            // React commits a click's render in a microtask, before any query could answer (their
            // results land in a later task), so this is what the first painted frame starts from.
            queueMicrotask(sample);
            requestAnimationFrame(step);
          }),
        button,
      );
      const final = frames.at(-1)?.text ?? 0;
      for (const frame of frames) {
        expect(frame.opacity).toBe(1);
        // History is visited cold first: its data was prefetched, so nothing pops in later.
        expect(frame.text).toBeGreaterThanOrEqual(final * 0.8);
        expect(frame.title.startsWith(title)).toBe(true);
      }
    }
  });
});
