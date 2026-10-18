Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W8 {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out wPOINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public struct RECT { public int Left, Top, Right, Bottom; }
  public struct wPOINT { public int X; public int Y; }
}
"@
foreach ($n in 'space','WindowsTerminal') {
  $p = Get-Process -Name $n -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if ($p) {
    $r = New-Object W8+RECT
    [void][W8]::GetWindowRect($p.MainWindowHandle, [ref]$r)
    $ex = [W8]::GetWindowLong($p.MainWindowHandle, -20)
    $topmost = (($ex -band 0x00000008) -ne 0)
    Write-Output ('{0}: rect=({1},{2})-({3},{4}) topmost={5}' -f $n, $r.Left, $r.Top, $r.Right, $r.Bottom, $topmost)
  }
}
$c = New-Object W8+wPOINT
[void][W8]::GetCursorPos([ref]$c)
Write-Output ('cursor now at screen ({0},{1})' -f $c.X, $c.Y)
$fg = [W8]::GetForegroundWindow()
$fgPid = [uint32]0
$null = [W8]::GetWindowThreadProcessId($fg, [ref]$fgPid)
$fgProc = Get-Process -Id $fgPid -ErrorAction SilentlyContinue
Write-Output ('foreground: {0}' -f $(if ($fgProc) { $fgProc.ProcessName + ' ' + $fgProc.Id } else { '?' }))
