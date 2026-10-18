param([int]$X, [int]$Y, [int]$Notches = -3, [string]$Proc = "space")
# Wheel at a window-relative point (no click). Negative notches = wheel-up =
# zoom-in for the map's cursor-anchored wheel zoom. Refuses when javaw is
# foreground.
. (Join-Path $PSScriptRoot 'tauriwin.ps1')
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W7 {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][W7]::SetProcessDPIAware()
$space = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $space) { Write-Error "process space not found"; exit 2 }
$h = [WTFind]::Find($space.Id)
if ($h -eq [IntPtr]::Zero) { Write-Error "space Tauri window not found"; exit 2 }
$javaw = Get-Process javaw -ErrorAction SilentlyContinue
$fg = [W7]::GetForegroundWindow()
$fgPid = [uint32]0
$null = [W7]::GetWindowThreadProcessId($fg, [ref]$fgPid)
$fgProc = Get-Process -Id $fgPid -ErrorAction SilentlyContinue
if ($fgProc -and $fgProc.ProcessName -eq 'javaw') { Write-Output 'REFUSED: javaw is foreground'; exit 7 }
$r = New-Object W7+RECT
[void][W7]::GetWindowRect($h, [ref]$r)
[void][W7]::SetCursorPos($r.Left + $X, $r.Top + $Y)
Start-Sleep -Milliseconds 160
$d = [BitConverter]::ToUInt32([BitConverter]::GetBytes([int](-$Notches * 120)), 0)
[W7]::mouse_event(0x0800, 0, 0, $d, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 80
Write-Output "wheel $Notches notches at window($X,$Y) screen($($r.Left + $X), $($r.Top + $Y)) :: fg=$(if ($fgProc) { $fgProc.ProcessName }) javaw=$(if ($javaw) { $javaw.Id } else { 'none' })"
