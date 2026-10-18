param([int]$ScanPid = 0)
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class W9 {
  public delegate bool EnumProc(IntPtr h, IntPtr lp);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
$targets = if ($ScanPid -gt 0) { @(, $ScanPid) } else { @(, ((Get-Process -Name space -ErrorAction SilentlyContinue).Id | ForEach-Object { [int]$_ })) }
$targetList = $targets | ForEach-Object { [int]$_ }
$cb = [W9+EnumProc]{ param($h, $lp)
  $pidU = [uint32]0
  $null = [W9]::GetWindowThreadProcessId($h, [ref]$pidU)
  if ($targetList -contains [int]$pidU) {
    $cls = New-Object System.Text.StringBuilder 256
    $clsN = [W9]::GetClassName($h, $cls, 256)
    $txt = New-Object System.Text.StringBuilder 256
    $txtN = [W9]::GetWindowText($h, $txt, 256)
    $r = New-Object W9+RECT
    [void][W9]::GetWindowRect($h, [ref]$r)
    $vis = [W9]::IsWindowVisible($h)
    $min = [W9]::IsIconic($h)
    Write-Output ("hwnd=0x{0:X} pid={1} vis={2} iconic={3} rect=({4},{5})-({6},{7}) class={8} title={9}" -f $h.ToInt64(), $pidU, $vis, $min, $r.Left, $r.Top, $r.Right, $r.Bottom, $cls.ToString(0, $clsN), $txt.ToString(0, $txtN))
  }
  return $true
}
try {
  $ok = [W9]::EnumWindows($cb, [IntPtr]::Zero)
  Write-Output "enum-returned: $ok"
} catch {
  Write-Output ("enum-exception: {0}" -f $_.Exception.Message)
}
