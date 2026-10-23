// Launches a throwaway Chrome/Edge (temp profile, remote debugging) and talks CDP over Node's
// built-in WebSocket. No dependencies. Every browser started here is killed by `close()`.
//
// The owner uses this PC while the spike runs, so nothing here may activate a browser window:
// windows open off-screen, tabs open in the background, and there is deliberately no
// `Target.activateTarget` / `Page.bringToFront` / `Browser.setWindowBounds` helper. Chrome
// answers those by activating its window (and fullscreen would cover the owner's monitor).

import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const BROWSERS = {
  chrome: {
    exe: "C:/Program Files/Google/Chrome/Application/chrome.exe",
    privateFlag: "--incognito",
    a11yUrl: "chrome://accessibility",
  },
  edge: {
    exe: "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    privateFlag: "--inprivate",
    a11yUrl: "edge://accessibility",
  },
};

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Where off-screen windows go: far outside any monitor layout. */
export const OFFSCREEN = { x: -32000, y: -32000 };

/** A CDP connection with flat sessions (`sessionId` per attached target). */
class Cdp {
  #ws;
  #nextId = 1;
  #pending = new Map();
  #listeners = new Set();

  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((resolve, reject) => {
      ws.addEventListener("open", resolve, { once: true });
      ws.addEventListener("error", reject, { once: true });
    });
    return new Cdp(ws);
  }

  constructor(ws) {
    this.#ws = ws;
    ws.addEventListener("message", (event) => {
      const msg = JSON.parse(event.data);
      if (msg.id !== undefined) {
        const pending = this.#pending.get(msg.id);
        this.#pending.delete(msg.id);
        if (msg.error) pending?.reject(new Error(`${pending.method}: ${msg.error.message}`));
        else pending?.resolve(msg.result);
      } else {
        for (const listener of this.#listeners) listener(msg);
      }
    });
  }

  send(method, params = {}, sessionId = undefined) {
    const id = this.#nextId++;
    this.#ws.send(JSON.stringify({ id, method, params, sessionId }));
    // A browser that died mid-call never answers; fail instead of hanging the run.
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`${method}: no reply in 30 s`));
      }, 30_000);
      this.#pending.set(id, {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (err) => {
          clearTimeout(timer);
          reject(err);
        },
        method,
      });
    });
  }

  /** Resolves with the first event matching `method` (and `sessionId`, if given). */
  once(method, sessionId = undefined, timeoutMs = 15000) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#listeners.delete(listener);
        reject(new Error(`timed out waiting for ${method}`));
      }, timeoutMs);
      const listener = (msg) => {
        if (msg.method === method && (sessionId === undefined || msg.sessionId === sessionId)) {
          clearTimeout(timer);
          this.#listeners.delete(listener);
          resolve(msg.params);
        }
      };
      this.#listeners.add(listener);
    });
  }

  close() {
    this.#ws.close();
  }
}

/** A page target attached in flat mode. */
export class Page {
  constructor(cdp, targetId, sessionId) {
    this.cdp = cdp;
    this.targetId = targetId;
    this.sessionId = sessionId;
  }

  send(method, params = {}) {
    return this.cdp.send(method, params, this.sessionId);
  }

  async navigate(url) {
    await this.send("Page.enable");
    const loaded = this.cdp.once("Page.loadEventFired", this.sessionId, 30000).catch(() => {});
    await this.send("Page.navigate", { url });
    await loaded;
  }

  async evaluate(expression) {
    const { result, exceptionDetails } = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
      userGesture: true,
    });
    if (exceptionDetails) {
      throw new Error(
        `evaluate: ${exceptionDetails.exception?.description ?? exceptionDetails.text}`,
      );
    }
    return result.value;
  }

  close() {
    return this.cdp.send("Target.closeTarget", { targetId: this.targetId });
  }
}

export class Browser {
  /**
   * Starts `name` ("chrome" | "edge") with a fresh temp profile. `args` are extra switches.
   *
   * By default the window opens off every monitor, so the owner never sees it. Chrome would then
   * mark it occluded and throttle its renderers, so occlusion tracking and backgrounding are
   * switched off: the page renders as if it were a visible window, which is what the soak's CPU
   * comparison needs.
   */
  static async launch(name, { args = [], url = "about:blank", offscreen = true } = {}) {
    const spec = BROWSERS[name];
    const profile = mkdtempSync(join(tmpdir(), `pt-spike-${name}-`));
    const placement = offscreen
      ? [
          `--window-position=${OFFSCREEN.x},${OFFSCREEN.y}`,
          "--window-size=1280,800",
          "--disable-backgrounding-occluded-windows",
          "--disable-renderer-backgrounding",
        ]
      : ["--window-size=1200,800"];
    const child = spawn(
      spec.exe,
      [
        `--user-data-dir=${profile}`,
        // Port 0: the browser picks a free port and writes it to DevToolsActivePort, so
        // harnesses running side by side never collide.
        "--remote-debugging-port=0",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-sync",
        // One switch: a repeated --disable-features replaces the earlier one. Translate's
        // bubble is a separate window that Windows clamps toward the screen.
        `--disable-features=Translate${offscreen ? ",CalculateNativeWinOcclusion" : ""}`,
        ...placement,
        ...args,
        url,
      ],
      { stdio: "ignore", detached: false },
    );
    const browser = new Browser(name, spec, profile, child);
    const wsUrl = await browser.#waitForDebugger();
    browser.cdp = await Cdp.connect(wsUrl);
    browser.pid = await browser.#browserPid();
    return browser;
  }

  /** PIDs of every process of this browser instance (all started by us). */
  async pids() {
    return (await this.processes()).map((p) => p.id);
  }

  constructor(name, spec, profile, child) {
    this.name = name;
    this.spec = spec;
    this.profile = profile;
    this.child = child;
  }

  /** The browser's WebSocket URL, from `<profile>/DevToolsActivePort` (port, then path). */
  async #waitForDebugger() {
    for (let i = 0; i < 100; i++) {
      try {
        const [port, path] = readFileSync(join(this.profile, "DevToolsActivePort"), "utf8")
          .trim()
          .split(/\r?\n/);
        if (port && path) return `ws://127.0.0.1:${port}${path}`;
      } catch {
        // Not written yet.
      }
      await sleep(200);
    }
    throw new Error(`${this.name}: DevToolsActivePort never appeared in ${this.profile}`);
  }

  /** Every browser process, with cumulative CPU seconds. */
  async processes() {
    const { processInfo } = await this.cdp.send("SystemInfo.getProcessInfo");
    return processInfo;
  }

  /**
   * CPU use (percent of one core) per process type over `ms`, from cumulative cpuTime deltas.
   * `renderer` sums every renderer process; `rendererMax` is the busiest single one.
   */
  async cpu(ms) {
    const before = new Map((await this.processes()).map((p) => [p.id, p]));
    await sleep(ms);
    const after = await this.processes();
    const pct = { browser: 0, renderer: 0, rendererMax: 0, gpu: 0, total: 0 };
    for (const p of after) {
      const prev = before.get(p.id);
      if (!prev) continue;
      const share = ((p.cpuTime - prev.cpuTime) / (ms / 1000)) * 100;
      pct.total += share;
      if (p.type === "browser") pct.browser += share;
      if (p.type === "GPU") pct.gpu += share;
      if (p.type === "renderer") {
        pct.renderer += share;
        pct.rendererMax = Math.max(pct.rendererMax, share);
      }
    }
    for (const key of Object.keys(pct)) pct[key] = Math.round(pct[key] * 100) / 100;
    return pct;
  }

  /**
   * Accessibility modes from chrome://accessibility (edge://accessibility), read from the page's
   * own DOM. Never uses the CDP Accessibility domain, which would itself enable web a11y.
   */
  async accessibility() {
    // A background tab: the measured tab stays the active, visible one, and no window opens.
    const page = await this.newPage(this.spec.a11yUrl);
    try {
      await sleep(2500);
      const state = await page.evaluate(`(() => {
        const text = document.body.innerText;
        const tabs = [];
        const lines = text.split("\\n");
        for (let i = 1; i < lines.length; i++) {
          const m = /^Native: (true|false)Web: (enabled|disabled)/.exec(lines[i]);
          if (m) tabs.push({ page: lines[i - 1], native: m[1] === "true", web: m[2] });
        }
        return {
          native: document.getElementById("native")?.checked ?? null,
          web: document.getElementById("web")?.checked ?? null,
          uiaClients: /UIA clients:\\n([^\\n]*)/.exec(text)?.[1] ?? null,
          tabs,
        };
      })()`);
      // The a11y page's own row says nothing about the probe; drop it.
      state.tabs = state.tabs.filter((t) => !/:\/\/accessibility\/?$/.test(t.page));
      return state;
    } finally {
      await page.close();
    }
  }

  async #browserPid() {
    const browser = (await this.processes()).find((p) => p.type === "browser");
    if (!browser) throw new Error(`${this.name}: no browser process in getProcessInfo`);
    return browser.id;
  }

  async pages() {
    const { targetInfos } = await this.cdp.send("Target.getTargets");
    return targetInfos.filter((t) => t.type === "page");
  }

  async attach(targetId) {
    const { sessionId } = await this.cdp.send("Target.attachToTarget", { targetId, flatten: true });
    return new Page(this.cdp, targetId, sessionId);
  }

  /** The first page target (the startup tab). */
  async firstPage() {
    for (let i = 0; i < 50; i++) {
      const [first] = await this.pages();
      if (first) return this.attach(first.targetId);
      await sleep(200);
    }
    throw new Error(`${this.name}: no page target`);
  }

  /**
   * Opens `url` in a new background tab of the existing window. A foreground tab would make
   * Chrome activate the window. To switch tabs, close the active one: the neighbour becomes
   * active without the window being activated.
   */
  async newPage(url) {
    const { targetId } = await this.cdp.send("Target.createTarget", { url, background: true });
    return this.attach(targetId);
  }

  async close() {
    try {
      await Promise.race([this.cdp?.send("Browser.close"), sleep(3000)]);
    } catch {
      // Already gone.
    }
    this.cdp?.close();
    await sleep(1000);
    if (this.child.exitCode === null) {
      try {
        execFileSync("taskkill", ["/PID", String(this.child.pid), "/T", "/F"], { stdio: "ignore" });
      } catch {
        // Exited between the check and the kill.
      }
    }
    // The profile can stay locked for a moment after exit.
    for (let i = 0; i < 10; i++) {
      try {
        rmSync(this.profile, { recursive: true, force: true });
        return;
      } catch {
        await sleep(500);
      }
    }
  }
}
