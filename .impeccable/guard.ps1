param([int]$X = -9999, [int]$Y = -9999, [string]$Proc = "space")
# Pre-click guard: refuses when (a) javaw is foreground, (b) the target point
# is not owned by space's window. -X/-Y skips the point check (fg-only probe).
. (Join-Path $PSScriptRoot 'tauriwin.ps1')
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W6 {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(wPOINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flag);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public struct wPOINT { public int X; public int Y; }
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
$space = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $space) { Write-Output 'guard: space window not found'; exit 2 }
$h = [WTFind]::Find($space.Id)
if ($h -eq [IntPtr]::Zero) { Write-Output 'guard: space Tauri window not found'; exit 2 }
$javaw = Get-Process javaw -ErrorAction SilentlyContinue
$fg = [W6]::GetForegroundWindow()
$fgPid = [uint32]0
$null = [W6]::GetWindowThreadProcessId($fg, [ref]$fgPid)
$fgProc = Get-Process -Id $fgPid -ErrorAction SilentlyContinue
$fgName = $(if ($fgProc) { $fgProc.ProcessName } else { '?' })
$sameFg = $fg -eq $h
$ptOwner = ''
$ptSame = $false
if ($X -ne -9999) {
  $r = New-Object W6+RECT
  [void][W6]::GetWindowRect($h, [ref]$r)
  $sx = $r.Left + $X; $sy = $r.Top + $Y
  $pt = New-Object W6+wPOINT; $pt.X = $sx; $pt.Y = $sy
  $ptHwnd = [W6]::WindowFromPoint($pt)
  $root = [W6]::GetAncestor($ptHwnd, 2)  # GA_ROOT: the WebView2 child's top-level
  if ($root -eq [IntPtr]::Zero) { $root = $ptHwnd }
  $ptPid = [uint32]0
  $null = [W6]::GetWindowThreadProcessId($root, [ref]$ptPid)
  $ptProc = Get-Process -Id $ptPid -ErrorAction SilentlyContinue
  $ptOwner = $(if ($ptProc) { $ptProc.ProcessName } else { '?' })
  $ptSame = $root -eq $h
}
$unsafe = ($fgName -eq 'javaw') -or ($X -ne -9999 -and -not $ptSame)
Write-Output "guard: fg=$fgName$(if($sameFg){' (=space)'} else {''}) point-owner=$ptOwner javaw=$(if ($javaw) { $javaw.Id } else { 'none' }) -> $(if ($unsafe) { 'UNSAFE' } else { 'safe' })"
exit $(if ($unsafe) { 7 } else { 0 })
