# Appends a line to -Out whenever the foreground window's process changes:
# "<ISO time> <pid> <hwnd>". The spike harness starts it, then checks that none of these PIDs
# was one of its own browsers. Runs until killed.
param([Parameter(Mandatory)][string]$Out, [int]$IntervalMs = 25)
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class PtFg {
  [DllImport("user32")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
}
"@
$last = -1
while ($true) {
  $h = [PtFg]::GetForegroundWindow(); $p = 0
  [void][PtFg]::GetWindowThreadProcessId($h, [ref]$p)
  if ($p -ne $last) {
    Add-Content -Path $Out -Value "$((Get-Date).ToString('o')) $p $h"
    $last = $p
  }
  Start-Sleep -Milliseconds $IntervalMs
}
