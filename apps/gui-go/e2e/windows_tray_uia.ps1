# UI Automation helper for the Windows notification area, used by windows_tray_run.py. It only reads the shell UI and
# clicks the icon whose tooltip/name contains -Match (the sandboxed test host) or one menu item it was asked to.
#   -Action list                      names of taskbar / overflow / menu elements (evidence)
#   -Action icon -Match <text> -Button left|right   click the matching notification icon (opens the overflow first if needed)
#   -Action menu                      JSON list of the open popup menu's items (name, enabled, has submenu)
#   -Action choose -Name <text>       click the menu item with this exact name (or open the submenu)
param([string]$Action, [string]$Match = 'UniClipboard', [string]$Button = 'right', [string]$Name = '')
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class Mouse {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  public static void Click(int x, int y, bool right) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(120);
    uint down = right ? 0x8u : 0x2u, up = right ? 0x10u : 0x4u;
    mouse_event(down, 0, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(60); mouse_event(up, 0, 0, 0, UIntPtr.Zero);
  }
}
'@
$UIA = [System.Windows.Automation.AutomationElement]
$root = $UIA::RootElement
function Kids($el, $scope = 'Descendants') {
  $cond = [System.Windows.Automation.Condition]::TrueCondition
  $s = if ($scope -eq 'Children') { [System.Windows.Automation.TreeScope]::Children } else { [System.Windows.Automation.TreeScope]::Descendants }
  $el.FindAll($s, $cond)
}
function Find-Icon($text) {
  foreach ($e in Kids $root) {
    try {
      $n = $e.Current.Name
      if ($n -and $n -like "*$text*" -and $e.Current.ControlType.ProgrammaticName -match 'Button|MenuItem|ListItem' -and -not $e.Current.IsOffscreen) { return $e }
    } catch {}
  }
  return $null
}
function Menu-Root() {
  $c = New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, '#32768')
  $root.FindAll([System.Windows.Automation.TreeScope]::Children, $c)
}
switch ($Action) {
  'list' {
    foreach ($e in Kids $root) { try { $n = $e.Current.Name; if ($n -and -not $e.Current.IsOffscreen -and $e.Current.ClassName -match 'TrayWnd|Overflow|NotifyIcon|TaskListThumb|Xaml|SystemTray' -or $n -like "*$Match*" -or $n -like '*隐藏*' -or $n -like '*hidden*') { "$($e.Current.ControlType.ProgrammaticName)|$($e.Current.ClassName)|$n|$($e.Current.AutomationId)" } } catch {} }
  }
  'icon' {
    $icon = Find-Icon $Match
    if (-not $icon) {
      # the icon may sit in the overflow flyout: open the chevron ("show hidden icons")
      foreach ($e in Kids $root) { try { $n = $e.Current.Name; if ($n -match '隐藏|hidden|Show Hidden' -and $e.Current.ControlType.ProgrammaticName -match 'Button') { $r = $e.Current.BoundingRectangle; [Mouse]::Click([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2), $false); Start-Sleep -Milliseconds 700; break } } catch {} }
      $icon = Find-Icon $Match
    }
    if (-not $icon) { Write-Output 'ICON_NOT_FOUND'; exit 2 }
    $r = $icon.Current.BoundingRectangle
    [Mouse]::Click([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2), ($Button -eq 'right'))
    Write-Output "CLICKED $($icon.Current.Name) at $([int]$r.X),$([int]$r.Y) $([int]$r.Width)x$([int]$r.Height)"
  }
  'toast' {
    # click the toast whose text contains -Match (a notification banner or its Action Center entry)
    foreach ($e in Kids $root) { try { $n = $e.Current.Name; if ($n -and $n -like "*$Match*" -and -not $e.Current.IsOffscreen) { $r = $e.Current.BoundingRectangle; if ($r.Width -gt 0) { [Mouse]::Click([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2), $false); Write-Output "TOAST_CLICKED $n"; exit 0 } } } catch {} }
    Write-Output 'TOAST_NOT_FOUND'; exit 2
  }
  'menu' {
    Start-Sleep -Milliseconds 500
    $items = @()
    foreach ($m in Menu-Root) { foreach ($i in Kids $m) { try { if ($i.Current.ControlType.ProgrammaticName -match 'MenuItem') { $items += [pscustomobject]@{ name = $i.Current.Name; enabled = $i.Current.IsEnabled; offscreen = $i.Current.IsOffscreen } } } catch {} } }
    $items | ConvertTo-Json -Compress
  }
  'choose' {
    foreach ($m in Menu-Root) { foreach ($i in Kids $m) { try { if ($i.Current.ControlType.ProgrammaticName -match 'MenuItem' -and $i.Current.Name -eq $Name) { $r = $i.Current.BoundingRectangle; [Mouse]::Click([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2), $false); Write-Output "CHOSE $Name"; exit 0 } } catch {} } }
    Write-Output 'ITEM_NOT_FOUND'; exit 2
  }
}
