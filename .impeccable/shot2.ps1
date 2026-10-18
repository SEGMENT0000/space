param([string]$Out = "C:\Users\SEGMENT\Downloads\new\.impeccable\review\shot.png", [string]$Proc = "space")
# Captures the space window via PrintWindow (PW_RENDERFULLCONTENT) — does NOT
# steal focus and works on background/minimized-ish windows.
. (Join-Path $PSScriptRoot 'tauriwin.ps1')
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W3 {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][W3]::SetProcessDPIAware()
$p = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Error "process $Proc with a window not found"; exit 2 }
$h = [WTFind]::Find($p.Id)
if ($h -eq [IntPtr]::Zero) { Write-Error "space Tauri window not found"; exit 2 }
$r = New-Object W3+RECT
[void][W3]::GetWindowRect($h, [ref]$r)
$w = $r.Right - $r.Left; $hh = $r.Bottom - $r.Top
if ($w -le 0 -or $hh -le 0) { Write-Error "bad window rect"; exit 2 }
$bmp = New-Object System.Drawing.Bitmap $w, $hh
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [W3]::PrintWindow($h, $hdc, 2)  # 2 = PW_RENDERFULLCONTENT
$g.ReleaseHdc($hdc)
New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
if ($ok) { Write-Output "saved $Out ($w x $hh) via PrintWindow" } else { Write-Output "PrintWindow returned false; saved possibly blank $Out ($w x $hh)" }
