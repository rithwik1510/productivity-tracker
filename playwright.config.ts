import { defineConfig } from "@playwright/test";

// E2E runs the UI in Vite dev with the mocked IPC (src/dev/mockIpc.ts), in installed Edge,
// at the window's default size, with motion reduced so screenshots are deterministic. Scrollbars
// are drawn (Playwright hides them by default) because the real window's take 12px of layout.
export default defineConfig({
  testDir: "e2e",
  use: {
    channel: "msedge",
    baseURL: "http://localhost:1420",
    viewport: { width: 1000, height: 680 },
    // The app formats dates in the user's locale; tests pin one so names are stable.
    locale: "en-US",
    contextOptions: { reducedMotion: "reduce" },
    launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] },
  },
  webServer: {
    command: "bun run dev",
    url: "http://localhost:1420",
    reuseExistingServer: true,
  },
});
