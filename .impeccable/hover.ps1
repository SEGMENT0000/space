param([int]$X, [int]$Y, [string]$Proc = "space")
# Moves the cursor to window-relative coords and leaves it there (hover), no click.
. (Join-Path $PSScriptRoot 'tauriwin.ps1')
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W4 {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][W4]::SetProcessDPIAware()
$p = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Error "process $Proc not found"; exit 2 }
$h = [WTFind]::Find($p.Id)
if ($h -eq [IntPtr]::Zero) { Write-Error "space Tauri window not found"; exit 2 }
$r = New-Object W4+RECT
[void][W4]::GetWindowRect($h, [ref]$r)
[void][W4]::SetCursorPos($r.Left + $X, $r.Top + $Y)
Write-Output "hover at window($X,$Y) screen($($r.Left + $X), $($r.Top + $Y))"
