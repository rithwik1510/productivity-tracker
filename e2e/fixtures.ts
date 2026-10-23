import { test as base, expect, type Page } from "@playwright/test";

// The mock hands out icon paths under <data_dir>/icons, which convertFileSrc turns into
// http://asset.localhost/... URLs. Serve a simple generated icon for each, so screenshots show real
// <img> icons. obs64 deliberately 404s to exercise the monogram fallback.
function iconSvg(slug: string) {
  let hue = 0;
  for (const ch of slug) hue = (hue * 31 + ch.charCodeAt(0)) % 360;
  const letter = (slug.replace(/^microsoft-/, "")[0] ?? "?").toUpperCase();
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="hsl(${hue} 55% 56%)"/><stop offset="1" stop-color="hsl(${hue} 50% 38%)"/>
  </linearGradient></defs>
  <rect x="4" y="4" width="56" height="56" rx="14" fill="url(#g)"/>
  <text x="32" y="42" font-family="Segoe UI, sans-serif" font-size="28" font-weight="600" fill="white" text-anchor="middle">${letter}</text>
</svg>`;
}

export const test = base.extend<{ page: Page }>({
  page: async ({ page }, use) => {
    await page.route("http://asset.localhost/**", (route) => {
      const path = decodeURIComponent(new URL(route.request().url()).pathname);
      const slug =
        path
          .split("\\")
          .at(-1)
          ?.replace(/\.png$/, "") ?? "";
      return slug === "obs64"
        ? route.fulfill({ status: 404 })
        : route.fulfill({ contentType: "image/svg+xml", body: iconSvg(slug) });
    });
    await use(page);
  },
});

export { expect };

/** Opens the app on a mock fixture and waits until the first view and the font are in. */
export async function open(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.locator("main").locator("*").first().waitFor();
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
}

export const shot = (page: Page, name: string) =>
  page.screenshot({ path: `e2e/screenshots/${name}.png` });
