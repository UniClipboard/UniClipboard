param([Parameter(Mandatory=$true)][string]$Binary,[Parameter(Mandatory=$true)][string]$RunRoot,[ValidateSet('process','blank','kit','text')][string]$Mode='blank',[ValidateSet('light','dark')][string]$Theme='light')
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
if(Test-Path $RunRoot){throw 'RunRoot must be fresh'}
if(Get-Process unic-desktop -ErrorAction SilentlyContinue){throw 'Another measurement is active'}
New-Item $RunRoot -ItemType Directory | Out-Null
Add-Type -Path "$PSScriptRoot\native.cs"
$env:UNIC_HOME="$RunRoot\profile";$env:APPDATA="$RunRoot\appdata";$env:LOCALAPPDATA="$RunRoot\localappdata";$env:USERPROFILE="$RunRoot\user"
foreach($d in @($env:UNIC_HOME,$env:APPDATA,$env:LOCALAPPDATA,$env:USERPROFILE)){New-Item $d -ItemType Directory -Force | Out-Null}
$env:UNIC_MEMORY_THEME=$Theme;$env:MYGO_FRAME_STATS='all';$env:UNIC_UI_SKIN='windows';$env:UNIC_MEMORY_WINDOW_MODE=$Mode;$env:UNIC_MEMORY_RESEARCH="$RunRoot\runtime"
$p=Start-Process $Binary -PassThru -RedirectStandardOutput "$RunRoot\stdout" -RedirectStandardError "$RunRoot\stderr"
$rows=[Collections.Generic.List[object]]::new();$timer=[Diagnostics.Stopwatch]::StartNew()
try{
 while($timer.Elapsed.TotalSeconds -lt 45){
  $p.Refresh();if($p.HasExited){throw 'Window probe exited'}
  $ws=[MemoryNative]::Resident($p.Handle)
  $rows.Add([pscustomobject]@{elapsedSeconds=$timer.Elapsed.TotalSeconds;workingSet=$p.WorkingSet64;privateWorkingSet=$ws['privateWorkingSet'];privateBytes=$p.PrivateMemorySize64;handles=$p.HandleCount;gdi=[MemoryNative]::GetGuiResources($p.Handle,0);user=[MemoryNative]::GetGuiResources($p.Handle,1)})
  Start-Sleep -Seconds 1
 }
 $p.Refresh();$hwnd=$p.MainWindowHandle
 if($Mode -ne 'process'){& "$PSScriptRoot\capture-window.ps1" -Hwnd ($hwnd.ToInt64()) -Output "$RunRoot\window.png"}
 if($Mode -ne 'process' -and ($hwnd -eq 0 -or -not [MemoryNative]::IsWindowVisible($hwnd))){throw 'Visible window not established'}
 [pscustomobject]@{mode=$Mode;theme=$Theme;backend=$(if((Get-Content "$RunRoot\stderr" -Raw) -match 'drawn on the GPU'){'GPU'}else{'not established'});pid=$p.Id;start=$p.StartTime.ToUniversalTime().ToString('o');sha256=(Get-FileHash $Binary).Hash.ToLower();session=$p.SessionId;dpi=$(if($hwnd -ne 0){[MemoryNative]::GetDpiForWindow($hwnd)}else{0});regions=[MemoryNative]::Regions($p.Handle)} | ConvertTo-Json -Depth 4 | Set-Content "$RunRoot\identity.json" -Encoding UTF8
 $rows | ConvertTo-Json | Set-Content "$RunRoot\samples.json" -Encoding UTF8
 Write-Output "PASS window=$Mode pid=$($p.Id)"
}finally{$p.Refresh();if(-not $p.HasExited){Stop-Process -Id $p.Id;$p.WaitForExit(10000)|Out-Null}}
