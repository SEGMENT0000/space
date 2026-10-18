param([int]$X, [int]$Y, [string]$Proc = "space", [ValidateSet("left","double","right")][string]$Button = "left")
# Moves the cursor to window-relative coords (X,Y inside the space window) and
# clicks. Refuses when javaw is foreground or the point is not owned by space.
. (Join-Path $PSScriptRoot 'tauriwin.ps1')
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WClick {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(wPOINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flag);
  public struct RECT { public int Left, Top, Right, Bottom; }
  public struct wPOINT { public int X; public int Y; }
}
"@
[void][WClick]::SetProcessDPIAware()
$p = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Error "process $Proc not found"; exit 2 }
$h = [WTFind]::Find($p.Id)
if ($h -eq [IntPtr]::Zero) { Write-Error "space Tauri window not found (is it running?)"; exit 2 }
# --- foreground guard: never click while javaw is foreground
$fg = [WClick]::GetForegroundWindow()
$fgPid = [uint32]0
$null = [WClick]::GetWindowThreadProcessId($fg, [ref]$fgPid)
$fgProc = Get-Process -Id $fgPid -ErrorAction SilentlyContinue
if ($fgProc -and $fgProc.ProcessName -eq 'javaw') { Write-Output 'REFUSED: javaw is foreground'; exit 7 }
# --- point guard: the click point must be owned by this space window
$r = New-Object WClick+RECT
[void][WClick]::GetWindowRect($h, [ref]$r)
$sx = $r.Left + $X; $sy = $r.Top + $Y
$pt = New-Object WClick+wPOINT; $pt.X = $sx; $pt.Y = $sy
$ptHwnd = [WClick]::WindowFromPoint($pt)
$root = [WClick]::GetAncestor($ptHwnd, 2)  # GA_ROOT
if ($root -eq [IntPtr]::Zero) { $root = $ptHwnd }
$ptPid = [uint32]0
$null = [WClick]::GetWindowThreadProcessId($root, [ref]$ptPid)
if ($root -ne $h) {
  $ptProc = Get-Process -Id $ptPid -ErrorAction SilentlyContinue
  Write-Output "REFUSED: point ($sx,$sy) owned by $(if ($ptProc) { $ptProc.ProcessName } else { '?' })"
  exit 7
}
[void][WClick]::SetCursorPos($sx, $sy)
Start-Sleep -Milliseconds 120
switch ($Button) {
  "left"   { [WClick]::mouse_event(0x0002,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [WClick]::mouse_event(0x0004,0,0,0,[UIntPtr]::Zero) }
  "double" { for ($i=0; $i -lt 2; $i++) { [WClick]::mouse_event(0x0002,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 40; [WClick]::mouse_event(0x0004,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 110 } }
  "right"  { [WClick]::mouse_event(0x0008,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [WClick]::mouse_event(0x0010,0,0,0,[UIntPtr]::Zero) }
}
Write-Output "clicked $Button at window($X,$Y) screen($sx,$sy)"
