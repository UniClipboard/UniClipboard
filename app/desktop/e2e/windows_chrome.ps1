# OS-level evidence for the canonical window-chrome-native case only.
param(
    [Parameter(Mandatory=$true)][int]$OwnedProcessId,
    [Parameter(Mandatory=$true)][string]$OwnedBinary,
    [Parameter(Mandatory=$true)][string]$Mode,
    [Parameter(Mandatory=$true)][string]$OutputBase,
    [string]$Action = 'capture'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ChromeOS {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
 [DllImport("dwmapi.dll")] public static extern int DwmFlush();
 [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h,int attribute,out RECT r,int size);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
 [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
}
'@
[void][ChromeOS]::SetThreadDpiAwarenessContext([IntPtr](-4))
$owned = Get-Process -Id $OwnedProcessId
if ([IO.Path]::GetFullPath($owned.Path) -ne [IO.Path]::GetFullPath($OwnedBinary)) { throw 'Owned executable mismatch' }
$hwnd = $owned.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { throw 'Owned GUI has no native main window' }
function Rect {
 $r = New-Object ChromeOS+RECT
 if (-not [ChromeOS]::GetWindowRect($hwnd,[ref]$r)) { throw 'GetWindowRect failed' }
 return $r
}
function FocusOwned {
 [void][ChromeOS]::SetForegroundWindow($hwnd)
 Start-Sleep -Milliseconds 150
 if ([ChromeOS]::GetForegroundWindow() -ne $hwnd) { throw 'Owned GUI could not acquire foreground; no physical input delivered' }
}
function Pointer([int]$x,[int]$y) { [void][ChromeOS]::SetCursorPos($x,$y) }
function Down { [ChromeOS]::mouse_event(2,0,0,0,[UIntPtr]::Zero) }
function Up { [ChromeOS]::mouse_event(4,0,0,0,[UIntPtr]::Zero) }
$checks = @('exact owned PID and executable verified')
if ($Action -eq 'maximized') {
 if (-not [ChromeOS]::IsZoomed($hwnd)) { throw 'OS window did not maximize' }
 $checks += 'OS IsZoomed confirms maximize'
} elseif ($Action -eq 'restored') {
 if ([ChromeOS]::IsZoomed($hwnd)) { throw 'OS window did not restore' }
 $checks += 'OS IsZoomed confirms restore'
} elseif ($Action -eq 'minimized') {
 if (-not [ChromeOS]::IsIconic($hwnd)) { throw 'OS window did not minimize' }
 $checks += 'OS IsIconic confirms minimize'
 [void][ChromeOS]::ShowWindow($hwnd,9)
} elseif ($Action -eq 'drag-resize') {
 FocusOwned
 $before = Rect
 $origin = New-Object ChromeOS+POINT
 [void][ChromeOS]::ClientToScreen($hwnd,[ref]$origin)
 $scale = [ChromeOS]::GetDpiForWindow($hwnd)/96.0
 # General settings has no interactive controls in the center of its toolbar.
 $x = $origin.X + [int](250*$scale); $y = $origin.Y + [int](24*$scale)
 Pointer $x $y; Down; Start-Sleep -Milliseconds 60
 try { Pointer ($x+44) ($y+35); Start-Sleep -Milliseconds 200 } finally { Up }
 $moved = Rect
 if ($moved.Left -eq $before.Left -and $moved.Top -eq $before.Top) { throw 'Empty toolbar drag did not move the OS window' }
 $checks += 'physical drag on empty toolbar moved the OS window'
 Pointer ($moved.Right-1) ([int](($moved.Top+$moved.Bottom)/2)); Down
 try { Pointer ($moved.Right+47) ([int](($moved.Top+$moved.Bottom)/2)); Start-Sleep -Milliseconds 200 } finally { Up }
 $resized = Rect
 if (($resized.Right-$resized.Left) -le ($moved.Right-$moved.Left)) { throw 'Native right-edge resize did not grow the OS window' }
 $checks += 'physical native edge resize increased OS window width'
 # Use empty toolbar space for the native double-click maximize/restore path.
 $origin = New-Object ChromeOS+POINT
 [void][ChromeOS]::ClientToScreen($hwnd,[ref]$origin)
 Pointer ($origin.X+[int](250*$scale)) ($origin.Y+[int](24*$scale))
 Down; Up; Start-Sleep -Milliseconds 60; Down; Up; Start-Sleep -Milliseconds 200
 if (-not [ChromeOS]::IsZoomed($hwnd)) { throw 'Empty toolbar double-click did not maximize' }
 $checks += 'physical double-click on empty toolbar maximized OS window'
 [void][ChromeOS]::ShowWindow($hwnd,9)
} elseif ($Action -ne 'capture') { throw 'Unknown owned-window action' }
FocusOwned
if ([ChromeOS]::IsIconic($hwnd)) { throw 'Cannot capture a minimized window' }
if ([ChromeOS]::DwmFlush() -ne 0) { throw 'DWM did not flush the owned window frame' }
$rect = Rect
$origin = New-Object ChromeOS+POINT
[void][ChromeOS]::ClientToScreen($hwnd,[ref]$origin)
$dpi = [ChromeOS]::GetDpiForWindow($hwnd)
$topInset = ($origin.Y-$rect.Top)*96.0/$dpi
if ($Mode -eq 'system' -and $topInset -lt 23) { throw "Native title bar absent: top inset $topInset" }
if ($Mode -eq 'custom' -and $topInset -gt 20) { throw "Unexpected native title row in custom mode: top inset $topInset" }
$checks += "OS client/top-frame geometry confirms $Mode decoration"
$captureRect = New-Object ChromeOS+RECT
if ([ChromeOS]::DwmGetWindowAttribute($hwnd,9,[ref]$captureRect,16) -ne 0) { $captureRect = $rect }
$screenLeft = [ChromeOS]::GetSystemMetrics(76); $screenTop = [ChromeOS]::GetSystemMetrics(77)
$screenRight = $screenLeft+[ChromeOS]::GetSystemMetrics(78); $screenBottom = $screenTop+[ChromeOS]::GetSystemMetrics(79)
if ($captureRect.Left -lt $screenLeft -or $captureRect.Top -lt $screenTop -or $captureRect.Right -gt $screenRight -or $captureRect.Bottom -gt $screenBottom) { throw 'Full owned window is outside the desktop; refusing a partial OS screenshot' }
$bitmap = [Drawing.Bitmap]::new(($captureRect.Right-$captureRect.Left),($captureRect.Bottom-$captureRect.Top))
$graphics = [Drawing.Graphics]::FromImage($bitmap)
try {
 $graphics.CopyFromScreen($captureRect.Left,$captureRect.Top,0,0,$bitmap.Size)
 $bitmap.Save($OutputBase+'.png',[Drawing.Imaging.ImageFormat]::Png)
} finally { $graphics.Dispose(); $bitmap.Dispose() }
@{pid=$OwnedProcessId;binary=$OwnedBinary;mode=$Mode;action=$Action;dpi=$dpi;window_rect=$rect;capture_rect=$captureRect;client_origin=$origin;top_inset_logical=$topInset;checks=$checks;capture='System.Drawing.CopyFromScreen of exact owned foreground window including OS decorations'} | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 ($OutputBase+'.json')
