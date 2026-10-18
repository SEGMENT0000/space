param([int]$ScanPid = 0)
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class W10 {
  public delegate bool EnumProc(IntPtr h, IntPtr lp);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int n);
  public struct RECT { public int Left, Top, Right, Bottom; }
  private static bool cb(IntPtr h, IntPtr lp) {
    uint pid; GetWindowThreadProcessId(h, out pid);
    int want = 0; try { want = (int)lp; } catch {}
    if (want == 0 || (int)pid == want) {
      StringBuilder cls = new StringBuilder(256); GetClassName(h, cls, 256);
      RECT r; GetWindowRect(h, out r);
      bool vis = IsWindowVisible(h);
      bool icon = IsIconic(h);
      string line = string.Format("hwnd=0x{0:X} pid={1} vis={2} iconic={3} rect=({4},{5})-({6},{7}) class={8}", h.ToInt64(), pid, vis, icon, r.Left, r.Top, r.Right, r.Bottom, cls.ToString());
      lines.Add(line);
    }
    return true;
  }
  private static System.Collections.Generic.List<string> lines = new System.Collections.Generic.List<string>();
  public static string Scan(int pid) {
    lines = new System.Collections.Generic.List<string>();
    EnumWindows(cb, (IntPtr)pid);
    return string.Join("\n", lines.ToArray());
  }
}
"@
Write-Output ([W10]::Scan($ScanPid))
