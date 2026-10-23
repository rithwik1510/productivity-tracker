# Runs a spike harness on its own, never-shown Win32 desktop, so the browsers it starts can't
# appear in front of the owner or take keyboard focus. Windows on a desktop other than the input
# desktop are never displayed, never get input, and can't SendInput into the input desktop (the
# trick Chrome uses to force itself to the front). The desktop is created, never switched to.
#
#   powershell -File scripts/spike/desktop.ps1 -Log <file> -Command "node scripts/spike/soak.mjs ..."
#
# Everything the command starts inherits the desktop: the browsers, tracker-probe (which needs
# EnumWindows and UIA on the browsers' desktop) and the PowerShell helpers. stdout and stderr go
# to -Log. Meanwhile this script watches the owner's real foreground window and writes every
# change to <Log>.foreground; `-Check` names the exes that must never show up there.
param(
  [Parameter(Mandatory)][string]$Log,
  [string]$Desktop = "PtSpike",
  [string[]]$Check = @("chrome.exe", "msedge.exe"),
  [Parameter(Mandatory)][string]$Command
)
$ErrorActionPreference = "Stop"
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class PtDesk {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct STARTUPINFO { public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError; }
  [StructLayout(LayoutKind.Sequential)]
  public struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
  [DllImport("user32", SetLastError = true, CharSet = CharSet.Unicode)]
  public static extern IntPtr CreateDesktop(string name, IntPtr dev, IntPtr mode, int flags, uint access, IntPtr sa);
  [DllImport("user32")] public static extern bool CloseDesktop(IntPtr h);
  [DllImport("kernel32", SetLastError = true, CharSet = CharSet.Unicode)]
  public static extern bool CreateProcess(string app, string cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags,
    IntPtr env, string dir, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
  [DllImport("kernel32")] public static extern uint WaitForSingleObject(IntPtr h, uint ms);
  [DllImport("kernel32")] public static extern bool GetExitCodeProcess(IntPtr h, out uint code);
  [DllImport("kernel32")] public static extern bool CloseHandle(IntPtr h);
  [DllImport("user32")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
}
"@
# GENERIC_ALL. Kept open until the command exits; the desktop goes away with its last handle.
$desk = [PtDesk]::CreateDesktop($Desktop, [IntPtr]::Zero, [IntPtr]::Zero, 0, 0x10000000, [IntPtr]::Zero)
if ($desk -eq [IntPtr]::Zero) { throw "CreateDesktop failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }

$si = New-Object PtDesk+STARTUPINFO
$si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si)
$si.lpDesktop = "WinSta0\$Desktop"
$pi = New-Object PtDesk+PROCESS_INFORMATION
# cmd does the redirect; PT_SPIKE_DESKTOP tells the harness it runs here. CREATE_NO_WINDOW.
$cmd = "cmd.exe /d /s /c `"set PT_SPIKE_DESKTOP=$Desktop&& $Command >`"$Log`" 2>&1`""
if (-not [PtDesk]::CreateProcess([NullString]::Value, $cmd, [IntPtr]::Zero, [IntPtr]::Zero, $false, 0x08000000, [IntPtr]::Zero, (Get-Location).Path, [ref]$si, [ref]$pi)) {
  throw "CreateProcess failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
}
"started pid $($pi.dwProcessId) on desktop $Desktop"

# The owner's foreground, from this (input-desktop) process, until the command exits.
$fgLog = "$Log.foreground"
Set-Content $fgLog ""
$last = -1; $bad = @()
while ([PtDesk]::WaitForSingleObject($pi.hProcess, 50) -ne 0) {
  $p = 0; [void][PtDesk]::GetWindowThreadProcessId([PtDesk]::GetForegroundWindow(), [ref]$p)
  if ($p -ne $last) {
    $image = (Get-Process -Id $p -ErrorAction SilentlyContinue).ProcessName
    Add-Content $fgLog "$((Get-Date).ToString('o')) $p $image"
    if ($image -and ($Check -contains "$image.exe")) { $bad += "$p $image" }
    $last = $p
  }
}
$code = 0; [void][PtDesk]::GetExitCodeProcess($pi.hProcess, [ref]$code)
[void][PtDesk]::CloseHandle($pi.hProcess); [void][PtDesk]::CloseHandle($pi.hThread)
[void][PtDesk]::CloseDesktop($desk)
"exit code $code"
"foreground changes: $((Get-Content $fgLog | Where-Object { $_ }).Count); spike-browser exes in front: $(if ($bad) { $bad -join ', ' } else { 'none' })"
