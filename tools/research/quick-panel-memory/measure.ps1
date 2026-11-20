param([Parameter(Mandatory=$true)][string]$Stage,[Parameter(Mandatory=$true)][string]$RunRoot,[ValidateSet('empty','text','images')][string]$Dataset='images',[ValidateRange(0,3)][int]$SelectionCycles=0)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
if(Test-Path $RunRoot){throw 'Fresh RunRoot required'}
if(@(Get-CimInstance Win32_Process | Where-Object {$_.Name -in @('unic-desktop.exe','unicd.exe')}).Count){throw 'Foreign app/service exists'}
New-Item $RunRoot -ItemType Directory | Out-Null
Add-Type -Path "$Stage\native.cs";Add-Type -Path "$Stage\window-state.cs"
$env:UNIC_HOME="$RunRoot\p";$env:UNIC_SOCK='';$env:MYGO_AUTOMATION='';$env:UNIC_DESKTOP_E2E_PROCESS_FIXTURE='';$env:UNIC_DESKTOP_E2E_GUEST=''
$env:APPDATA="$RunRoot\a";$env:LOCALAPPDATA="$RunRoot\l";$env:UNIC_DESKTOP_E2E_PROFILE=$env:UNIC_HOME
$env:UNIC_UI_SKIN='windows';$env:UNIC_DESKTOP_E2E_WINDOW='1100x720';$env:UNIC_DESKTOP_E2E_THEME='light';$env:UNIC_MEMORY_RESEARCH="$RunRoot\runtime"
foreach($d in @($env:UNIC_HOME,$env:APPDATA,$env:LOCALAPPDATA)){New-Item $d -ItemType Directory | Out-Null}
[IO.File]::WriteAllText("$env:UNIC_HOME\e2e-owner",'desktop-e2e')
[IO.File]::WriteAllText("$env:UNIC_HOME\config.toml", "[telemetry]`nenabled=false`n[sync]`nautomatic=false`n[desktop]`nlanguage='en'`nquick_panel_shortcut=''`n")
$bin="$Stage\bin";$rows=[Collections.Generic.List[object]]::new();$identities=@{};$timing=[Collections.Generic.List[object]]::new();$seq=[MemoryNative]::GetClipboardSequenceNumber()
function Quote-Argument([string]$Value){
    if($Value.Length -gt 0 -and $Value -notmatch '[\s"]'){return $Value}
    return '"'+[regex]::Replace([regex]::Replace($Value,'(\\*)"','$1$1\"'),'(\\+)$','$1$1')+'"'
}
function Invoke-Task([string]$Program,[string[]]$ArgValues){
 $start=New-Object Diagnostics.ProcessStartInfo;$start.FileName=$Program;$start.Arguments=($ArgValues|ForEach-Object{Quote-Argument $_}) -join ' ';$start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $child=New-Object Diagnostics.Process;$child.StartInfo=$start
 if(-not $child.Start()){throw 'launch failed'}
 $stdout=$child.StandardOutput.ReadToEndAsync();$stderr=$child.StandardError.ReadToEndAsync()
 if(-not $child.WaitForExit(30000)){throw 'Unknown command outcome; do not replay'}
 if($child.ExitCode -notin @(0,4)){throw $stderr.Result};return $stdout.Result
}
function Invoke-Unic([string[]]$Values){$out=Invoke-Task "$bin\unic.exe" $Values;$out|Add-Content "$RunRoot\cli.jsonl";return $out}
function Role([string]$Name,[string]$Command){return (Invoke-Task "$bin\unic-desktop.exe" @('--role',$Name,'--command',$Command))|ConvertFrom-Json}
function RPC([string]$Name,[string]$Method,$Params){$ep=if($Name -eq 'quick-panel'){'automation-panel.sock'}else{'automation.sock'};return (Invoke-Task "$bin\helper.exe" @('rpc','--endpoint',"$env:UNIC_HOME\$ep",'--method',$Method,'--params',($Params|ConvertTo-Json -Compress -Depth 8)))|ConvertFrom-Json}
function Bind([string]$Name){$r=Role $Name probe;$p=Get-Process -Id $r.pid;if($p.Path -ine "$bin\unic-desktop.exe"){throw 'Role binary mismatch'};$pm=[uint16]0;$nm=[uint16]0;[void][MemoryNative]::IsWow64Process2($p.Handle,[ref]$pm,[ref]$nm);$identities[$Name]=@{pid=$p.Id;start=$p.StartTime.ToUniversalTime().ToString('o');exe=$p.Path;sha256=(Get-FileHash $p.Path).Hash;processMachine=$pm;nativeMachine=$nm;regionsAtBind=[MemoryNative]::Regions($p.Handle)};return $p}
function Phase([string]$Name,[int]$Seconds=20){$watch=[Diagnostics.Stopwatch]::StartNew();$sample=0;while($watch.Elapsed.TotalSeconds -lt $Seconds){foreach($role in @($identities.Keys)){$p=Get-Process -Id $identities[$role].pid -ErrorAction SilentlyContinue;if($null -eq $p){continue};$ws=[MemoryNative]::Resident($p.Handle);$wins=@([MemoryWindowState]::ForProcess($p.Id));$rows.Add(@{sample=$sample;phase=$Name;elapsed=$watch.Elapsed.TotalSeconds;utcMillis=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();role=$role;pid=$p.Id;workingSet=$p.WorkingSet64;kernelPeakWorkingSet=$p.PeakWorkingSet64;privateWorkingSet=$ws['privateWorkingSet'];privateCommit=$p.PrivateMemorySize64;cpu=$p.TotalProcessorTime.TotalSeconds;handles=$p.HandleCount;threads=$p.Threads.Count;gdi=[MemoryNative]::GetGuiResources($p.Handle,0);user=[MemoryNative]::GetGuiResources($p.Handle,1);visible=@($wins|Where-Object{$_.Visible}).Count;foreground=@($wins|Where-Object{$_.Foreground}).Count})};$sample++;Start-Sleep -Milliseconds 1000};$rows.ToArray()|ConvertTo-Json -Depth 6|Set-Content "$RunRoot\samples.json" -Encoding UTF8}
function Ready([string]$RoleName){$w=[Diagnostics.Stopwatch]::StartNew();do{try{$windows=RPC $RoleName windows @{};$windows|ConvertTo-Json -Depth 10|Set-Content "$RunRoot\ready-windows.json" -Encoding UTF8;$content=@($windows.windows|Where-Object{$_.Visible -and $_.Kind -eq 'content' -and ($RoleName -ne 'quick-panel' -or $_.title -notlike 'unic E2E*')});if($content.Count -eq 1){$tree=RPC $RoleName tree @{window=$content[0].ID};$tree|ConvertTo-Json -Depth 10|Set-Content "$RunRoot\ready-tree.json" -Encoding UTF8;if(@($tree.nodes|Where-Object{$_.role -in @('textField','search')}).Count){$timing.Add(@{event='observed-ready';role=$RoleName;millis=$w.Elapsed.TotalMilliseconds});return $content[0].ID}}}catch{$_.ToString()|Set-Content "$RunRoot\ready-error.txt"};Start-Sleep -Milliseconds 100}while($w.Elapsed.TotalSeconds -lt 25);throw 'Native content readiness timeout'}
function Open-Panel {
 $at=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();$watch=[Diagnostics.Stopwatch]::StartNew()
 $null=Role tray open-quick-panel;$null=Bind quick-panel;$script:panelWindow=Ready quick-panel;Focus-Panel
 $searchMillis=$watch.Elapsed.TotalMilliseconds
 if($Dataset -eq 'images'){
  do{
   $events=@();if(Test-Path "$RunRoot\runtime.latency.jsonl"){$events=@(Get-Content "$RunRoot\runtime.latency.jsonl"|ForEach-Object{$_|ConvertFrom-Json}|Where-Object{$_.pid -eq $identities['quick-panel'].pid -and $_.unixMillis -ge $at})}
   if($events.Count){if($events[-1].width -ne 2048 -or $events[-1].height -ne 1536){throw 'Original dimensions changed'};$timing.Add(@{event='panel-open';at=$at;searchObservedMillis=$searchMillis;originalReadyMillis=$events[-1].unixMillis-$at;originalWidth=$events[-1].width;originalHeight=$events[-1].height});return}
   Start-Sleep -Milliseconds 50
  }while($watch.Elapsed.TotalSeconds -lt 15)
  throw 'Selected original readiness timeout'
 }
 $timing.Add(@{event='panel-open';at=$at;searchObservedMillis=$searchMillis})
}
function Focus-Panel {
 $bound=@([MemoryWindowState]::ForProcess($identities['quick-panel'].pid)|Where-Object {$_.Visible -and $_.Title -like 'unic*' -and $_.Title -notlike 'unic E2E*'})
 if($bound.Count -lt 1){throw 'Missing owned panel HWND'}
 [void][MemoryNative]::SetForegroundWindow([IntPtr]$bound[0].Handle)
}
function Capture([string]$RoleName,[int]$ID,[string]$Name){$windows=RPC $RoleName windows @{};if(-not @($windows.windows|Where-Object{$_.ID -eq $ID -and $_.Visible}).Count){throw ('Capture target hidden: '+$RoleName)};$null=RPC $RoleName idle @{window=$ID;timeout=5000};$frame=RPC $RoleName capture @{window=$ID};[IO.File]::WriteAllBytes("$RunRoot\$Name.png",[Convert]::FromBase64String($frame.png));$tree=RPC $RoleName tree @{window=$ID};$tree|ConvertTo-Json -Depth 10|Set-Content "$RunRoot\$Name-tree.json" -Encoding UTF8
 if($RoleName -eq 'quick-panel'){
  $windows=RPC $RoleName windows @{}
  foreach($sat in @($windows.windows|Where-Object{$_.Visible -and $_.ID -ne $ID})){$f=RPC $RoleName capture @{window=$sat.ID};[IO.File]::WriteAllBytes("$RunRoot\$Name-satellite.png",[Convert]::FromBase64String($f.png))}
 }
}
try{
 $null=Invoke-Unic @('migrate','--initialize-empty','--json')
 $service=Start-Process "$bin\unicd.exe" -PassThru -RedirectStandardError "$RunRoot\s.err" -RedirectStandardOutput "$RunRoot\s.out"
 $readyClock=[Diagnostics.Stopwatch]::StartNew()
 $serviceReady=$false
 do{try{$null=Invoke-Task "$bin\helper.exe" @('api','--endpoint',"$env:UNIC_HOME\unic.sock",'--method','GET','--path','/v1/service');$serviceReady=$true;break}catch{if($service.HasExited){throw ('Service exited: '+(Get-Content "$RunRoot\s.err" -Raw))}};Start-Sleep -Milliseconds 100}while($readyClock.Elapsed.TotalSeconds -lt 25)
 if(-not $serviceReady){throw 'Service readiness timed out'}
 if($Dataset -ne 'empty'){for($i=0;$i -lt 30;$i++){$null=Invoke-Unic @('send','--to','all','--json',("Synthetic panel text {0:D3}. " -f $i))}}
 if($Dataset -eq 'images'){foreach($f in Get-ChildItem "$Stage\fixtures\*.png"){$null=Invoke-Unic @('send','--to','all','--png',$f.FullName,'--json')}}
 $history=Invoke-Task "$bin\helper.exe" @('api','--endpoint',"$env:UNIC_HOME\unic.sock",'--method','GET','--path','/v1/history?limit=50');$history|Set-Content "$RunRoot\seed-history.json";$expectedCount=if($Dataset -eq 'images'){42}elseif($Dataset -eq 'text'){30}else{0};if(@(($history|ConvertFrom-Json).items).Count -ne $expectedCount){throw 'Fixture history count mismatch'}
 $service=Get-Process -Id $service.Id
 $identities.service=@{pid=$service.Id;start=$service.StartTime.ToUniversalTime().ToString('o');exe=$service.Path;sha256=(Get-FileHash $service.Path).Hash}
 $launch=Start-Process "$bin\unic-desktop.exe" -PassThru -RedirectStandardError "$RunRoot\launch.err" -RedirectStandardOutput "$RunRoot\launch.out"
 $launch.WaitForExit(30000)|Out-Null
 $null=Bind tray;$null=Bind gui;$guiWindow=Ready gui
 Phase 'gui-only' 10;Capture gui $guiWindow 'gui'
 Open-Panel
 Phase 'panel-with-gui' 10;Capture quick-panel $panelWindow 'panel'
 $null=Role gui close;Start-Sleep -Milliseconds 500;if(-not (Role quick-panel status).visible){Open-Panel};Phase 'panel-gui-exited' 10;Capture quick-panel $panelWindow 'panel-alone'
 $null=Role quick-panel close;Phase 'hidden'
 for($i=1;$i -le 3;$i++){Open-Panel;Phase "reopen-$i" 5;$null=Role quick-panel close;Phase "rehide-$i" 10}
 Open-Panel;Capture quick-panel $panelWindow 'reopened'
 if($Dataset -eq 'images'){
  for($selection=1;$selection -le $SelectionCycles;$selection++){
   $at=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();$selectionWatch=[Diagnostics.Stopwatch]::StartNew()
   $null=RPC 'quick-panel' input @{window=$panelWindow;events=@(@{type='key';key='Down'})}
   do{
    $events=@(Get-Content "$RunRoot\runtime.latency.jsonl"|ForEach-Object{$_|ConvertFrom-Json}|Where-Object{$_.pid -eq $identities['quick-panel'].pid -and $_.unixMillis -ge $at})
    if($events.Count){break};Start-Sleep -Milliseconds 50
   }while($selectionWatch.Elapsed.TotalSeconds -lt 15)
   if(-not $events.Count){throw 'Changed selection original readiness timeout'}
   if($events[-1].width -ne 2048 -or $events[-1].height -ne 1536){throw 'Changed selection dimensions changed'}
   $timing.Add(@{event='panel-selection';selection=$selection;intentToOriginalMillis=$events[-1].unixMillis-$at;observedReadyMillis=$selectionWatch.Elapsed.TotalMilliseconds})
   Phase "selection-$selection" 5;Capture quick-panel $panelWindow "selected-$selection"
  }
  if($SelectionCycles){$null=Role quick-panel close;Phase 'selection-final-hidden' 20}
 }
 $null=Role tray quit
 $verdict='PASS'
}catch{$verdict='FAIL';$failure=$_.ToString()+" at "+$_.ScriptStackTrace;Write-Output $failure}finally{
 $owned=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath -and $_.ExecutablePath.StartsWith($bin+'\',[StringComparison]::OrdinalIgnoreCase)})
 foreach($p in $owned){Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue}
 $cleanupClock=[Diagnostics.Stopwatch]::StartNew()
 do{Start-Sleep -Milliseconds 100;$left=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath -and $_.ExecutablePath.StartsWith($bin+'\',[StringComparison]::OrdinalIgnoreCase)})}while($left.Count -and $cleanupClock.Elapsed.TotalSeconds -lt 20)
 $left=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath -and $_.ExecutablePath.StartsWith($bin+'\',[StringComparison]::OrdinalIgnoreCase)})
 $after=[MemoryNative]::GetClipboardSequenceNumber()
 if($left.Count -or $seq -ne $after){$verdict='FAIL';$failure+=' cleanup/clipboard invariant failed'}
 @{driverSHA256=(Get-FileHash $PSCommandPath).Hash;verdict=$verdict;failure=$failure;dataset=$Dataset;selectionCycles=$SelectionCycles;identities=$identities;timing=$timing;clipboardSequenceBefore=$seq;clipboardSequenceAfter=$after;ownedProcessesRemaining=$left.Count;os=(Get-CimInstance Win32_OperatingSystem|Select-Object Caption,BuildNumber,OSArchitecture)}|ConvertTo-Json -Depth 8|Set-Content "$RunRoot\receipt.json" -Encoding UTF8
 foreach($d in @('p','a','l')){Remove-Item "$RunRoot\$d" -Recurse -Force -ErrorAction SilentlyContinue}
}
if($verdict -ne 'PASS'){throw $failure}
