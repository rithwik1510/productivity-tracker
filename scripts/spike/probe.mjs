// Runs `tracker-probe` subcommands. `Probe` keeps a long-lived `probe` process and exposes its
// latest Sample, so checks can wait for a condition instead of sleeping a fixed time.

import { execFile, spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { promisify } from "node:util";

import { sleep } from "./browser.mjs";

// Built by `cargo build -p tracker-win --features probe --bin tracker-probe`; override with --probe.
export const DEFAULT_PROBE = `${process.env.CARGO_TARGET_DIR ?? "target"}/debug/tracker-probe.exe`;

const run = promisify(execFile);

export class Probe {
  samples = [];
  errors = [];

  /** `target` is `{ pid }`, `{ hwnd }` or `{}` (foreground). */
  constructor(exe, target = {}, intervalMs = 1000) {
    const args = ["probe", "--interval-ms", String(intervalMs)];
    if (target.pid) args.push("--pid", String(target.pid));
    if (target.hwnd) args.push("--hwnd", String(target.hwnd));
    this.child = spawn(exe, args, { stdio: ["ignore", "pipe", "pipe"] });
    createInterface({ input: this.child.stdout }).on("line", (line) => {
      try {
        this.samples.push(JSON.parse(line));
      } catch {
        this.errors.push(`bad line: ${line}`);
      }
    });
    createInterface({ input: this.child.stderr }).on("line", (line) => this.errors.push(line));
  }

  get latest() {
    return this.samples.at(-1);
  }

  /** Waits for a sample taken after now that satisfies `predicate`; returns it or the last one. */
  async waitFor(predicate, timeoutMs = 8000) {
    const start = this.samples.length;
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const hit = this.samples.slice(start).find(predicate);
      if (hit) return { ok: true, sample: hit };
      await sleep(100);
    }
    return { ok: false, sample: this.latest };
  }

  /** The next `n` samples taken after now. */
  async next(n = 1) {
    const start = this.samples.length;
    while (this.samples.length < start + n) await sleep(100);
    return this.samples.slice(start, start + n);
  }

  stop() {
    this.child.kill();
  }
}

/** `tracker-probe spike <args>`; resolves to `{ ok, out }`. */
export async function spike(exe, ...args) {
  try {
    const { stdout, stderr } = await run(exe, ["spike", ...args.map(String)]);
    return { ok: true, out: (stdout + stderr).trim() };
  } catch (err) {
    return { ok: false, out: `${err.stdout ?? ""}${err.stderr ?? err.message}`.trim() };
  }
}
