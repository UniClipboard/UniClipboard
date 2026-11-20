param(
 [Parameter(Mandatory=$true)][string]$BinaryDir,
 [Parameter(Mandatory=$true)][string]$RunRoot,
 [ValidateSet('empty','text','images')][string]$Dataset='empty',
 [int]$Repetition=1,
 [int]$Cycles=0,
 [switch]$Diagnostic,
 [switch]$Explore,
 [switch]$QuickOnly
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if(Test-Path $RunRoot){throw 'RunRoot must be fresh'}
New-Item $RunRoot -ItemType Directory | Out-Null
Add-Type -Path "$PSScriptRoot\native.cs"
$env:UNIC_HOME="$RunRoot\profile"
$env:UNIC_SOCK=''
$env:UC_PROFILE=''
$env:APPDATA="$RunRoot\appdata"
$env:LOCALAPPDATA="$RunRoot\localappdata"
$env:USERPROFILE="$RunRoot\user"
foreach($d in @($env:UNIC_HOME,$env:APPDATA,$env:LOCALAPPDATA,$env:USERPROFILE)){New-Item $d -ItemType Directory -Force | Out-Null}
$env:UNIC_UI_SKIN='windows'
$env:UNIC_MEMORY_RESEARCH="$RunRoot\runtime"
if(-not $Diagnostic){$env:MYGO_AUTOMATION='';$env:UNIC_DESKTOP_E2E_PROFILE=''}
if($Diagnostic){
 $env:UNIC_DESKTOP_E2E_PROFILE=$env:UNIC_HOME
 $env:MYGO_AUTOMATION="$env:UNIC_HOME\automation.sock"
 $env:UNIC_DESKTOP_E2E_WINDOW="1100x720"
 $env:UNIC_DESKTOP_E2E_THEME="light"
 [IO.File]::WriteAllText("$env:UNIC_HOME\e2e-owner","desktop-e2e")
}
@'
[telemetry]
enabled = false
[sync]
automatic = false
[desktop]
language = "en"
quick_panel_shortcut = ""
'@ | Set-Content "$env:UNIC_HOME\config.toml" -Encoding UTF8
# Initialize explicitly. Environment-owned v1 discovery paths are empty.
& "$BinaryDir\unic.exe" migrate --initialize-empty --json > "$RunRoot\initialization.jsonl"
if($LASTEXITCODE -ne 0){throw 'Explicit CLI initialization failed'}
$foreign=@(Get-CimInstance Win32_Process | Where-Object {$_.Name -eq 'unic-desktop.exe' -or $_.Name -eq 'unicd.exe'})
if($foreign.Count){throw 'Existing Desktop/service process; coordinate before replay'}
$service=$null;$desktop=$null;$locked=$false
$rows=[Collections.Generic.List[object]]::new()
$memoryWindowStateReady=$false
$sequenceBefore=[MemoryNative]::GetClipboardSequenceNumber()
function Sample([string]$phase,[double]$elapsed){
 foreach($p in @($desktop,$service)){
  if($null -eq $p){continue};$p.Refresh();if($p.HasExited){throw "Target exited in $phase"}
  $ws=[MemoryNative]::Resident($p.Handle)
  $row=[ordered]@{utcMillis=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();phase=$phase;elapsedSeconds=[math]::Round($elapsed,3);pid=$p.Id;role= $(if($p.Id -eq $desktop.Id){'desktop'}else{'service'});workingSet=$p.WorkingSet64;privateWorkingSet=$ws['privateWorkingSet'];sharedWorkingSet=$ws['sharedWorkingSet'];privateBytes=$p.PrivateMemorySize64;virtualBytes=$p.VirtualMemorySize64;handles=$p.HandleCount;threads=$p.Threads.Count;cpuSeconds=$p.TotalProcessorTime.TotalSeconds;gdi=[MemoryNative]::GetGuiResources($p.Handle,0);user=[MemoryNative]::GetGuiResources($p.Handle,1)}
  if($memoryWindowStateReady -and $p.Id -eq $desktop.Id){
   $state=@([MemoryWindowState]::ForProcess([uint32]$desktop.Id))
   $row.visibleOwnedWindows=@($state | Where-Object {$_.Visible}).Count
   $row.foregroundOwnedWindows=@($state | Where-Object {$_.Foreground}).Count
  }
  $rows.Add([pscustomobject]$row)
 }
}
function Phase([string]$name,[int]$seconds){
 $clock=[Diagnostics.Stopwatch]::StartNew()
 while($clock.Elapsed.TotalSeconds -lt $seconds){Sample $name $clock.Elapsed.TotalSeconds;Start-Sleep -Milliseconds 1000}
 $rows | ConvertTo-Json -Depth 5 | Set-Content "$RunRoot\samples.json" -Encoding UTF8
}
try {
 if($Dataset -ne 'empty'){
 $service=Start-Process "$BinaryDir\unicd.exe" -PassThru -RedirectStandardOutput "$RunRoot\service.out" -RedirectStandardError "$RunRoot\service.err"
 & (Join-Path (Split-Path $PSScriptRoot) 'service-probe.exe') ready
 if($LASTEXITCODE -ne 0){throw 'Private service readiness failed'}
 if($Dataset -ne 'empty'){
  for($i=0;$i -lt 60;$i++){
   & "$BinaryDir\unic.exe" send --to all --request-id "memory-text-$i" --json (("Synthetic memory fixture {0:D3}. " -f $i)*8) >> "$RunRoot\seed.jsonl"
   if($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 4){throw 'Text seeding failed'}
  }
 }
 if($Dataset -eq 'images'){
  foreach($f in Get-ChildItem "$PSScriptRoot\fixtures\*.png"){
   & "$BinaryDir\unic.exe" send --to all --png $f.FullName --json >> "$RunRoot\seed.jsonl"
   if($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 4){throw 'Image seeding failed'}
  }
 }
 & (Join-Path (Split-Path $PSScriptRoot) 'service-probe.exe') stop
 if($LASTEXITCODE -ne 0){throw 'Private seed service shutdown failed'}
 if(-not $service.WaitForExit(15000)){throw 'Private seed service did not exit'}; $service=$null
 }
 if(-not $Diagnostic){
  if(-not [MemoryNative]::OpenClipboard([IntPtr]::Zero)){throw 'Cannot isolate clipboard without reading it'}
  $locked=$true
 }
 $desktop=Start-Process "$BinaryDir\unic-desktop.exe" -PassThru -RedirectStandardOutput "$RunRoot\desktop.out" -RedirectStandardError "$RunRoot\desktop.err"
 $startup=[Diagnostics.Stopwatch]::StartNew()
 while($startup.Elapsed.TotalSeconds -lt 45){
  if($null -eq $service){$child=Get-CimInstance Win32_Process -Filter "ParentProcessId=$($desktop.Id)" | Where-Object {$_.Name -eq 'unicd.exe'} | Select-Object -First 1;if($child){$service=Get-Process -Id $child.ProcessId}}
  Sample 'visible' $startup.Elapsed.TotalSeconds
  Start-Sleep -Seconds 1
 }
 $desktop.Refresh();$hwnd=$desktop.MainWindowHandle
 if($hwnd -eq [IntPtr]::Zero -or -not [MemoryNative]::IsWindowVisible($hwnd)){throw 'No visible native target window'}
 if($null -eq $service){throw 'No Desktop-owned service'}
 if($Diagnostic){
  $auto=Join-Path (Split-Path $PSScriptRoot) 'mygo-automation.exe'
  & $auto open --socket $env:MYGO_AUTOMATION > "$RunRoot\automation-open.txt"
  & $auto snapshot --json > "$RunRoot\native-tree.json"
  $tree=Get-Content "$RunRoot\native-tree.json" -Raw
  if($tree -notmatch 'History' -or $tree -notmatch 'Settings'){throw 'Native history UI not ready'}
 }else{
  & (Join-Path (Split-Path $PSScriptRoot) 'service-probe.exe') ready
  if($LASTEXITCODE -ne 0){throw 'Exact-package service did not become ready'}
  & "$PSScriptRoot\capture-window.ps1" -Hwnd ($hwnd.ToInt64()) -Output "$RunRoot\exact-window.png"
 }
 $rect=[MemoryNative+Rect]::new();[void][MemoryNative]::GetClientRect($hwnd,[ref]$rect)
 $pm=[uint16]0;$nm=[uint16]0;[void][MemoryNative]::IsWow64Process2($desktop.Handle,[ref]$pm,[ref]$nm)
 $identity=[ordered]@{historyValidation=$(if($Diagnostic){'native automation history controls'}else{'target-window capture requires review'});scriptSHA256=(Get-FileHash "$PSCommandPath").Hash.ToLower();dataset=$Dataset;repetition=$Repetition;diagnostic=[bool]$Diagnostic;session=$desktop.SessionId;desktopPID=$desktop.Id;servicePID=$service.Id;desktopStart=$desktop.StartTime.ToUniversalTime().ToString('o');serviceStart=$service.StartTime.ToUniversalTime().ToString('o');processMachine=$pm;nativeMachine=$nm;dpi=[MemoryNative]::GetDpiForWindow($hwnd);clientWidth=$rect.Right;clientHeight=$rect.Bottom;desktopSHA256=(Get-FileHash "$BinaryDir\unic-desktop.exe").Hash.ToLower();serviceSHA256=(Get-FileHash "$BinaryDir\unicd.exe").Hash.ToLower();regionsVisible=[MemoryNative]::Regions($desktop.Handle);clipboard=$(if($Diagnostic){'private synthetic testboard'}else{'locked without read/write'});clipboardSequenceBefore=$sequenceBefore}
 $identity | ConvertTo-Json -Depth 5 | Set-Content "$RunRoot\identity.json" -Encoding UTF8
 if($Diagnostic){
  if($Dataset -eq 'images'){
   $cache=Get-Content "$RunRoot\runtime.cache.jsonl" -Tail 1 | ConvertFrom-Json
   if($cache.imageObjects -ne 12 -or $cache.fullImages -ne 1){throw 'Incomplete synthetic image previews'}
  }
  & "$PSScriptRoot\capture-window.ps1" -Hwnd ($hwnd.ToInt64()) -Output "$RunRoot\selected-native.png"
 }
 $identity.processTree=@(Get-CimInstance Win32_Process | Where-Object {$_.ProcessId -eq $desktop.Id -or $_.ProcessId -eq $service.Id} | Select-Object ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine)
 $identity.lifecycle='GUI remains alive when hidden to tray; independent tray/quickpanel roles not part of this source'
 # Real application close-to-tray and second-instance reopen.
 [void][MemoryNative]::PostMessage($hwnd,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
 Phase 'hidden' 30
 $identity.hiddenVerified=(-not [MemoryNative]::IsWindowVisible($hwnd))
 $activate=Start-Process "$BinaryDir\unic-desktop.exe" -PassThru -RedirectStandardOutput "$RunRoot\activation.out" -RedirectStandardError "$RunRoot\activation.err"
 $activate.WaitForExit(10000) | Out-Null
 Phase 'reopened' 30
 $identity.reopenVerified=[MemoryNative]::IsWindowVisible($hwnd)
 if($Diagnostic -and $Dataset -eq 'images'){
  $timings=@(Get-Content "$RunRoot\runtime.latency.jsonl" | ForEach-Object {$_ | ConvertFrom-Json})
  $before=$timings.Count
  $tree=& $auto snapshot --json --window 1 | ConvertFrom-Json
  $target=@($tree.nodes | Where-Object {$_.role -eq 'listItem' -and $_.key -match '^[0-9a-f]{32}$'})[2]
  if($null -eq $target){throw 'Missing synthetic selection target'}
  $watch=[Diagnostics.Stopwatch]::StartNew()
  & $auto press "id=$($target.id)" --window 1 --settle --timeout 5s > "$RunRoot\selection-action.txt"
  if($LASTEXITCODE -ne 0){throw 'Selection action failed'}
  do{
   $timings=@(Get-Content "$RunRoot\runtime.latency.jsonl" | ForEach-Object {$_ | ConvertFrom-Json})
   if($timings.Count -gt $before){break}
   Start-Sleep -Milliseconds 50
  }while($watch.Elapsed.TotalSeconds -lt 8)
  if($timings.Count -le $before){throw 'Selected original did not become ready'}
  $identity.selectionActionToObservedReadyMillis=$watch.Elapsed.TotalMilliseconds
  if($timings[-1].width -ne 2048 -or $timings[-1].height -ne 1536){throw 'Selected original dimensions changed'}
  Phase 'selection-settled' 10
  & "$PSScriptRoot\capture-window.ps1" -Hwnd ($hwnd.ToInt64()) -Output "$RunRoot\selected-after-native.png"
 }
 if($Explore -or $QuickOnly){. "$PSScriptRoot\explore.ps1"}
 if($Cycles -gt 0){
  for($i=1;$i -le $Cycles;$i++){
   [void][MemoryNative]::PostMessage($hwnd,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
   Phase "cycle-$i-hidden" 5
   $activate=Start-Process "$BinaryDir\unic-desktop.exe" -PassThru -RedirectStandardOutput "$RunRoot\activation.out" -RedirectStandardError "$RunRoot\activation.err";$activate.WaitForExit(10000) | Out-Null
   Phase "cycle-$i-reopened" 5
  }
  Phase 'final-idle' 60
 }
 if($Explore -or $QuickOnly){& "$PSScriptRoot\capture-window.ps1" -Hwnd ($hwnd.ToInt64()) -Output "$RunRoot\restored-native.png"}
 $identity.regionsFinal=[MemoryNative]::Regions($desktop.Handle)
 $identity.clipboardSequenceLocked=[MemoryNative]::GetClipboardSequenceNumber()
 $identity | ConvertTo-Json -Depth 5 | Set-Content "$RunRoot\identity.json" -Encoding UTF8
 $rows | ConvertTo-Json -Depth 5 | Set-Content "$RunRoot\samples.json" -Encoding UTF8
 Write-Output "PASS dataset=$Dataset desktop=$($desktop.Id) service=$($service.Id) samples=$($rows.Count)"
} finally {
 $rows | ConvertTo-Json -Depth 5 | Set-Content "$RunRoot\samples.json" -Encoding UTF8
 foreach($p in @($desktop,$service)){
  if($null -ne $p){$p.Refresh();if(-not $p.HasExited){$p.Kill(); $p.WaitForExit(10000) | Out-Null}}
 }
 $released=$false
 if($locked){$released=[MemoryNative]::CloseClipboard();if(-not $released){throw 'Task clipboard lock release failed'}}
 [pscustomobject]@{ownedProcessesStopped=$true;clipboardReleased=$released;profileRemoved=$false} | ConvertTo-Json | Set-Content "$RunRoot\cleanup.json" -Encoding UTF8
}
