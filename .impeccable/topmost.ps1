param([string]$Proc = "space", [switch]$Restore)
# (Re)stacks the space window as HWND_TOPMOST without stealing focus
# (SWP_NOACTIVATE) so synthetic cursor input lands on it while another app
# (e.g. a fullscreen game) covers the screen. -Restore takes it back down.
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W7 {
  [DllImport("user32.dll")] public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
  public static readonly uint TOPMOST = 0xFFFF;   // HWND_TOPMOST
  public static readonly uint NOTOPMOST = 0xFFFE; // HWND_NOTOPMOST
}
"@
$p = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { Write-Error "process $Proc not found"; exit 2 }
$h = $p.MainWindowHandle
$after = [IntPtr]([W7]::TOPMOST)
if ($Restore) { $after = [IntPtr]([W7]::NOTOPMOST) }
$ok = [W7]::SetWindowPos($h, $after, 0, 0, 0, 0, 0x0013) # NOMOVE|NOSIZE|NOACTIVATE
Write-Output "SetWindowPos on $Proc hwnd=$h -> $(if ($Restore) { 'NOTOPMOST' } else { 'TOPMOST' }) ok=$ok"
