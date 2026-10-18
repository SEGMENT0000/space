param([string]$Proc = "space")
# ALT-trick activation: attach-thread-input + ALT down/up + SetForegroundWindow.
# Proven at 13:51. NEVER run when javaw is foreground (guarded below).
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W5 {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("user32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, IntPtr e);
}
"@
$p = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { Write-Error "process $Proc not found"; exit 2 }
$h = $p.MainWindowHandle
$fg = [W5]::GetForegroundWindow()
$fgPid = [uint32]0
$null = [W5]::GetWindowThreadProcessId($fg, [ref]$fgPid)
$fgProc = Get-Process -Id $fgPid -ErrorAction SilentlyContinue
if ($fgProc -and $fgProc.ProcessName -eq 'javaw') { Write-Output 'REFUSED: javaw is foreground'; exit 7 }
$fgThread = [W5]::GetWindowThreadProcessId($fg, [ref]$fgPid)
$mine = [W5]::GetCurrentThreadId()
$attached = [W5]::AttachThreadInput($fgThread, $mine, $true)
[W5]::keybd_event(0x12, 0, 0, [IntPtr]::Zero)      # ALT down
[W5]::keybd_event(0x12, 0, 2, [IntPtr]::Zero)      # ALT up
[W5]::SetForegroundWindow($h) | Out-Null
[W5]::AttachThreadInput($fgThread, $mine, $false) | Out-Null
Start-Sleep -Milliseconds 350
$fg2 = [W5]::GetForegroundWindow()
$fgPid2 = [uint32]0
$null = [W5]::GetWindowThreadProcessId($fg2, [ref]$fgPid2)
$now = Get-Process -Id $fgPid2 -ErrorAction SilentlyContinue
Write-Output "activate: attached=$attached :: foreground now = $($now.ProcessName) $($now.Id)"
