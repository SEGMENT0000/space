param([string]$Out = "C:\Users\SEGMENT\Downloads\new\.impeccable\review\shot.png", [string]$Proc = "space")
# DEPRECATED — do not use this driver. It is CopyFromScreen-based: it captures
# whatever is actually IN FRONT, which during live QA was the user's fullscreen
# game (Minecraft), not the reviewed app. Always use shot2.ps1 (PrintWindow +
# PW_RENDERFULLCONTENT — focus-free and renders the named window's own content
# even when occluded).
# Captures the space app window (foregrounded) as a PNG.
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][W]::SetProcessDPIAware()
$p = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { Write-Error "process $Proc with a window not found"; exit 2 }
[void][W]::ShowWindow($p.MainWindowHandle, 9)
[void][W]::SetForegroundWindow($p.MainWindowHandle)
Start-Sleep -Milliseconds 700
$r = New-Object W+RECT
[void][W]::GetWindowRect($p.MainWindowHandle, [ref]$r)
$w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
if ($w -le 0 -or $h -le 0) { Write-Error "bad window rect"; exit 2 }
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Output "saved $Out ($w x $h)"
