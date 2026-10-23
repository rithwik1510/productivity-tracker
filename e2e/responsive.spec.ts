// Every view at the window sizes that matter (issue 14), into e2e/screenshots/responsive/ as
// <size>-<view>.png for review; nothing is compared. Each shot also checks the layout rules that
// are cheap to measure: nothing scrolls sideways, and nothing sits under the titlebar.
import type { Page } from "@playwright/test";
import { expect, open, test } from "./fixtures";

// Logical px. 760×520 is the minimum; 1093×576 is a 1366×768 laptop at 125%, maximized;
// 1000×680 is the default; the rest are common laptop and maximized desktop sizes.
const SIZES = [
  [760, 520],
  [1093, 576],
  [1000, 680],
  [1280, 800],
  [1440, 900],
  [1920, 1080],
  [2560, 1440],
  [3440, 1440],
] as const;

const TITLEBAR = 40;

/**
 * Fails if anything inside <main> scrolls sideways, the view starts under the titlebar, or the
 * titlebar's title (or first-run brand) doesn't line up with the view column.
 */
async function checkLayout(page: Page, where: string) {
  const problems = await page.evaluate((titlebar) => {
    const found: string[] = [];
    const main = document.querySelector("main");
    const column = main?.firstElementChild;
    if (!main || !column) return ["no <main> column"];
    for (const el of [main, ...main.querySelectorAll("*")]) {
      const style = getComputedStyle(el);
      const scrolls = style.overflowX === "auto" || style.overflowX === "scroll" || el === main;
      if (scrolls && el.scrollWidth > el.clientWidth + 1) {
        found.push(`${el.tagName}.${el.className.toString().slice(0, 60)} scrolls sideways`);
      }
    }
    if (main.getBoundingClientRect().top < titlebar) found.push("main starts under the titlebar");
    const title = document.querySelector("header h1, header span");
    const offset = (title?.getBoundingClientRect().left ?? 0) - column.getBoundingClientRect().left;
    if (Math.abs(offset) > 1) found.push(`the title is ${offset}px off the view column`);
    return found;
  }, TITLEBAR);
  expect(problems, where).toEqual([]);
}

async function shoot(page: Page, size: string, view: string) {
  await checkLayout(page, `${size} ${view}`);
  await page.screenshot({ path: `e2e/screenshots/responsive/${size}-${view}.png` });
}

const chart = (page: Page, name: string) =>
  page.getByRole("radiogroup", { name: "Chart" }).getByRole("radio", { name }).click();

for (const [width, height] of SIZES) {
  const size = `${width}x${height}`;

  test(`views at ${size}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await open(page);
    await expect(page.getByRole("img", { name: /^Productive/ })).toBeVisible();
    await expect(page.getByText("Tracking · Visual Studio Code")).toBeVisible();
    // Under about 900px the ring card is a strip above the list; otherwise they sit side by side.
    const ring = await page.getByRole("img", { name: /^Productive/ }).boundingBox();
    const list = await page.getByRole("region", { name: "Productive" }).boundingBox();
    const stacked = (list?.y ?? 0) > (ring?.y ?? 0) + (ring?.height ?? 0);
    expect(stacked, `${size}: Day stacks only when narrow`).toBe(width < 900);
    await shoot(page, size, "day");

    await page.getByRole("button", { name: "History" }).click();
    await page.getByRole("button", { name: "Previous week" }).click();
    await expect(page.getByRole("group", { name: /^Week of/ })).toBeVisible();
    await shoot(page, size, "history-bars");
    await chart(page, "Pie");
    await expect(page.getByRole("img", { name: /^This week/ })).toBeVisible();
    await shoot(page, size, "history-pie");
    await chart(page, "Heatmap");
    await expect(page.getByRole("group", { name: /^2026/ })).toBeVisible();
    await shoot(page, size, "history-heatmap");

    await page.getByRole("button", { name: "Apps", exact: true }).click();
    await page.getByRole("button", { name: /^Discord,/ }).click();
    await expect(page.getByRole("radiogroup", { name: "Category for Discord" })).toBeVisible();
    await shoot(page, size, "apps-popover");
    await page.keyboard.press("Escape");

    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByRole("switch", { name: "Launch at login" })).toBeVisible();
    await shoot(page, size, "settings");
    await page.getByRole("button", { name: "Delete…" }).click();
    await expect(page.getByRole("dialog", { name: "Delete all data?" })).toBeVisible();
    await shoot(page, size, "settings-delete");
  });

  test(`first run at ${size}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await open(page, "?fixture=firstRun");
    for (const name of ["Visual Studio Code", "Figma", "Slack", "github.com"]) {
      await page.getByRole("button", { name: new RegExp(`^${name},`) }).click();
    }
    await shoot(page, size, "first-run-work");
    await page.getByRole("button", { name: "Continue" }).click();
    await page.getByRole("button", { name: /^Spotify,/ }).click();
    await shoot(page, size, "first-run-fun");
    await page.getByRole("button", { name: "Continue" }).click();
    await expect(page.getByRole("heading", { name: "Set a daily goal" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Start tracking" })).toBeInViewport();
    await shoot(page, size, "first-run-goal");
  });
}
