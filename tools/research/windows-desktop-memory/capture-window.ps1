param([Parameter(Mandatory=$true)][Int64]$Hwnd,[Parameter(Mandatory=$true)][string]$Output)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MemoryCapture {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int L,T,R,B;}
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int X,Y;}
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref Point p);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern int GetSystemMetrics(int n);
}
'@
$old=[MemoryCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))
$bitmap=$null;$graphics=$null
try{
 $target=[IntPtr]$Hwnd
 if([MemoryCapture]::GetForegroundWindow() -ne $target){[void][MemoryCapture]::SetForegroundWindow($target);Start-Sleep -Milliseconds 200}
 if([MemoryCapture]::GetForegroundWindow() -ne $target){throw 'Target foreground identity not established'}
 $rect=[MemoryCapture+Rect]::new();$point=[MemoryCapture+Point]::new()
 if(-not [MemoryCapture]::GetClientRect($target,[ref]$rect) -or -not [MemoryCapture]::ClientToScreen($target,[ref]$point)){throw 'Target client rect unavailable'}
 $left=[MemoryCapture]::GetSystemMetrics(76);$top=[MemoryCapture]::GetSystemMetrics(77)
 $right=$left+[MemoryCapture]::GetSystemMetrics(78);$bottom=$top+[MemoryCapture]::GetSystemMetrics(79)
 if($point.X -lt $left -or $point.Y -lt $top -or $point.X+$rect.R -gt $right -or $point.Y+$rect.B -gt $bottom){throw 'Target client extends outside the desktop'}
 $bitmap=[Drawing.Bitmap]::new($rect.R,$rect.B)
 $graphics=[Drawing.Graphics]::FromImage($bitmap)
 $graphics.CopyFromScreen($point.X,$point.Y,0,0,$bitmap.Size)
 $bitmap.Save($Output,[Drawing.Imaging.ImageFormat]::Png)
 [pscustomobject]@{width=$rect.R;height=$rect.B;sha256=(Get-FileHash $Output).Hash.ToLower();method='GDI CopyFromScreen of verified foreground client rectangle';hwnd=$Hwnd} | ConvertTo-Json | Set-Content "$Output.json" -Encoding UTF8
}finally{
 if($null -ne $graphics){$graphics.Dispose()};if($null -ne $bitmap){$bitmap.Dispose()}
 [void][MemoryCapture]::SetThreadDpiAwarenessContext($old)
}
