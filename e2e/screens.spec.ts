// One screenshot per view with its most telling fixture, plus the Day view on both surfaces.
// Written to e2e/screenshots/ for review; nothing is compared. Window sizes are in responsive.spec.ts.
import { expect, open, shot, test } from "./fixtures";

test("day, glass and solid", async ({ page }) => {
  await open(page, "?surface=glass");
  await expect(page.locator("html")).toHaveAttribute("data-surface", "glass");
  await expect(page.getByText("Tracking · Visual Studio Code")).toBeVisible();
  await shot(page, "day-glass");

  await open(page, "?surface=solid");
  await expect(page.locator("html")).toHaveAttribute("data-surface", "solid");
  await expect(page.getByRole("img", { name: /^Productive/ })).toBeVisible();
  await shot(page, "day-solid");
});

test("day edge cases", async ({ page }) => {
  await open(page, "?fixture=empty");
  await expect(page.getByRole("button", { name: "Choose apps" })).toBeVisible();
  await shot(page, "day-empty");

  await open(page, "?fixture=categorySplit");
  await expect(page.getByRole("region", { name: "Entertainment" })).toContainText("Discord");
  await shot(page, "day-category-split");

  await open(page, "?fixture=longNames");
  await expect(page.getByRole("region", { name: "Productive" })).toBeVisible();
  await shot(page, "day-long-names");

  await open(page, "?fixture=browserAndSite");
  await expect(page.getByRole("region", { name: "Productive" })).toContainText("other sites");
  await page.getByRole("button", { name: "Previous day" }).click();
  await expect(page.getByRole("button", { name: "Back to today" })).toBeVisible();
  await shot(page, "day-past");
});

test("history charts", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "History" }).click();
  await expect(page.getByRole("group", { name: /^Week of/ })).toBeVisible();
  await shot(page, "history-bars-current");
  await page.getByRole("button", { name: "Previous week" }).click();
  await page.getByRole("button", { name: /^Thursday, October 15:/ }).hover();
  await shot(page, "history-bars");

  await page.getByRole("radiogroup", { name: "Chart" }).getByRole("radio", { name: "Pie" }).click();
  await expect(page.getByRole("img", { name: /^This week/ })).toBeVisible();
  await shot(page, "history-pie");

  await page
    .getByRole("radiogroup", { name: "Chart" })
    .getByRole("radio", { name: "Heatmap" })
    .click();
  await expect(page.getByRole("group", { name: /^2026/ })).toBeVisible();
  await shot(page, "history-heatmap");

  await open(page, "?fixture=empty");
  await page.getByRole("button", { name: "History" }).click();
  await expect(page.getByText("No tracked time this week")).toBeVisible();
  await shot(page, "history-empty");
});

test("apps and sites", async ({ page }) => {
  await open(page, "?fixture=browserAndSite");
  await page.getByRole("button", { name: "Apps", exact: true }).click();
  await page.getByRole("button", { name: /^Discord,/ }).click();
  await expect(page.getByRole("radiogroup", { name: "Category for Discord" })).toBeVisible();
  await shot(page, "apps-popover");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /^twitch\.tv,/ }).click();
  await expect(page.getByRole("textbox", { name: "Title keyword for twitch.tv" })).toBeVisible();
  await shot(page, "apps-site-popover");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /^Claude,/ }).click();
  await expect(page.getByRole("switch", { name: "Counts while I watch" })).toBeVisible();
  await shot(page, "apps-popover-watch");

  await open(page, "?fixture=longNames");
  await page.getByRole("button", { name: "Apps", exact: true }).click();
  await page.getByRole("button", { name: /^Show all \d+ installed apps/ }).click();
  await shot(page, "apps-long-names");

  await open(page, "?fixture=manyApps");
  await page.getByRole("button", { name: "Apps", exact: true }).click();
  await page.getByRole("searchbox", { name: "Search apps and sites" }).fill("micro");
  await expect(page.getByRole("button", { name: /^Microsoft Teams,/ })).toBeVisible();
  await shot(page, "apps-search");

  // list_catalog never answers here, so the loading state is held for the shot.
  await open(page, "?fixture=browserAndSite&surface=solid&slow=list_catalog");
  await page.getByRole("button", { name: "Apps", exact: true }).click();
  await expect(page.getByText("Looking for your apps…")).toBeVisible();
  await shot(page, "apps-solid-loading");
});

test("first run and settings", async ({ page }) => {
  await open(page, "?fixture=firstRun");
  for (const name of ["Visual Studio Code", "Figma", "Slack", "Windows Terminal", "github.com"]) {
    await page.getByRole("button", { name: new RegExp(`^${name},`) }).click();
  }
  await shot(page, "first-run-work");
  await page.getByRole("button", { name: "Continue" }).click();
  for (const name of ["Spotify", "Steam", "youtube.com"]) {
    await page.getByRole("button", { name: new RegExp(`^${name},`) }).click();
  }
  await shot(page, "first-run-fun");
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByRole("heading", { name: "Set a daily goal" })).toBeVisible();
  await shot(page, "first-run-goal");
  await page.getByRole("button", { name: "Start tracking" }).click();
  await expect(page.getByText("Tracking started.")).toBeVisible();
  await shot(page, "day-after-first-run");

  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByRole("switch", { name: "Launch at login" })).toBeVisible();
  await shot(page, "settings");
  await page.getByRole("region", { name: "About" }).scrollIntoViewIfNeeded();
  await shot(page, "settings-end");
  await page.getByRole("button", { name: "Delete…" }).click();
  await expect(page.getByRole("dialog", { name: "Delete all data?" })).toBeVisible();
  await shot(page, "settings-delete-dialog");
  await page.keyboard.press("Escape");

  // The whole page in one shot, and its height against the window's, for the navigation decision
  // (a section list only if it runs past about two window heights).
  const main = page.locator("main");
  const height = await main.evaluate((el) => el.scrollHeight);
  console.log(
    `Settings scroll height: ${height}px in a ${await main.evaluate((el) => el.clientHeight)}px view`,
  );
  await page.setViewportSize({ width: 1000, height: height + 40 });
  await shot(page, "settings-full");
});

test("day, paused from the tray", async ({ page }) => {
  await open(page, "?paused=1");
  await expect(page.getByText("Paused until 3:40 PM")).toBeVisible();
  await shot(page, "day-paused");
});

// The other status line states; "Tracking" is in day-glass and paused in day-paused.
test("day status line states", async ({ page }) => {
  const states = [
    ["saveFailed", "Can't save, retrying", "day-status-save-failed"],
    ["away", "Away · removed 6m idle time", "day-status-away"],
    ["awayBrief", "Away", "day-status-away-brief"],
    ["untracked", "Nothing tracked in front", "day-status-untracked"],
  ] as const;
  for (const [state, text, name] of states) {
    await open(page, `?status=${state}`);
    await expect(page.getByText(text, { exact: true })).toBeVisible();
    await shot(page, name);
  }
  await open(page, "?waiting=1");
  await expect(page.getByText("Tracking · Claude · waiting")).toBeVisible();
  await shot(page, "day-status-waiting");
  await open(page, "?watching=1");
  await expect(page.getByText("Tracking · Claude · watching")).toBeVisible();
  await shot(page, "day-status-watching");
});

test("minimum window size", async ({ page }) => {
  await page.setViewportSize({ width: 760, height: 520 });
  await open(page, "?fixture=longNames&surface=solid");
  await expect(page.getByRole("img", { name: /^Productive/ })).toBeVisible();
  await shot(page, "min-day-solid");

  await page.getByRole("button", { name: "History" }).click();
  await page.getByRole("button", { name: "Previous week" }).click();
  await shot(page, "min-history-bars");
  await page.getByRole("radiogroup", { name: "Chart" }).getByRole("radio", { name: "Pie" }).click();
  await shot(page, "min-history-pie");
  await page
    .getByRole("radiogroup", { name: "Chart" })
    .getByRole("radio", { name: "Heatmap" })
    .click();
  await shot(page, "min-history-heatmap");

  await page.getByRole("button", { name: "Apps", exact: true }).click();
  await expect(page.getByRole("region", { name: "Running now" })).toBeVisible();
  await shot(page, "min-apps");

  await open(page, "?fixture=firstRun");
  await expect(page.getByRole("region", { name: "Running now" })).toBeVisible();
  await shot(page, "min-first-run");
});
