param([string]$Proc = "space")
# Restores space's minimized "Tauri Window" and activates it. Refuses when
# javaw is foreground.
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class W11 {
  public delegate bool EnumProc(IntPtr h, IntPtr lp);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int n);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, IntPtr e);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  private static IntPtr found = IntPtr.Zero;
  private static bool cb(IntPtr h, IntPtr lp) {
    uint pid; GetWindowThreadProcessId(h, out pid);
    int want = 0; try { want = (int)lp; } catch {}
    if ((int)pid == want) {
      StringBuilder cls = new StringBuilder(256); GetClassName(h, cls, 256);
      if (cls.ToString() == "Tauri Window") { found = h; return false; }
    }
    return true;
  }
  public static IntPtr Find(int pid) {
    found = IntPtr.Zero;
    EnumWindows(cb, (IntPtr)pid);
    return found;
  }
}
"@
$space = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $space) { Write-Error "process space not found"; exit 2 }
$fg = [W11]::GetForegroundWindow()
$fgPid = [uint32]0
$null = [W11]::GetWindowThreadProcessId($fg, [ref]$fgPid)
$fgProc = Get-Process -Id $fgPid -ErrorAction SilentlyContinue
if ($fgProc -and $fgProc.ProcessName -eq 'javaw') { Write-Output 'REFUSED: javaw is foreground'; exit 7 }
$h = [W11]::Find($space.Id)
if ($h -eq [IntPtr]::Zero) { Write-Output 'no Tauri Window class found'; exit 3 }
$restored = [W11]::ShowWindow($h, 9)  # SW_RESTORE
Start-Sleep -Milliseconds 250
$fgThread = [W11]::GetWindowThreadProcessId([W11]::GetForegroundWindow(), [ref]$fgPid)
$mine = [W11]::GetCurrentThreadId()
$null = [W11]::AttachThreadInput($fgThread, $mine, $true)
[W11]::keybd_event(0x12, 0, 0, [IntPtr]::Zero)
[W11]::keybd_event(0x12, 0, 2, [IntPtr]::Zero)
[W11]::SetForegroundWindow($h) | Out-Null
[W11]::AttachThreadInput($fgThread, $mine, $false) | Out-Null
Start-Sleep -Milliseconds 300
$fg2Pid = [uint32]0
$null = [W11]::GetWindowThreadProcessId([W11]::GetForegroundWindow(), [ref]$fg2Pid)
$now = Get-Process -Id $fg2Pid -ErrorAction SilentlyContinue
Write-Output "restore: hwnd=0x$($h.ToInt64().ToString('X')) restored=$restored :: fg now = $($now.ProcessName) $($now.Id)"
