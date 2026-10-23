// Win32 checks the harness needs to stay out of the owner's way: monitors, a process's windows
// and where they are, the foreground window, and moving a window without activating it.
// Also lists WASAPI audio sessions with their PIDs, to attribute an audible result.
// Runs PowerShell with a small C# helper, compiled once into the temp work dir.

import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const WORK = join(tmpdir(), "pt-spike");

/** Set by desktop.ps1 when the harness runs on its own, never-shown desktop. */
export const ON_HIDDEN_DESKTOP = process.env.PT_SPIKE_DESKTOP ?? null;

const SOURCE = String.raw`
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class PtWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  delegate bool MonProc(IntPtr m, IntPtr dc, ref RECT r, IntPtr d);
  delegate bool WndProc(IntPtr h, IntPtr l);
  [DllImport("user32")] static extern bool EnumDisplayMonitors(IntPtr dc, IntPtr clip, MonProc f, IntPtr d);
  [DllImport("user32")] static extern bool EnumWindows(WndProc f, IntPtr l);
  [DllImport("user32")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32")] static extern bool IsIconic(IntPtr h);
  [DllImport("user32")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  static string Rect(RECT r) { return "[" + r.L + "," + r.T + "," + r.R + "," + r.B + "]"; }
  static string Esc(string s) { return s.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("\n", " "); }
  public static string Monitors() {
    var list = new List<string>();
    EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (IntPtr m, IntPtr dc, ref RECT r, IntPtr d) => { list.Add(Rect(r)); return true; }, IntPtr.Zero);
    return "[" + string.Join(",", list) + "]";
  }
  public static string Windows(uint[] pids) {
    var want = new HashSet<uint>(pids);
    var list = new List<string>();
    EnumWindows((h, l) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (!want.Contains(pid) || !IsWindowVisible(h)) return true;
      RECT r; GetWindowRect(h, out r);
      var t = new StringBuilder(512); GetWindowText(h, t, 512);
      var c = new StringBuilder(256); GetClassName(h, c, 256);
      list.Add("{\"hwnd\":" + h.ToInt64() + ",\"pid\":" + pid + ",\"rect\":" + Rect(r) + ",\"iconic\":" + (IsIconic(h) ? "true" : "false") + ",\"class\":\"" + Esc(c.ToString()) + "\",\"title\":\"" + Esc(t.ToString()) + "\"}");
      return true;
    }, IntPtr.Zero);
    return "[" + string.Join(",", list) + "]";
  }
  public static string Foreground() {
    var h = GetForegroundWindow(); uint pid; GetWindowThreadProcessId(h, out pid);
    return "{\"hwnd\":" + h.ToInt64() + ",\"pid\":" + pid + "}";
  }
  // SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER
  public static bool Move(long h, int x, int y) { return SetWindowPos(new IntPtr(h), IntPtr.Zero, x, y, 0, 0, 0x1 | 0x4 | 0x10 | 0x200); }

  // WASAPI sessions on every active render endpoint, the same view the sampler's audio.rs has,
  // but with the owning PID of each session, so a result can be attributed.
  [ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumerator {}
  [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDeviceEnumerator { [PreserveSig] int EnumAudioEndpoints(int flow, int mask, out IMMDeviceCollection c); }
  [ComImport, Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDeviceCollection { [PreserveSig] int GetCount(out uint n); [PreserveSig] int Item(uint i, out IMMDevice d); }
  [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDevice { [PreserveSig] int Activate(ref Guid iid, int ctx, IntPtr p, [MarshalAs(UnmanagedType.IUnknown)] out object o); }
  [ComImport, Guid("77AA99A0-1BD6-484F-8BC7-2C654C9A9B6F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioSessionManager2 { [PreserveSig] int GetAudioSessionControl(IntPtr a, int b, out IntPtr c); [PreserveSig] int GetSimpleAudioVolume(IntPtr a, int b, out IntPtr c); [PreserveSig] int GetSessionEnumerator(out IAudioSessionEnumerator e); }
  [ComImport, Guid("E2F5BB11-0570-40CA-ACDD-3AA01277DEE8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioSessionEnumerator { [PreserveSig] int GetCount(out int n); [PreserveSig] int GetSession(int i, [MarshalAs(UnmanagedType.IUnknown)] out object s); }
  [ComImport, Guid("bfb7ff88-7239-4fc9-8fa2-07c950be9c6d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioSessionControl2 {
    [PreserveSig] int GetState(out int s); [PreserveSig] int GetDisplayName(out IntPtr n); [PreserveSig] int SetDisplayName(string n, IntPtr g);
    [PreserveSig] int GetIconPath(out IntPtr p); [PreserveSig] int SetIconPath(string p, IntPtr g); [PreserveSig] int GetGroupingParam(out Guid g);
    [PreserveSig] int SetGroupingParam(IntPtr g, IntPtr c); [PreserveSig] int RegisterAudioSessionNotification(IntPtr n); [PreserveSig] int UnregisterAudioSessionNotification(IntPtr n);
    [PreserveSig] int GetSessionIdentifier(out IntPtr s); [PreserveSig] int GetSessionInstanceIdentifier(out IntPtr s); [PreserveSig] int GetProcessId(out uint pid); [PreserveSig] int IsSystemSoundsSession();
  }
  [ComImport, Guid("C02216F6-8C67-4B5B-9D00-D008E73E0064"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioMeterInformation { [PreserveSig] int GetPeakValue(out float p); }
  [ComImport, Guid("87CE5498-68D6-44E5-9215-6F892A0B5B1E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface ISimpleAudioVolume { [PreserveSig] int SetMasterVolume(float v, IntPtr g); [PreserveSig] int GetMasterVolume(out float v); [PreserveSig] int SetMute(bool m, IntPtr g); [PreserveSig] int GetMute(out bool m); }
  public static string Sessions() {
    var list = new List<string>();
    var en = (IMMDeviceEnumerator)new MMDeviceEnumerator();
    IMMDeviceCollection devs; en.EnumAudioEndpoints(0 /* eRender */, 1 /* ACTIVE */, out devs);
    uint n; devs.GetCount(out n);
    var iid = typeof(IAudioSessionManager2).GUID;
    for (uint i = 0; i < n; i++) {
      IMMDevice dev; devs.Item(i, out dev);
      object o; dev.Activate(ref iid, 23 /* CLSCTX_ALL */, IntPtr.Zero, out o);
      IAudioSessionEnumerator se; ((IAudioSessionManager2)o).GetSessionEnumerator(out se);
      int count; se.GetCount(out count);
      for (int j = 0; j < count; j++) {
        object s; se.GetSession(j, out s);
        var c = (IAudioSessionControl2)s;
        if (c.IsSystemSoundsSession() == 0) continue; // S_OK: the system sounds session
        uint pid; c.GetProcessId(out pid);
        int state; c.GetState(out state);
        float peak; ((IAudioMeterInformation)s).GetPeakValue(out peak);
        bool mute = false; var vol = s as ISimpleAudioVolume; if (vol != null) vol.GetMute(out mute);
        list.Add("{\"endpoint\":" + i + ",\"pid\":" + pid + ",\"state\":" + state + ",\"peak\":" + peak.ToString(System.Globalization.CultureInfo.InvariantCulture) + ",\"muted\":" + (mute ? "true" : "false") + "}");
      }
    }
    return "[" + string.Join(",", list) + "]";
  }
}`;

// Named by a hash of the source, so editing the C# recompiles it.
const DLL = join(
  WORK,
  `PtSpikeWin-${createHash("sha1").update(SOURCE).digest("hex").slice(0, 8)}.dll`,
);

function ps(script) {
  if (!existsSync(DLL)) {
    mkdirSync(WORK, { recursive: true });
    const escaped = SOURCE.replaceAll("'", "''");
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-Command",
        `Add-Type -TypeDefinition '${escaped}' -OutputAssembly '${DLL}' -OutputType Library`,
      ],
      { stdio: "ignore" },
    );
  }
  // MTA, like the sampler's poll thread.
  const out = execFileSync(
    "powershell.exe",
    ["-NoProfile", "-MTA", "-Command", `Add-Type -Path '${DLL}'; ${script}`],
    { encoding: "utf8" },
  );
  return JSON.parse(out.trim());
}

/** Monitor rects `[left, top, right, bottom]` in virtual-screen pixels. */
export const monitors = () => ps("[PtWin]::Monitors()");

/** Visible top-level windows owned by any of `pids`. */
export const windowsOf = (pids) =>
  ps(`[PtWin]::Windows([uint32[]]@(${pids.map(Number).join(",")}))`);

/** The foreground window and its process. */
export const foreground = () => ps("[PtWin]::Foreground()");

/** Moves a window to (x, y) without activating it or changing its z-order. */
export const moveWindow = (hwnd, x, y) =>
  ps(`[PtWin]::Move(${Number(hwnd)}, ${x}, ${y}) | ConvertTo-Json`);

const intersects = (a, b) => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];

// Windows' input-language indicator, which the text input framework puts in every GUI process
// on a desktop with no taskbar (desktop.ps1's). Not browser UI, and that desktop is never shown.
const OS_INDICATOR = new Set(["UAC Input Indicator", "UAC_InputIndicatorOverlayWnd"]);

/** Visible, non-minimized windows of `pids` that overlap a monitor. */
function onScreen(pids, mons) {
  return windowsOf(pids).filter(
    (w) => !w.iconic && !OS_INDICATOR.has(w.class) && mons.some((m) => intersects(w.rect, m)),
  );
}

/**
 * Throws unless every visible window of `pids` lies outside every monitor. Returns what it saw,
 * for the report.
 */
export function assertOffscreen(pids) {
  const mons = monitors();
  const wins = windowsOf(pids);
  const visible = onScreen(pids, mons);
  if (visible.length > 0) {
    throw new Error(
      `window on screen: ${JSON.stringify(visible)} (monitors ${JSON.stringify(mons)})`,
    );
  }
  return { monitors: mons, windows: wins };
}

/** Every non-system render session: `{ endpoint, pid, state, peak, muted }` (state 1 = active). */
export const audioSessions = () => ps("[PtWin]::Sessions()");

/**
 * Keeps our browsers out of the owner's way. Moves any visible window of `pids` that touches a
 * monitor back off-screen (without activating it), and throws if one of them is the foreground
 * window, so the harness stops and closes its browsers. Returns the windows it moved.
 */
export function guard(pids, offscreen) {
  const fg = foreground();
  if (pids.includes(fg.pid)) {
    throw new Error(`a spike browser became the foreground window (pid ${fg.pid})`);
  }
  const mons = monitors();
  const moved = onScreen(pids, mons);
  for (const w of moved) moveWindow(w.hwnd, offscreen.x, offscreen.y);
  return moved;
}

/**
 * Starts `fgwatch.ps1`, which logs every foreground change. `stop()` ends it and returns the
 * changes as `{ at, pid, hwnd }`, so a run can prove no spike browser was ever in front.
 */
export function watchForeground(out, intervalMs = 25) {
  // On desktop.ps1's hidden desktop, GetForegroundWindow sees nothing useful (it returns 0);
  // desktop.ps1 watches the owner's real foreground from outside instead.
  if (ON_HIDDEN_DESKTOP) return null;
  writeFileSync(out, "");
  const child = spawn(
    "powershell.exe",
    [
      "-NoProfile",
      "-File",
      fileURLToPath(new URL("./fgwatch.ps1", import.meta.url)),
      "-Out",
      out,
      "-IntervalMs",
      String(intervalMs),
    ],
    { stdio: "ignore" },
  );
  return {
    pid: child.pid,
    stop() {
      child.kill();
      return readFileSync(out, "utf8")
        .split(/\r?\n/)
        .filter(Boolean)
        .map((line) => {
          const [at, pid, hwnd] = line.split(" ");
          return { at, pid: Number(pid), hwnd: Number(hwnd) };
        });
    },
  };
}
