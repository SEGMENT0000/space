Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WTFind {
  public delegate bool EnumProc(IntPtr h, IntPtr lp);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int n);
  private static IntPtr best = IntPtr.Zero;
  private static int bestScore;
  private static void consider(IntPtr h, int pid) {
    uint wpid; GetWindowThreadProcessId(h, out wpid);
    if ((int)wpid != pid) return;
    StringBuilder cls = new StringBuilder(256); GetClassName(h, cls, 256);
    if (cls.ToString() != "Tauri Window") return;
    int score = IsWindowVisible(h) ? (IsIconic(h) ? 1 : 3) : 0;
    if (score > bestScore) { bestScore = score; best = h; }
  }
  private static bool cb(IntPtr h, IntPtr lp) { consider(h, (int)lp); return true; }
  public static IntPtr Find(int pid) {
    best = IntPtr.Zero; bestScore = -1;
    EnumWindows(cb, (IntPtr)pid);
    return bestScore >= 0 ? best : IntPtr.Zero;
  }
}
"@
