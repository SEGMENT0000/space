param([string[]]$Files, [string[]]$Points)
Add-Type -AssemblyName System.Drawing
# Points are "name,x,y" in capture coordinates (1336x899).
foreach ($f in $Files) {
  $img = [System.Drawing.Bitmap]::FromFile($f)
  Write-Output "=== $([System.IO.Path]::GetFileName($f)) ==="
  foreach ($pt in $Points) {
    $p = $pt -split ','
    $x = [int]$p[1]; $y = [int]$p[2]
    $c = $img.GetPixel($x, $y)
    Write-Output ("{0,-12} ({1,4},{2,4})  R={3,3} G={4,3} B={5,3}  hex #{6:X2}{7:X2}{8:X2}" -f $p[0], $x, $y, $c.R, $c.G, $c.B, $c.R, $c.G, $c.B)
  }
  $img.Dispose()
}
