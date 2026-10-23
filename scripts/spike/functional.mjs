// ADR-0002 functional checks (plan section 8.2) against throwaway Chrome and Edge instances,
// read through `tracker-probe probe --pid`, plus the packaged-app identity checks.
//
//   node scripts/spike/functional.mjs [--probe <tracker-probe.exe>] [--out <json>]
//
// Temp profiles only; every browser started here is closed at the end. The owner uses this PC
// during the run, so nothing here may come to the front:
// - browsers open off-screen (checked against every monitor), tabs open in the background, and a
//   tab switch is done by closing the active tab, which activates its neighbour without
//   activating the window;
// - a foreground watcher logs every foreground change, and the run fails if one was ours;
// - better still, run it through desktop.ps1, on a desktop the owner never sees;
// - checks that need focus or the screen are recorded as skipped, with the reason: the drafts
//   and the empty focused omnibox (`spike focus` / UIA SetFocus activate the window), fullscreen
//   (it would cover the owner's monitor), and launching Calculator / Terminal (on-screen windows;
//   they are probed only if already running).

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseArgs } from "node:util";

import { BROWSERS, Browser, OFFSCREEN, sleep } from "./browser.mjs";
import { DEFAULT_PROBE, Probe } from "./probe.mjs";
import {
  assertOffscreen,
  audioSessions,
  guard,
  ON_HIDDEN_DESKTOP,
  watchForeground,
} from "./win.mjs";

const { values } = parseArgs({
  options: {
    probe: { type: "string", default: DEFAULT_PROBE },
    out: { type: "string", default: join(tmpdir(), "pt-spike", "functional-results.json") },
  },
});
const exe = values.probe;
const workDir = join(tmpdir(), "pt-spike");
mkdirSync(workDir, { recursive: true });

const results = [];
const ourPids = new Set();
const movedWindows = [];
const log = (...args) => console.log(new Date().toISOString().slice(11, 19), ...args);
const domainOf = (s) => s?.foreground?.browser?.domain ?? null;
const view = (s) =>
  s && {
    identity: s.foreground?.identity,
    domain: domainOf(s),
    private: s.foreground?.browser?.private,
    audible: s.foreground?.audible,
    title: s.foreground?.title,
  };

/** `pass` is true, false, or null for a check that was skipped on purpose. */
function record(criterion, browser, pass, detail) {
  results.push({ criterion, browser, pass, detail });
  log(pass === null ? "SKIP" : pass ? "PASS" : "FAIL", browser, criterion, JSON.stringify(detail));
}

/** Collects the browser's PIDs and runs the guard (throws if one of ours is in front). */
async function keepOut(browser) {
  for (const pid of await browser.pids()) ourPids.add(pid);
  for (const w of guard([...ourPids], OFFSCREEN)) {
    movedWindows.push({ at: new Date().toISOString(), ...w });
    log("moved an on-screen window back off-screen", JSON.stringify(w));
  }
}

/** Waits until the probe reports `domain`, and records how long it took. */
async function expectDomain(probe, criterion, browser, domain) {
  const start = Date.now();
  const { ok, sample } = await probe.waitFor((s) => domainOf(s) === domain);
  record(criterion, browser, ok, { expected: domain, got: view(sample), ms: Date.now() - start });
}

async function browserChecks(name) {
  const browser = await Browser.launch(name, {
    url: "https://example.com/",
    args: ["--autoplay-policy=no-user-gesture-required"],
  });
  const probe = new Probe(exe, { pid: browser.pid });
  try {
    await sleep(2000);
    await keepOut(browser);
    const placement = assertOffscreen([...ourPids]);
    record("window opens off every monitor", name, true, placement);

    const first = await browser.firstPage();
    await expectDomain(probe, "initial page (normal window, depth-6 walk)", name, "example.com");

    await first.navigate("https://example.org/");
    await expectDomain(probe, "after navigation", name, "example.org");

    await first.navigate("https://en.wikipedia.org/wiki/Main_Page");
    await expectDomain(probe, "subdomain", name, "en.wikipedia.org");
    await keepOut(browser);

    // Tab switches without activating the window: open a background tab (the domain must not
    // change), then close the active tab so the background one becomes active.
    const second = await browser.newPage("https://example.com/");
    await sleep(3000);
    const steady = await probe.next(3);
    record(
      "background tab does not change the domain",
      name,
      steady.every((s) => domainOf(s) === "en.wikipedia.org"),
      { domains: steady.map(domainOf) },
    );
    await first.close();
    await expectDomain(probe, "tab switch (active tab closed)", name, "example.com");
    const third = await browser.newPage("https://example.org/");
    await sleep(3000);
    await second.close();
    await expectDomain(probe, "tab switch (second time)", name, "example.org");
    await keepOut(browser);

    await third.navigate(name === "chrome" ? "chrome://newtab/" : "edge://newtab/");
    await sleep(2500);
    const ntp = await probe.next(2);
    record(
      "new tab page gives None",
      name,
      ntp.every((s) => domainOf(s) === null),
      { got: ntp.map(view) },
    );
    await third.navigate("https://example.com/");
    await expectDomain(probe, "after leaving the new tab page", name, "example.com");
    await keepOut(browser);

    for (const criterion of [
      'draft "github.com" ignored',
      'draft "hello wor" ignored',
      "empty focused omnibox gives None",
    ]) {
      record(criterion, name, null, {
        skipped:
          "needs keyboard focus in the browser: `spike focus` and UIA SetFocus bring it to the front",
      });
    }
    for (const criterion of ["fullscreen (F11)", "fullscreen (element)"]) {
      record(criterion, name, null, {
        skipped: "a fullscreen window covers the nearest monitor, which is the owner's screen",
      });
    }

    record(
      "normal window not private",
      name,
      probe.samples.every((s) => s.foreground?.browser?.private === false),
      { samples: probe.samples.length },
    );

    // The capped walk must keep web accessibility off in a normal window too.
    const a11y = await browser.accessibility();
    record(
      "after the run: Web accessibility off (global and per tab)",
      name,
      a11y.web === false && a11y.tabs.every((t) => t.web === "disabled"),
      a11y,
    );

    await audioCheck(name, browser, third, probe);
    await keepOut(browser);
  } finally {
    probe.stop();
    await browser.close();
  }
}

/** Sessions that are actually heard, split into ours (this browser) and everyone else's. */
function heardSessions(pids) {
  const heard = audioSessions().filter((s) => s.peak > 0.001 && !s.muted);
  const ours = heard.filter((s) => pids.has(s.pid));
  const others = heard
    .filter((s) => !pids.has(s.pid))
    .map((s) => ({ ...s, image: imageOf(s.pid) }));
  return { ours, others };
}

function imageOf(pid) {
  try {
    const out = execFileSync("tasklist", ["/FI", `PID eq ${pid}`, "/FO", "CSV", "/NH"], {
      encoding: "utf8",
    });
    return /^"([^"]+)"/.exec(out)?.[1]?.toLowerCase() ?? null;
  } catch {
    return null;
  }
}

/**
 * A quiet (gain 0.05) 5 s tone in the active tab. The AudioContext starts silent and the tone
 * begins only after the sampler's 10 s session refresh, so the long-running probe must find the
 * new session the way the app would.
 *
 * Audio from any other process with the same exe name (say, your own Chrome window) also
 * counts as audible by design, so every phase also lists the heard WASAPI sessions by PID. The
 * result is clean only if, during the tone, the heard session is ours and no other process with
 * this browser's exe name is heard, and the probe is silent before and after.
 */
async function audioCheck(name, browser, page, probe) {
  const image = BROWSERS[name].exe.split("/").at(-1).toLowerCase();
  await page.evaluate(`(async () => {
    const ctx = new AudioContext();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    gain.gain.value = 0;
    osc.frequency.value = 440;
    osc.connect(gain).connect(ctx.destination);
    osc.start();
    await ctx.resume();
    window.spikeTone = { ctx, gain };
    return ctx.state;
  })()`);
  await sleep(11_000);
  const pids = new Set(await browser.pids());
  const sessionsBefore = heardSessions(pids);
  const before = await probe.next(3);
  await page.evaluate("window.spikeTone.gain.gain.value = 0.05; true");
  const toneStart = Date.now();
  // 3 s: leaves room for the session snapshot (about 1.5 s) inside the 5 s tone.
  const { ok, sample } = await probe.waitFor((s) => s.foreground?.audible === true, 3000);
  const detectMs = Date.now() - toneStart;
  const sessionsDuring = heardSessions(new Set(await browser.pids()));
  await sleep(Math.max(0, 5000 - (Date.now() - toneStart)));
  await page.evaluate("window.spikeTone.ctx.close().then(() => true)");
  await sleep(1500);
  const after = await probe.next(3);
  const sessionsAfter = heardSessions(pids);

  const sameExe = (phase) => phase.others.filter((s) => s.image === image);
  const audibleBefore = before.some((s) => s.foreground?.audible);
  const confounded = audibleBefore || sameExe(sessionsDuring).length > 0;
  const attributed = sessionsDuring.ours.length > 0;
  const silentAfter = after.every((s) => !s.foreground?.audible);
  const identity = sample?.foreground?.identity?.key ?? image;
  record(
    `5 s WebAudio tone marks ${identity} audible`,
    name,
    ok && attributed && silentAfter && !confounded,
    {
      verdict: confounded
        ? `inconclusive: another ${image} was heard (or the probe was already audible)`
        : !ok
          ? "not detected"
          : !attributed
            ? "detected, but no heard session of ours: session lister disagrees"
            : silentAfter
              ? "detected, attributed to our browser"
              : "detected, but still audible after the tone",
      audibleBeforeTone: before.map((s) => s.foreground?.audible),
      detectedAfterMs: ok ? detectMs : null,
      audibleAfterTone: after.map((s) => s.foreground?.audible),
      sessionsBefore,
      sessionsDuring,
      sessionsAfter,
    },
  );
}

async function privateCheck(name) {
  const browser = await Browser.launch(name, {
    url: "https://example.com/",
    args: [BROWSERS[name].privateFlag],
  });
  const probe = new Probe(exe, { pid: browser.pid });
  try {
    await sleep(3000);
    await keepOut(browser);
    const samples = await probe.next(3);
    const title = samples.at(-1)?.foreground?.title ?? "";
    const titleMarker = title.includes(" - [InPrivate] - ");
    record(
      `${BROWSERS[name].privateFlag} window reports private`,
      name,
      samples.every((s) => s.foreground?.browser?.private === true),
      {
        got: view(samples.at(-1)),
        signal: titleMarker
          ? "window title marker (and UIA root view name)"
          : "UIA root view name only (title has no marker)",
      },
    );
  } finally {
    probe.stop();
    await browser.close();
  }
}

/** PIDs of running processes with this image name. */
function pidsOf(image) {
  const out = execFileSync("tasklist", ["/FI", `IMAGENAME eq ${image}`, "/FO", "CSV", "/NH"], {
    encoding: "utf8",
  });
  return [...out.matchAll(/^"[^"]+","(\d+)"/gm)].map((m) => Number(m[1]));
}

/** Probes an app window, only if the app is already running: launching it would show a window. */
async function packagedCheck(label, image, expectedPrefix) {
  const [pid] = pidsOf(image);
  if (!pid) {
    record(`${label} resolves to an AUMID`, label, null, {
      skipped: "not running; launching it would open a window in front of the owner",
    });
    return;
  }
  const probe = new Probe(exe, { pid });
  const [sample] = await probe.next(1);
  probe.stop();
  const identity = sample?.foreground?.identity;
  record(
    `${label} resolves to an AUMID`,
    label,
    identity?.kind === "aumid" && identity.key.startsWith(expectedPrefix),
    { pid, identity },
  );
}

const fgWatch = watchForeground(join(workDir, "functional-foreground.log"));
try {
  for (const name of ["chrome", "edge"]) {
    await browserChecks(name);
    await privateCheck(name);
  }
  await packagedCheck("Calculator", "CalculatorApp.exe", "Microsoft.WindowsCalculator_");
  await packagedCheck("Windows Terminal", "WindowsTerminal.exe", "Microsoft.WindowsTerminal_");
} finally {
  if (fgWatch) {
    const changes = fgWatch.stop();
    const ours = changes.filter((c) => ourPids.has(c.pid));
    record("no spike browser was ever the foreground window", "all", ours.length === 0, {
      foregroundChanges: changes.length,
      ours,
      movedWindows,
      pidsWatched: ourPids.size,
    });
  } else {
    log(`on hidden desktop ${ON_HIDDEN_DESKTOP}: desktop.ps1 logs the owner's foreground`);
  }
  writeFileSync(join(workDir, "functional-pids.json"), JSON.stringify([...ourPids]));
  writeFileSync(values.out, JSON.stringify(results, null, 2));
  log("wrote", values.out);
  const failed = results.filter((r) => r.pass === false);
  const skipped = results.filter((r) => r.pass === null);
  log(
    `${results.length - failed.length - skipped.length} passed, ${failed.length} failed, ${skipped.length} skipped`,
  );
}
