# Called inside measure.ps1 after the reopened baseline. It uses only private
# synthetic data and the existing mygo automation plugin; never Copy/Paste/Apply.
function Tree([int]$window=1){
 $raw=& $auto snapshot --json --window $window
 if($LASTEXITCODE -ne 0){throw 'Native snapshot failed'}
 return ($raw | ConvertFrom-Json)
}
function Press([string]$role,[string]$label,[int]$window=1){
 $node=(Tree $window).nodes | Where-Object {$_.role -eq $role -and $_.label -eq $label} | Select-Object -First 1
 if(-not $node){throw "Missing synthetic control: $role/$label"}
 & $auto press "id=$($node.id)" --window $window --settle --timeout 5s > "$RunRoot\action.txt"
 if($LASTEXITCODE -ne 0){throw "Native press failed: $role/$label"}
}
if(-not $QuickOnly){
Press 'radio' 'Grid'
Phase 'grid' 30
Press 'radio' 'List'
$row=(Tree).nodes | Where-Object {$_.role -eq 'listItem' -and $_.key -match '^[0-9a-f]{32}$'} | Select-Object -First 1
if(-not $row){throw 'Missing synthetic history row'}
& $auto press "id=$($row.id)" --window 1 --settle --timeout 5s > "$RunRoot\action.txt"
Phase 'selected-image' 30
$visibleRows=@((Tree).nodes | Where-Object {$_.role -eq 'listItem' -and $_.key -match '^[0-9a-f]{32}$'} | Select-Object -First 8)
$i=0
foreach($candidate in $visibleRows){
 $fresh=(Tree).nodes | Where-Object {$_.key -eq $candidate.key -and $_.role -eq 'listItem'} | Select-Object -First 1
 if($fresh){
  & $auto press "id=$($fresh.id)" --window 1 --settle --timeout 5s > "$RunRoot\action.txt"
  Phase "image-selection-$i" 3
  $i++
 }
}
Phase 'after-image-selections' 30
& "$PSScriptRoot\capture-window.ps1" -Hwnd ($hwnd.ToInt64()) -Output "$RunRoot\selected.png"
(Tree) | ConvertTo-Json -Depth 30 | Set-Content "$RunRoot\selected-tree.json" -Encoding UTF8
# Scroll both ways through the real native view. Keep selected ownership.
foreach($direction in @(1,-1)){
 for($j=0;$j -lt 12;$j++){
  & $auto wheel 350 480 0 ($direction*550) --window 1 > "$RunRoot\action.txt"
  Phase "scroll-$direction-$j" 1
 }
}
Phase 'after-scroll-idle' 30
}
# Open from the Settings button. Opening from the More menu auto-dismissed
# in the first native probe; retain that failure separately.
& $auto press 'key=settings' --window 1 --settle --timeout 5s > "$RunRoot\action.txt"
& $auto press 'key=quickPanel' --window 1 --settle --timeout 5s > "$RunRoot\action.txt"
Add-Type -Path "$PSScriptRoot\window-state.cs"
$memoryWindowStateReady=$true
Phase 'settings-before-quick' 15
$open=(Tree).nodes | Where-Object {$_.role -eq 'button' -and $_.label -match 'Open.*Quick.*Panel'} | Select-Object -First 1
if(-not $open){throw 'Missing Settings quick-panel button'}
& $auto press "id=$($open.id)" --window 1 --settle --timeout 5s > "$RunRoot\action.txt"
$nativeStart=@([MemoryWindowState]::ForProcess([uint32]$desktop.Id))
$nativeStart | ConvertTo-Json -Depth 5 | Set-Content "$RunRoot\quick-native-start.json" -Encoding UTF8
Phase 'quick-panel' 30
$nativeEnd=@([MemoryWindowState]::ForProcess([uint32]$desktop.Id))
$nativeEnd | ConvertTo-Json -Depth 5 | Set-Content "$RunRoot\quick-native-end.json" -Encoding UTF8
$w=$nativeEnd | Where-Object {$_.Handle -ne $hwnd.ToInt64() -and $_.Visible -and $_.Foreground} | Select-Object -First 1
$identity.quickVisibleAtCheckpoint=($null -ne $w)
if($null -ne $w){
 & "$PSScriptRoot\capture-window.ps1" -Hwnd $w.Handle -Output "$RunRoot\quick-native.png"
 & $auto key Escape --window 2 --settle --timeout 5s > "$RunRoot\action.txt"
}
& $auto press 'key=back' --window 1 --settle --timeout 5s > "$RunRoot\action.txt"
Phase 'quick-dismissed' 30
$desktop.Refresh();$identity.modules=@($desktop.Modules | Select-Object ModuleName,ModuleMemorySize)
