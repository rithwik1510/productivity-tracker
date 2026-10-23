// ADR-0002 soak: Chrome and Edge on a static page, polled by `tracker-probe probe --pid` at 1 s
// for N minutes. Measures renderer CPU without and with the probe, and reads the accessibility
// modes from chrome://accessibility / edge://accessibility at the end.
//
//   node scripts/spike/soak.mjs [--minutes 30] [--probe <tracker-probe.exe>] [--out <json>]
//
// Temp profiles only; every browser and probe started here is stopped at the end. The browsers
// open off-screen and are never activated: every minute the harness checks that no window of
// theirs is on a monitor (moving strays back without activating them) and that none is the
// foreground window, and a foreground watcher logs every foreground change for the report.
// Run it through desktop.ps1 so it lives on a desktop the owner never sees.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseArgs } from "node:util";

import { Browser, OFFSCREEN, sleep } from "./browser.mjs";
import { DEFAULT_PROBE, Probe } from "./probe.mjs";
import { assertOffscreen, guard, ON_HIDDEN_DESKTOP, watchForeground } from "./win.mjs";

const { values } = parseArgs({
  options: {
    minutes: { type: "string", default: "30" },
    probe: { type: "string", default: DEFAULT_PROBE },
    out: { type: "string", default: join(tmpdir(), "pt-spike", "soak-results.json") },
  },
});
const minutes = Number(values.minutes);
const STATIC_URL = "https://example.com/";
const CPU_WINDOW_MS = 60_000;

// Run a private copy so the crate can be rebuilt while the soak holds the exe open.
const workDir = join(tmpdir(), "pt-spike");
mkdirSync(workDir, { recursive: true });
const probeExe = join(workDir, "tracker-probe-soak.exe");
copyFileSync(values.probe, probeExe);

const log = (...args) => console.log(new Date().toISOString().slice(11, 19), ...args);

/** CPU seconds a process has used so far, from PowerShell's Get-Process. */
function processCpuSeconds(pid) {
  const out = execFileSync(
    "powershell.exe",
    ["-NoProfile", "-Command", `(Get-Process -Id ${pid}).CPU`],
    {
      encoding: "utf8",
    },
  );
  return Number(out.trim().replace(",", "."));
}

const browsers = [];
const probes = [];
const results = {
  minutes,
  startedAt: new Date().toISOString(),
  browsers: {},
  owner: { hiddenDesktop: ON_HIDDEN_DESKTOP },
};
const fgWatch = watchForeground(join(workDir, "soak-foreground.log"), 50);
const ourPids = new Set();
const movedWindows = [];

/** Every PID of our browsers so far (renderers come and go), then the guard. */
async function checkOwnerUndisturbed() {
  for (const b of browsers) for (const pid of await b.pids()) ourPids.add(pid);
  for (const w of guard([...ourPids], OFFSCREEN)) {
    movedWindows.push({ at: new Date().toISOString(), ...w });
    log("moved an on-screen window back off-screen", JSON.stringify(w));
  }
}

try {
  for (const name of ["chrome", "edge"])
    browsers.push(await Browser.launch(name, { url: STATIC_URL }));
  log("launched", browsers.map((b) => `${b.name}:${b.pid}`).join(" "));
  await sleep(5_000);
  await checkOwnerUndisturbed();
  results.owner.placement = assertOffscreen([...ourPids]);
  await sleep(25_000); // let first-run work settle before the baseline

  for (const b of browsers) {
    results.browsers[b.name] = { pid: b.pid, a11yBefore: await b.accessibility() };
  }
  log("baseline CPU (no probe)...");
  const baseline = await Promise.all(browsers.map((b) => b.cpu(CPU_WINDOW_MS)));
  browsers.forEach((b, i) => {
    results.browsers[b.name].cpuBaseline = baseline[i];
  });
  log("baseline", JSON.stringify(baseline));

  for (const b of browsers) probes.push(new Probe(probeExe, { pid: b.pid }));
  const probeStart = Date.now();
  const deadline = probeStart + minutes * 60_000;
  while (Date.now() < deadline) {
    await sleep(Math.min(60_000, deadline - Date.now()));
    await checkOwnerUndisturbed();
    const status = browsers.map((b, i) => {
      const p = probes[i];
      return `${b.name}: ${p.samples.length} samples, domain ${p.latest?.foreground?.browser?.domain}`;
    });
    log(`${Math.round((Date.now() - probeStart) / 60_000)} min`, status.join(" | "));
  }

  log("CPU with probe...");
  const withProbe = await Promise.all(browsers.map((b) => b.cpu(CPU_WINDOW_MS)));
  for (const [i, b] of browsers.entries()) {
    const r = results.browsers[b.name];
    r.cpuWithProbe = withProbe[i];
    // Read while the probes are still polling, from a background tab: the measured tab stays
    // the active one, so the probe keeps reading example.com.
    r.a11yAfter = await b.accessibility();
  }
  const elapsedS = (Date.now() - probeStart) / 1000;
  for (const [i, b] of browsers.entries()) {
    const p = probes[i];
    const r = results.browsers[b.name];
    r.probeCpuPct = Math.round((processCpuSeconds(p.child.pid) / elapsedS) * 10000) / 100;
    p.stop();
    const domains = {};
    let maxGapMs = 0;
    for (const [j, s] of p.samples.entries()) {
      const d = String(s.foreground?.browser?.domain ?? null);
      domains[d] = (domains[d] ?? 0) + 1;
      if (j > 0) maxGapMs = Math.max(maxGapMs, s.monoMs - p.samples[j - 1].monoMs);
    }
    Object.assign(r, {
      samples: p.samples.length,
      expectedSamples: Math.floor(elapsedS),
      maxGapMs,
      domains,
      probeLog: p.errors.slice(0, 20),
    });
  }
  await checkOwnerUndisturbed();
  results.finishedAt = new Date().toISOString();
} catch (err) {
  results.error = String(err?.stack ?? err);
  throw err;
} finally {
  for (const p of probes) p.stop();
  for (const b of browsers) await b.close();
  if (fgWatch) {
    const changes = fgWatch.stop();
    results.owner.foregroundChanges = changes.length;
    results.owner.foregroundWasOurs = changes.filter((c) => ourPids.has(c.pid));
  }
  results.owner.ourPids = [...ourPids];
  results.owner.movedWindows = movedWindows;
  // Written even when the run aborts, so a partial soak still leaves its numbers.
  writeFileSync(values.out, JSON.stringify(results, null, 2));
  log("wrote", values.out);
  console.log(JSON.stringify(results, null, 2));
}
