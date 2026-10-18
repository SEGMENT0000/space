param([string]$Text = "", [string]$Esc = "no")
# Types into the focused element of the foreground window (space must BE
# foreground). Refuses when javaw is foreground.
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WType {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
}
"@
$fg = [WType]::GetForegroundWindow()
$fgPid = [uint32]0
$null = [WType]::GetWindowThreadProcessId($fg, [ref]$fgPid)
$fgProc = Get-Process -Id $fgPid -ErrorAction SilentlyContinue
if ($fgProc -and $fgProc.ProcessName -eq 'javaw') { Write-Output 'REFUSED: javaw is foreground'; exit 7 }
$sh = New-Object -ComObject WScript.Shell
if ($Text -ne "") { $sh.SendKeys($Text) | Out-Null; Start-Sleep -Milliseconds 120 }
if ($Esc -eq "esc") { $sh.SendKeys("{ESC}") | Out-Null }
Write-Output "typed '$Text' esc=$Esc"
