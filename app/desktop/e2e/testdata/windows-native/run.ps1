[CmdletBinding()]
param(
    [ValidateSet('inspect','run')][string]$Action='inspect',
    [ValidateSet('lightweight','full')][string]$Phase='lightweight',
    [Parameter(Mandatory=$true)][string]$BinDir,
    [Parameter(Mandatory=$true)][string]$ArtifactDir,
    [Parameter(Mandatory=$true)][string]$SourceSHA,
    [string]$SourceReceipt='',
    [string]$TaskRoot='',
    [string]$TrayIconName='跨设备剪贴板同步',
    [switch]$OpenOverflow
)
$BinDir=[IO.Path]::GetFullPath($BinDir)
$ArtifactDir=[IO.Path]::GetFullPath($ArtifactDir)
$ErrorActionPreference='Stop'
Set-StrictMode -Version 2
if([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT){throw 'Windows interactive desktop required'}
if([Threading.Thread]::CurrentThread.ApartmentState -ne 'STA'){throw 'Run powershell.exe -NoProfile -STA -File run.ps1'}
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing,System.Windows.Forms
Add-Type -Path (Join-Path $PSScriptRoot 'native.cs') -ReferencedAssemblies System.Drawing
$Desktop=Join-Path $BinDir 'unic-desktop.exe'
$CLI=Join-Path $BinDir 'unic.exe'
$Daemon=Join-Path $BinDir 'unicd.exe'
$Helper=Join-Path $BinDir 'windows-native-helper.exe'
$Events=New-Object Collections.ArrayList
$Identities=New-Object Collections.ArrayList
$Handles=New-Object Collections.ArrayList
$Services=New-Object Collections.ArrayList
$ServiceBindings=New-Object Collections.ArrayList
$ScenarioStatus=@{lightweight_close_reopen='NOT_RUN';background_core_only='NOT_RUN';normal_gui_absent_hotkey='NOT_RUN';lightweight_owned_detach='NOT_RUN';fresh_owned_quit='NOT_RUN'}
$TaskProfiles=New-Object Collections.ArrayList
$ExternalTarget=$null
$Independent=$null
$Verdict='FAIL'
$Cleanup='PASS'
$FailureText=''
$TaskRootCreated=$false
$NativeMenus=0
$GlobalShortcutStatus='NOT_RUN'
$PreviousDPIContext=[UnicNativeB973]::SetThreadDpiAwarenessContext([IntPtr](-4))
if($PreviousDPIContext -eq [IntPtr]::Zero){throw 'Per-monitor DPI awareness could not be established'}
$MousePosition=[Windows.Forms.Cursor]::Position

function New-PrivateDirectory([string]$Path){
    if(Test-Path -LiteralPath $Path){throw "Refusing existing task directory: $Path"}
    [void][IO.Directory]::CreateDirectory($Path)
    $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User
    $acl=New-Object Security.AccessControl.DirectorySecurity
    $acl.SetOwner($sid)
    $acl.SetAccessRuleProtection($true,$false)
    $rule=New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow')
    $acl.AddAccessRule($rule)
    Set-Acl -LiteralPath $Path -AclObject $acl
}
function Event([string]$Kind,[string]$Name,$Data){
    [void]$Events.Add(@{at_utc=[DateTime]::UtcNow.ToString('o');kind=$Kind;name=$Name;data=$Data})
}
function Save-JSON([string]$Name,$Value){$Value|ConvertTo-Json -Depth 24|Set-Content -Encoding UTF8 -LiteralPath (Join-Path $ArtifactDir $Name)}
function Quote-Argument([string]$Value){
    if($Value.Length -gt 0 -and $Value -notmatch '[\s"]'){return $Value}
    return '"'+[regex]::Replace([regex]::Replace($Value,'(\\*)"','$1$1\"'),'(\\+)$','$1$1')+'"'
}
function Start-TaskProcess([string]$Program,[string[]]$ArgValues,[string]$TaskProfile){
    $start=New-Object Diagnostics.ProcessStartInfo
    $start.FileName=$Program
    $start.Arguments=($ArgValues|ForEach-Object{Quote-Argument $_}) -join ' '
    $start.UseShellExecute=$false
    $start.CreateNoWindow=$true
    $start.RedirectStandardOutput=$true
    $start.RedirectStandardError=$true
    foreach($key in @($start.EnvironmentVariables.Keys)){
        if($key.StartsWith('UNIC_') -or $key.StartsWith('MYGO_')){$start.EnvironmentVariables.Remove($key)}
    }
    if($TaskProfile){
        $start.EnvironmentVariables['UNIC_HOME']=$TaskProfile
        $start.EnvironmentVariables['UNIC_SOCK']=Join-Path $TaskProfile 'unic.sock'
        $start.EnvironmentVariables['UNIC_DESKTOP_E2E_PROFILE']=$TaskProfile
        $start.EnvironmentVariables['UNIC_RENDEZVOUS_URL']='http://127.0.0.1:1'
        $start.EnvironmentVariables['MYGO_AUTOMATION']=''
        $start.EnvironmentVariables.Remove('UNIC_DESKTOP_E2E_PROCESS_FIXTURE')
        $start.EnvironmentVariables.Remove('UNIC_DESKTOP_E2E_GUEST')
        $discovery=Join-Path (Split-Path $TaskProfile -Parent) ('discovery-'+(Split-Path $TaskProfile -Leaf))
        $start.EnvironmentVariables['HOME']=$discovery
        $start.EnvironmentVariables['USERPROFILE']=$discovery
        $start.EnvironmentVariables['LOCALAPPDATA']=Join-Path $discovery 'local'
        $start.EnvironmentVariables['APPDATA']=Join-Path $discovery 'roaming'
        $start.EnvironmentVariables['XDG_CONFIG_HOME']=Join-Path $discovery 'config'
        $start.EnvironmentVariables['XDG_DATA_HOME']=Join-Path $discovery 'data'
        $start.EnvironmentVariables['UC_PROFILE']=''
    }
    $process=New-Object Diagnostics.Process
    $process.StartInfo=$start
    if(-not $process.Start()){throw 'Process launch failed'}
    # Drain both streams asynchronously. GUI-subsystem commands still use pipes.
    $outTask=$process.StandardOutput.ReadToEndAsync()
    $errTask=$process.StandardError.ReadToEndAsync()
    return @{process=$process;stdout=$outTask;stderr=$errTask;program=$Program}
}
function Invoke-Task([string]$Program,[string[]]$ArgValues,[string]$TaskProfile){
    $child=Start-TaskProcess $Program $ArgValues $TaskProfile
    if(-not $child.process.WaitForExit(50000)){throw 'Task command outcome unknown; do not replay'}
    $text=$child.stdout.Result
    if($child.process.ExitCode -ne 0){throw ($child.stderr.Result.Trim())}
    return $text
}
function Role([string]$TaskProfile,[string]$RoleName,[string]$Command){
    $text=Invoke-Task $Desktop @('--role',$RoleName,'--command',$Command) $TaskProfile
    Event 'role-ipc' ($RoleName+' '+$Command) @{profile=$TaskProfile}
    return $text|ConvertFrom-Json
}
function Try-Role([string]$TaskProfile,[string]$RoleName){try{return Role $TaskProfile $RoleName 'probe'}catch{return $null}}
function API([string]$TaskProfile,[string]$Method,[string]$Path,[string]$Body='null'){
    $text=Invoke-Task $Helper @('api','--endpoint',(Join-Path $TaskProfile 'unic.sock'),'--method',$Method,'--path',$Path,'--params',$Body) $TaskProfile
    return $text|ConvertFrom-Json
}
function Wait-Until([scriptblock]$Predicate,[string]$Description,[int]$Seconds=25){
    $deadline=[DateTime]::UtcNow.AddSeconds($Seconds)
    do{if(& $Predicate){return};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $deadline)
    throw ('Timed out: '+$Description)
}
function Bind-Role([string]$TaskProfile,[string]$RoleName){
    $script:LastRole=$null
    Wait-Until {$script:LastRole=Try-Role $TaskProfile $RoleName;return $null -ne $script:LastRole} ($RoleName+' IPC readiness')
    $identity=$script:LastRole
    if($identity.profile -ne $TaskProfile -or $identity.role -ne $RoleName){throw 'Role profile identity mismatch'}
    $process=[Diagnostics.Process]::GetProcessById([int]$identity.pid)
    [void]$process.Handle
    $creation=$process.StartTime.ToUniversalTime().ToString('o')
    $image=$process.MainModule.FileName
    if([IO.Path]::GetFullPath($image) -ine [IO.Path]::GetFullPath($Desktop)){throw 'Role executable path mismatch'}
    $digest=(Get-FileHash -Algorithm SHA256 -LiteralPath $image).Hash.ToLowerInvariant()
    if($digest -ne $identity.binary_sha256){throw 'Role executable digest mismatch'}
    $bound=@{role=$RoleName;pid=$process.Id;created_utc=$creation;binary=$image;binary_sha256=$digest;ipc=$identity;process=$process;profile=$TaskProfile}
    [void]$Handles.Add($bound)
    [void]$Identities.Add(@{role=$RoleName;pid=$process.Id;created_utc=$creation;binary=$image;binary_sha256=$digest;ipc=$identity;profile=$TaskProfile})
    return $bound
}
function Find-Window($Bound,[string]$Title,[bool]$RequireForeground=$false){
    $script:FoundWindow=[IntPtr]::Zero
    Wait-Until {
        if($Bound.process.HasExited){throw 'Owned role exited before its window appeared'}
        $windows=@([UnicNativeB973]::Windows([int]$Bound.pid,$Title))
        Save-JSON ($Bound.role+'-window-observation.json') @{expected_title=$Title;windows=[UnicNativeB973]::OwnedWindows([int]$Bound.pid);role_status=(Role $Bound.profile $Bound.role 'status');foreground_pid=[UnicNativeB973]::Owner([UnicNativeB973]::GetForegroundWindow())}
        if($windows.Count -gt 1){throw 'Ambiguous owned native windows'}
        if($windows.Count -eq 1){$script:FoundWindow=$windows[0];return $true};return $false
    } ('native window '+$Title)
    if($RequireForeground){
        Wait-Until {return [UnicNativeB973]::Owner([UnicNativeB973]::GetForegroundWindow()) -eq $Bound.pid} ('native foreground owner '+$Bound.role)
        Event 'native-foreground-observation' $Bound.role @{pid=$Bound.pid;hwnd=$script:FoundWindow.ToInt64();foreground_hwnd=[UnicNativeB973]::GetForegroundWindow().ToInt64()}
    }
    return $script:FoundWindow
}
function Capture-Owned([IntPtr]$Window,[int]$ExpectedPID,[string]$Name){
    $bound=@($Handles|Where-Object{$_.pid -eq $ExpectedPID -and $_.role -eq 'quick-panel'})
    if($bound.Count -eq 1){
        $profile=$bound[0].profile
        $endpoint=Join-Path $profile 'automation-panel.sock'
        Wait-Until {
            $windows=Invoke-Task $Helper @('rpc','--endpoint',$endpoint,'--method','windows') $profile|ConvertFrom-Json
            $content=@($windows.windows|Where-Object{$_.Visible -and $_.Kind -eq 'content'})
            if($content.Count -ne 1){return $false}
            $script:PanelFrameParams=@{window=$content[0].ID}|ConvertTo-Json -Compress
            $tree=Invoke-Task $Helper @('rpc','--endpoint',$endpoint,'--method','tree','--params',$script:PanelFrameParams) $profile|ConvertFrom-Json
            Save-JSON ($Name+'-tree.json') $tree
            return @($tree.nodes|Where-Object{$_.role -eq 'textField' -and $_.label -eq '搜索快捷面板'}).Count -eq 1
        } 'rendered panel search field'
        $idleParams=@{window=($script:PanelFrameParams|ConvertFrom-Json).window;timeout=5000}|ConvertTo-Json -Compress
        $null=Invoke-Task $Helper @('rpc','--endpoint',$endpoint,'--method','idle','--params',$idleParams) $profile
        $frame=Invoke-Task $Helper @('rpc','--endpoint',$endpoint,'--method','capture','--params',$script:PanelFrameParams) $profile|ConvertFrom-Json
        [IO.File]::WriteAllBytes((Join-Path $ArtifactDir ($Name+'-app-frame.png')),[Convert]::FromBase64String($frame.png))
        [UnicNativeB973]::FlushCompositor()
        Event 'app-scoped-render-readiness' $Name @{window=($script:PanelFrameParams|ConvertFrom-Json).window;search_field=$true;native_visibility_checked_separately=$true}
    }
    $method=[UnicNativeB973]::Capture($Window,$ExpectedPID,(Join-Path $ArtifactDir ($Name+'.png')))
    Event 'native-capture' $Name @{hwnd=$Window.ToInt64();pid=$ExpectedPID;dpi=[UnicNativeB973]::GetDpiForWindow($Window);method=$method}
}
function Close-OwnedGUI($Bound){
    $window=Find-Window $Bound 'unic'
    if($Bound.process.HasExited -or [UnicNativeB973]::Owner($window) -ne $Bound.pid){throw 'WM_CLOSE target identity changed'}
    if(-not [UnicNativeB973]::PostMessage($window,0x0010,[IntPtr]::Zero,[IntPtr]::Zero)){throw 'WM_CLOSE was not delivered'}
    Event 'native-WM_CLOSE' 'close GUI' @{pid=$Bound.pid;created_utc=$Bound.created_utc;hwnd=$window.ToInt64()}
    if(-not $Bound.process.WaitForExit(15000)){throw 'GUI retained its OS process after WM_CLOSE'}
    if($null -ne (Try-Role $Bound.profile 'gui')){throw 'GUI role still active after WM_CLOSE'}
}
function UIA-Record($Element){
    $current=$Element.Current
    return @{name=$current.Name;class=$current.ClassName;automation_id=$current.AutomationId;process_id=$current.ProcessId;hwnd=$current.NativeWindowHandle;offscreen=$current.IsOffscreen;runtime_id=($Element.GetRuntimeId()-join '.');rect=@{x=$current.BoundingRectangle.X;y=$current.BoundingRectangle.Y;width=$current.BoundingRectangle.Width;height=$current.BoundingRectangle.Height}}
}
function Tray-Roots {
    $condition=New-Object Windows.Automation.OrCondition((New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ClassNameProperty,'Shell_TrayWnd')),(New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ClassNameProperty,'TopLevelWindowForOverflowXamlIsland')))
    $roots=[Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,$condition)
    foreach($root in $roots){
        $shellProcess=[Diagnostics.Process]::GetProcessById($root.Current.ProcessId)
        if([IO.Path]::GetFileName($shellProcess.MainModule.FileName) -ine 'explorer.exe'){throw 'Notification-area root is not Explorer'}
        $root
    }
}
function Tray-Candidates {
    $found=New-Object Collections.ArrayList
    $condition=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,$TrayIconName)
    foreach($root in @(Tray-Roots)){
        foreach($element in $root.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)){
            if($element.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and -not $element.Current.IsOffscreen){[void]$found.Add($element)}
        }
    }
    return @($found.ToArray())
}
function Show-Overflow {
    if(-not $OpenOverflow){return}
    $roots=@(Tray-Roots)
    $visible=@($roots|Where-Object{$_.Current.ClassName -eq 'TopLevelWindowForOverflowXamlIsland' -and $_.Current.NativeWindowHandle -ne 0 -and [UnicNativeB973]::IsWindowVisible([IntPtr]$_.Current.NativeWindowHandle)})
    if($visible.Count -gt 1){throw 'Visible notification overflow is ambiguous'}
    if($visible.Count -eq 1 -and [UnicNativeB973]::Owner([UnicNativeB973]::GetForegroundWindow()) -eq $visible[0].Current.ProcessId){return}
    $condition=New-Object Windows.Automation.AndCondition((New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,'显示隐藏的图标')),(New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ClassNameProperty,'SystemTray.NormalButton')))
    $items=@($roots|Where-Object{$_.Current.ClassName -eq 'Shell_TrayWnd'}|ForEach-Object{$_.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)}|Where-Object{-not $_.Current.IsOffscreen})
    if($items.Count -eq 0){Event 'native-observation' 'notification overflow button absent' $null;return}
    if($items.Count -ne 1){throw 'Overflow button is not unique; inspect before continuing'}
    $script:OverflowRect=$null
    Wait-Until {
        $fresh=@(Tray-Roots|Where-Object{$_.Current.ClassName -eq 'Shell_TrayWnd'}|ForEach-Object{$_.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)}|Where-Object{-not $_.Current.IsOffscreen})
        if($fresh.Count -gt 1){throw 'Overflow button became ambiguous'}
        if($fresh.Count -ne 1){return $false}
        $script:OverflowRect=$fresh[0].Current.BoundingRectangle
        return -not [double]::IsNaN($script:OverflowRect.X) -and -not [double]::IsNaN($script:OverflowRect.Y) -and $script:OverflowRect.Width -gt 0 -and $script:OverflowRect.Height -gt 0
    } 'stable notification overflow button bounds'
    $rect=$script:OverflowRect
    [UnicNativeB973]::Mouse([int]($rect.X+$rect.Width/2),[int]($rect.Y+$rect.Height/2),$false)
    Event 'native-mouse' 'show notification overflow' (UIA-Record $items[0])
    Wait-Until {return @(Tray-Roots|Where-Object{$_.Current.ClassName -eq 'TopLevelWindowForOverflowXamlIsland' -and $_.Current.NativeWindowHandle -ne 0 -and [UnicNativeB973]::IsWindowVisible([IntPtr]$_.Current.NativeWindowHandle)}).Count -eq 1} 'visible notification overflow'
}
function Tray-Menu($Tray,[string]$ItemName,[string]$ReceiptName){
    Show-Overflow
    $candidates=@(Tray-Candidates)
    Save-JSON ($ReceiptName+'-tray-candidates.json') @($candidates|ForEach-Object{UIA-Record $_})
    if($candidates.Count -gt 1){throw 'Visible task tray candidate is ambiguous; no click delivered'}
    $script:OverflowReopenAttempts=0
    Wait-Until {
        $script:NativeIcon=[UnicNativeB973]::TrayRect([int]$Tray.pid,[uint32]3015896799)
        if($script:NativeIcon.HResult -ne 0){return $false}
        $script:VisibleRoots=@(Tray-Roots|Where-Object{
            $r=$_.Current.BoundingRectangle
            $_.Current.NativeWindowHandle -ne 0 -and [UnicNativeB973]::IsWindowVisible([IntPtr]$_.Current.NativeWindowHandle) -and $r.Left -le $script:NativeIcon.Left -and $r.Top -le $script:NativeIcon.Top -and $r.Right -ge $script:NativeIcon.Right -and $r.Bottom -ge $script:NativeIcon.Bottom
        })
        $fresh=@(Tray-Candidates|Where-Object{$r=$_.Current.BoundingRectangle;-not [double]::IsNaN($r.X) -and -not [double]::IsNaN($r.Y) -and [Math]::Abs($r.X-$script:NativeIcon.Left) -lt 3 -and [Math]::Abs($r.Y-$script:NativeIcon.Top) -lt 3})
        if($fresh.Count -gt 1){throw 'Task tray became ambiguous'}
        $openRoots=@(Tray-Roots|Where-Object{$_.Current.ClassName -eq 'TopLevelWindowForOverflowXamlIsland' -and [UnicNativeB973]::IsWindowVisible([IntPtr]$_.Current.NativeWindowHandle)})
        if($fresh.Count -eq 0 -and $openRoots.Count -eq 0 -and $script:OverflowReopenAttempts -lt 3){
            $script:OverflowReopenAttempts++
            Event 'native-observation' 'overflow observed closed; open from confirmed closed state' @{attempt=$script:OverflowReopenAttempts}
            Show-Overflow
            return $false
        }
        Save-JSON ($ReceiptName+'-icon-stability.json') @{icon=$script:NativeIcon;candidates=@($fresh|ForEach-Object{UIA-Record $_});all_exact_tooltip_candidates=@(Tray-Candidates|ForEach-Object{UIA-Record $_});roots=@(Tray-Roots|ForEach-Object{UIA-Record $_});foreground_pid=[UnicNativeB973]::Owner([UnicNativeB973]::GetForegroundWindow())}
        if($fresh.Count -ne 1 -or $script:VisibleRoots.Count -ne 1){return $false}
        $script:CandidateRect=$fresh[0].Current.BoundingRectangle
        return -not [double]::IsNaN($script:CandidateRect.X) -and -not [double]::IsNaN($script:CandidateRect.Y) -and [Math]::Abs($script:CandidateRect.X-$script:NativeIcon.Left) -lt 3 -and [Math]::Abs($script:CandidateRect.Y-$script:NativeIcon.Top) -lt 3 -and $script:CandidateRect.Width -gt 0 -and $script:CandidateRect.Height -gt 0
    } 'stable PID-bound visible task icon and UIA bounds'
    $nativeIcon=$script:NativeIcon;$visibleRoots=$script:VisibleRoots
    if($Tray.process.HasExited -or $Tray.process.SessionId -ne [Diagnostics.Process]::GetCurrentProcess().SessionId){throw 'Tray process session identity changed'}
    $iconBitmap=New-Object Drawing.Bitmap(($nativeIcon.Right-$nativeIcon.Left),($nativeIcon.Bottom-$nativeIcon.Top))
    $iconGraphics=[Drawing.Graphics]::FromImage($iconBitmap)
    try{$iconGraphics.CopyFromScreen($nativeIcon.Left,$nativeIcon.Top,0,0,$iconBitmap.Size);$iconBitmap.Save((Join-Path $ArtifactDir ($ReceiptName+'-icon.png')),[Drawing.Imaging.ImageFormat]::Png)}finally{$iconGraphics.Dispose();$iconBitmap.Dispose()}
    $candidateRect=$script:CandidateRect
    $hitPoint=New-Object Windows.Point([double]($candidateRect.X+$candidateRect.Width/2),[double]($candidateRect.Y+$candidateRect.Height/2))
    $hit=[Windows.Automation.AutomationElement]::FromPoint($hitPoint)
    Save-JSON ($ReceiptName+'-icon-hit.json') @{hit=(UIA-Record $hit);icon=$nativeIcon;cursor_before=[Windows.Forms.Cursor]::Position}
    [UnicNativeB973]::Mouse([int]$hitPoint.X,[int]$hitPoint.Y,$true)
    Save-JSON ($ReceiptName+'-cursor-after.json') @{cursor=[Windows.Forms.Cursor]::Position}
    Event 'native-mouse' 'right click PID-bound Shell notification icon' @{icon=$nativeIcon;root_class=$visibleRoots[0].Current.ClassName;uia_exact_candidates=$candidates.Count}
    $script:MenuItem=$null;$script:MenuWindow=[IntPtr]::Zero;$script:MenuObservation=@()
    Wait-Until {
        $ownerCondition=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty,[int]$Tray.pid)
        $ownedRoots=[Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,$ownerCondition)
        $script:MenuObservation=@(foreach($ownedRoot in $ownedRoots){UIA-Record $ownedRoot;foreach($e in $ownedRoot.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)){UIA-Record $e}})
        Save-JSON ($ReceiptName+'-owned-menu-observation.json') @{nodes=$script:MenuObservation;foreground_pid=[UnicNativeB973]::Owner([UnicNativeB973]::GetForegroundWindow());foreground_hwnd=[UnicNativeB973]::GetForegroundWindow().ToInt64();icon=$nativeIcon}
        $condition=New-Object Windows.Automation.AndCondition((New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty,$ItemName)),(New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::MenuItem)))
        $items=@($ownedRoots|ForEach-Object{$_.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)}|Where-Object{-not $_.Current.IsOffscreen})
        if($items.Count -gt 1){throw 'Owned menu item is ambiguous; no click delivered'}
        if($items.Count -eq 1){
            $ancestor=$items[0]
            for($depth=0;$depth -lt 12 -and $null -ne $ancestor;$depth++){
                $handle=[IntPtr]$ancestor.Current.NativeWindowHandle
                if($handle -ne [IntPtr]::Zero -and [UnicNativeB973]::Owner($handle) -eq $Tray.pid){$script:MenuWindow=$handle;break}
                $ancestor=[Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($ancestor)
            }
            if($script:MenuWindow -eq [IntPtr]::Zero){throw 'Cannot bind menu HWND to task tray PID'}
            $script:MenuItem=$items[0];return $true
        };return $false
    } ('owned native menu '+$ItemName) 8
    Wait-Until {return [UnicNativeB973]::Owner([UnicNativeB973]::GetForegroundWindow()) -eq $Tray.pid} 'owned native menu foreground' 8
    Capture-Owned $script:MenuWindow $Tray.pid ($ReceiptName+'-menu')
    $item=UIA-Record $script:MenuItem
    if($Tray.process.HasExited -or [UnicNativeB973]::Owner($script:MenuWindow) -ne $Tray.pid -or [UnicNativeB973]::Owner([UnicNativeB973]::GetForegroundWindow()) -ne $Tray.pid){throw 'Menu owner or foreground changed before click'}
    $rect=$script:MenuItem.Current.BoundingRectangle
    if($rect.Width -le 0 -or $rect.Height -le 0 -or $script:MenuItem.Current.IsOffscreen){throw 'Menu item bounds became unavailable'}
    [UnicNativeB973]::Mouse([int]($rect.X+$rect.Width/2),[int]($rect.Y+$rect.Height/2),$false)
    $script:NativeMenus++
    Event 'native-mouse' ('click tray menu '+$ItemName) @{tray_pid=$Tray.pid;menu_hwnd=$script:MenuWindow.ToInt64();item=$item}
}
function Save-Config([string]$TaskProfile,[string]$Mode){
    $text="[desktop]`nmode='$Mode'`nlanguage='zh-CN'`nlaunch_at_login=false`nstart_in_background=false`nquick_panel_shortcut='Ctrl+Alt+V'`n[telemetry]`nenabled=false`n[sync]`nenabled=false`n"
    $file=Join-Path $TaskProfile 'requested.toml'
    [IO.File]::WriteAllText($file,$text,(New-Object Text.UTF8Encoding($false)))
    $null=Invoke-Task $CLI @('config','apply',$file) $TaskProfile
    Event 'production-cli-config' ('save mode '+$Mode) @{profile=$TaskProfile;sha256=(Get-FileHash $file -Algorithm SHA256).Hash}
}
function New-TaskProfile([string]$Name,[string]$Mode){
    $TaskProfile=Join-Path $TaskRoot $Name
    New-PrivateDirectory $TaskProfile
    $TaskProfile=(Invoke-Task $Helper @('canonical',$TaskProfile) '').Trim()
    [void]$TaskProfiles.Add($TaskProfile)
    foreach($socket in @('unic.sock','automation.sock','automation-panel.sock')){if([Text.Encoding]::UTF8.GetByteCount((Join-Path $TaskProfile $socket)) -ge 104){throw 'Task socket path too long; supply a shorter new -TaskRoot'}}
    [IO.File]::WriteAllText((Join-Path $TaskProfile 'e2e-owner'),'desktop-e2e')
    [IO.File]::WriteAllText((Join-Path $TaskProfile 'config.toml'),"[telemetry]`nenabled=false`n[desktop]`nmode='$Mode'`nlanguage='zh-CN'`nquick_panel_shortcut='Ctrl+Alt+V'`n[sync]`nenabled=false`n")
    $null=Invoke-Task $CLI @('migrate','--initialize-empty','--json') $TaskProfile
    [IO.File]::WriteAllText((Join-Path $TaskProfile 'desktop-setup.json'),'{"complete":true}')
    return $TaskProfile
}
function Launch-Desktop([string]$TaskProfile){
    $launcher=Start-TaskProcess $Desktop @() $TaskProfile
    if(-not $launcher.process.WaitForExit(45000)){throw 'Desktop launcher did not finish'}
    if($launcher.process.ExitCode -ne 0){throw $launcher.stderr.Result}
    $tray=Bind-Role $TaskProfile 'tray'
    $script:RuntimeStatus=$null
    Wait-Until {
        $script:RuntimeStatus=Role $TaskProfile 'tray' 'status'
        if($script:RuntimeStatus.shortcut.error){throw 'Global shortcut registration failed; no input will be injected'}
        return $script:RuntimeStatus.ready -and $script:RuntimeStatus.shortcut.requested -eq 'Ctrl+Alt+V' -and $script:RuntimeStatus.shortcut.effective -eq 'Ctrl+Alt+V'
    } 'tray runtime and asynchronous shortcut registration'
    Event 'registration-receipt' 'shortcut requested/effective' $script:RuntimeStatus.shortcut
    return $tray
}
function Wait-RolesExit([string]$TaskProfile){
    foreach($bound in @($Handles|Where-Object{$_.profile -eq $TaskProfile})){
        if(-not $bound.process.WaitForExit(15000)){throw ('Owned role did not exit: '+$bound.role)}
    }
    foreach($roleName in @('gui','quick-panel','tray')){if($null -ne (Try-Role $TaskProfile $roleName)){throw ('Role endpoint survived Quit: '+$roleName)}}
}
function Observe-Service([string]$TaskProfile){
    $instance=API $TaskProfile 'GET' '/v1/service'
    if($instance.directory -ne $TaskProfile){throw 'Service profile identity mismatch'}
    $process=[Diagnostics.Process]::GetProcessById([int]$instance.pid)
    [void]$process.Handle
    [void]$Services.Add($process)
    if($process.MainModule.FileName -ine [IO.Path]::GetFullPath($Daemon)){throw 'Service executable mismatch'}
    Event 'service-identity' 'observe service' @{instance=$instance;created_utc=$process.StartTime.ToUniversalTime().ToString('o');binary_sha256=(Get-FileHash $Daemon -Algorithm SHA256).Hash}
    $bound=@{instance=$instance;process=$process;profile=$TaskProfile}
    [void]$ServiceBindings.Add($bound)
    return $bound
}
function Assert-Service($Expected,[string]$ExpectedMode=''){
    $current=API $Expected.profile 'GET' '/v1/service'
    if($Expected.process.HasExited -or $current.id -ne $Expected.instance.id -or $current.pid -ne $Expected.instance.pid -or $current.directory -ne $Expected.profile){throw 'Observed service identity changed'}
    if($ExpectedMode -and ($current.mode -ne $ExpectedMode -or $current.retiring)){throw ('Expected non-retiring service mode '+$ExpectedMode)}
    Event 'service-continuity' 'same generation and OS process' @{instance=$current;created_utc=$Expected.process.StartTime.ToUniversalTime().ToString('o')}
}
function Stop-ObservedService($Expected,[string]$Reason){
    if($Expected.process.HasExited){return}
    Assert-Service $Expected
    $null=API $Expected.profile 'POST' '/v1/service/stop' (@{id=$Expected.instance.id}|ConvertTo-Json -Compress)
    if(-not $Expected.process.WaitForExit(15000)){throw 'Driver-owned generation cleanup timed out'}
    Event 'driver-generation-cleanup' $Reason @{pid=$Expected.instance.pid;generation=$Expected.instance.id;profile=$Expected.profile}
}
function Show-VerifiedGUI([string]$TaskProfile,[string]$ReceiptName){
    $bound=Bind-Role $TaskProfile 'gui'
    $window=Find-Window $bound 'unic' $true
    Capture-Owned $window $bound.pid $ReceiptName
    return $bound
}
function Assert-NewGUI($Current,$Previous){
    if($Current.pid -eq $Previous.pid -and $Current.created_utc -eq $Previous.created_utc){throw 'Manual launch retained the previous GUI OS process'}
}
function Launch-BackgroundCoreOnly([string]$TaskProfile,$ExpectedService){
    $script:ScenarioStatus.background_core_only='RUNNING'
    # A completed lightweight background launch has no tray to bind.
    $background=Start-TaskProcess $Desktop @('--background') $TaskProfile
    if(-not $background.process.WaitForExit(45000)){throw 'Lightweight background launcher did not finish'}
    if($background.process.ExitCode -ne 0){throw $background.stderr.Result}
    Wait-RolesExit $TaskProfile
    Assert-Service $ExpectedService 'standalone'
    Event 'manual-background-launch' 'lightweight starts only core' @{profile=$TaskProfile;arguments=@('--background');gui=$false;tray=$false;quick_panel=$false}
    $script:ScenarioStatus.background_core_only='PASS'
}
function Invoke-GUIAbsentHotkey([string]$TaskProfile,$Tray){
    $script:ScenarioStatus.normal_gui_absent_hotkey='RUNNING'
    if($null -ne (Try-Role $TaskProfile 'gui')){throw 'GUI must be absent before independent hotkey test'}
    if($null -ne (Try-Role $TaskProfile 'quick-panel')){throw 'First-open hotkey test requires no previous panel process'}
    $targetFile=Join-Path $TaskRoot 'target.json'
    $targetTitle='UNIC task '+[Guid]::NewGuid().ToString('N')
    $script:ExternalTarget=Start-TaskProcess (Join-Path $PSHOME 'powershell.exe') @('-NoProfile','-STA','-File',(Join-Path $PSScriptRoot 'target.ps1'),'-ReceiptPath',$targetFile,'-WindowTitle',$targetTitle) ''
    Wait-Until {return Test-Path -LiteralPath $targetFile} 'task external target'
    $target=Get-Content -Raw -LiteralPath $targetFile|ConvertFrom-Json
    if($target.pid -ne $ExternalTarget.process.Id){throw 'Target process identity mismatch'}
    $targetWindow=[IntPtr][long]$target.hwnd
    if([UnicNativeB973]::Owner($targetWindow) -ne $target.pid){throw 'Target HWND identity mismatch'}
    if(-not [UnicNativeB973]::SetForegroundWindow($targetWindow)){throw 'Foreground activation rejected; no hotkey injected'}
    Wait-Until {return [UnicNativeB973]::GetForegroundWindow() -eq $targetWindow} 'exact task target foreground'
    $shortcut=Role $TaskProfile 'tray' 'status'
    if($shortcut.shortcut.effective -ne 'Ctrl+Alt+V' -or $shortcut.shortcut.error){throw 'Shortcut is no longer effective; no key injected'}
    [UnicNativeB973]::CheckModifiers()
    $script:GlobalShortcutStatus='ATTEMPTED'
    [UnicNativeB973]::Hotkey()
    $script:GlobalShortcutStatus='INJECTED_NOT_YET_OBSERVED'
    Event 'native-SendInput' 'Ctrl+Alt+V first panel with no GUI' @{target_pid=$target.pid;target_created_utc=$target.created_utc;target_hwnd=$target.hwnd;gui_absent=$true;panel_first_open=$true;input_replayed=$false}
    $panel=Bind-Role $TaskProfile 'quick-panel'
    Save-JSON 'first-hotkey-role-ready.json' @{role=(Role $TaskProfile 'quick-panel' 'status');windows=[UnicNativeB973]::OwnedWindows([int]$panel.pid);foreground_pid=[UnicNativeB973]::Owner([UnicNativeB973]::GetForegroundWindow())}
    # native18 returned role.visible=true while its Win32 window stayed hidden.
    # Both actual visibility and foreground ownership remain mandatory here.
    $panelWindow=Find-Window $panel 'unic · 快捷面板' $true
    Capture-Owned $panelWindow $panel.pid 'global-shortcut-first-panel'
    if($panel.pid -eq $Tray.pid){throw 'Panel shares the tray OS process'}
    $script:GlobalShortcutStatus='PASS'
    $windows=Invoke-Task $Helper @('rpc','--endpoint',(Join-Path $TaskProfile 'automation-panel.sock'),'--method','windows') $TaskProfile|ConvertFrom-Json
    $content=@($windows.windows|Where-Object{$_.Visible -and $_.Kind -eq 'content'})
    if($content.Count -ne 1){throw 'Panel automation window is ambiguous'}
    $keyParams=@{window=$content[0].ID;events=@(@{type='key';key='Escape'})}|ConvertTo-Json -Depth 5 -Compress
    $null=Invoke-Task $Helper @('rpc','--endpoint',(Join-Path $TaskProfile 'automation-panel.sock'),'--method','input','--params',$keyParams) $TaskProfile
    Event 'app-scoped-automation' 'Escape dismiss panel' @{window=$content[0].ID;endpoint='automation-panel.sock'}
    Wait-Until {$status=Role $TaskProfile 'quick-panel' 'status';return -not $status.visible} 'panel dismissal'
    Wait-Until {return [UnicNativeB973]::GetForegroundWindow() -eq $targetWindow} 'Escape restored exact task target focus'
    $script:ScenarioStatus.normal_gui_absent_hotkey='PASS'
    return $panel
}

New-PrivateDirectory $ArtifactDir
try{
    foreach($file in @($Desktop,$CLI,$Daemon,$Helper)){if(-not (Test-Path -LiteralPath $file -PathType Leaf)){throw ('Missing task executable: '+$file)}}
    $sourceBinding=$null
    if($SourceReceipt){
        $sourceBinding=Get-Content -Raw -LiteralPath $SourceReceipt|ConvertFrom-Json
        if(-not $sourceBinding.source_head.StartsWith($SourceSHA)){throw 'Source receipt revision mismatch'}
    }
    $builds=@{}
    foreach($product in @(@{name='unic-desktop';path=$Desktop},@{name='unic';path=$CLI},@{name='unicd';path=$Daemon})){
        $info=Invoke-Task $Helper @('build-info',$product.path) ''|ConvertFrom-Json
        $builds[$product.name]=$info
        $revision=@($info.Settings|Where-Object{$_.Key -eq 'vcs.revision'})
        if($null -ne $sourceBinding){
            $expected=$sourceBinding.binary_sha256.($product.name)
            if(-not $expected -or $expected.ToLowerInvariant() -ne (Get-FileHash $product.path -Algorithm SHA256).Hash.ToLowerInvariant()){throw 'Source receipt binary digest mismatch'}
        }elseif($revision.Count -ne 1 -or -not $revision[0].Value.StartsWith($SourceSHA)){throw 'Product embedded VCS revision does not match requested source; supply an exact source/binary receipt for VCS-disabled builds'}
    }
    Save-JSON 'build-identity.json' @{dpi_context='PER_MONITOR_AWARE_V2 (task thread only)';system_dpi=[UnicNativeB973]::GetDpiForSystem();source_requested=$SourceSHA;build_info=$builds;binary_sha256=@{desktop=(Get-FileHash $Desktop -Algorithm SHA256).Hash;unic=(Get-FileHash $CLI -Algorithm SHA256).Hash;unicd=(Get-FileHash $Daemon -Algorithm SHA256).Hash;helper=(Get-FileHash $Helper -Algorithm SHA256).Hash};driver_sha256=(Get-FileHash $PSCommandPath -Algorithm SHA256).Hash;helper_source_sha256=(Get-FileHash (Join-Path $PSScriptRoot 'helper.go') -Algorithm SHA256).Hash;native_source_sha256=(Get-FileHash (Join-Path $PSScriptRoot 'native.cs') -Algorithm SHA256).Hash;target_source_sha256=(Get-FileHash (Join-Path $PSScriptRoot 'target.ps1') -Algorithm SHA256).Hash}
    if($SourceReceipt){Copy-Item -LiteralPath $SourceReceipt -Destination (Join-Path $ArtifactDir 'source-receipt.json')}
    Show-Overflow
    $BeforeTrayIDs=@(Tray-Candidates|ForEach-Object{$_.GetRuntimeId()-join '.'})
    $BeforeAllTrayIDs=@(foreach($root in @(Tray-Roots)){foreach($e in $root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)){$e.GetRuntimeId()-join '.'}})
    Save-JSON 'tray-inspect.json' @{at_utc=[DateTime]::UtcNow.ToString('o');expected_name=$TrayIconName;candidates=@(Tray-Candidates|ForEach-Object{UIA-Record $_});note='Exact-name tray candidates only; unrelated desktop text is not collected.'}
    $structure=New-Object Collections.ArrayList
    foreach($trayRoot in @(Tray-Roots)){
        $nodes=$trayRoot.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
        foreach($node in $nodes){
            $current=$node.Current
            [void]$structure.Add(@{root_class=$trayRoot.Current.ClassName;class=$current.ClassName;automation_id=$current.AutomationId;control_type=$current.ControlType.ProgrammaticName;process_id=$current.ProcessId;hwnd=$current.NativeWindowHandle;offscreen=$current.IsOffscreen;runtime_id=($node.GetRuntimeId()-join '.')})
            if($structure.Count -ge 240){break}
        }
        if($structure.Count -ge 240){break}
    }
    Save-JSON 'tray-structure.json' @{nodes=@($structure);names_collected=$false;scope='Explorer taskbar and notification-overflow roots only'}
    if($Action -eq 'inspect'){$Verdict='INSPECT_ONLY';return}
    if(-not $TaskRoot){$TaskRoot=Join-Path ([IO.Path]::GetTempPath()) ('un-'+[Guid]::NewGuid().ToString('N').Substring(0,7))}
    New-PrivateDirectory $TaskRoot
    $TaskRootCreated=$true
    $TaskRoot=[IO.Path]::GetFullPath($TaskRoot)
    $initialMode='lightweight'
    if($Phase -eq 'full'){$initialMode='normal'}
    $roundOne=New-TaskProfile 'a' $initialMode
    $Independent=Start-TaskProcess $Daemon @() $roundOne
    Wait-Until {try{$null=API $roundOne 'GET' '/v1/service';return $true}catch{return $false}} 'independent S0 service'
    $S0=Observe-Service $roundOne
    if($S0.process.Id -ne $Independent.process.Id){throw 'S0 is not the driver-created process'}
    $tray=Launch-Desktop $roundOne
    $initialGUI=Show-VerifiedGUI $roundOne 'initial-manual-gui'
    Assert-Service $S0 'standalone'

    if($Phase -eq 'full'){
        Close-OwnedGUI $initialGUI
        if($tray.process.HasExited){throw 'Normal GUI close stopped the tray'}
        Assert-Service $S0 'standalone'
        $panel=Invoke-GUIAbsentHotkey $roundOne $tray
        Tray-Menu $tray '打开主窗口' 'normal-tray-reopen'
        $initialGUI=Show-VerifiedGUI $roundOne 'normal-tray-reopened-gui'
        $uniquePIDs=@(@($tray.pid,$initialGUI.pid,$panel.pid,$S0.process.Id)|Select-Object -Unique)
        if($uniquePIDs.Count -ne 4){throw 'Roles do not have four independent OS processes'}
        Save-Config $roundOne 'lightweight'
    }
    $ScenarioStatus.lightweight_close_reopen='RUNNING'
    Close-OwnedGUI $initialGUI
    Wait-RolesExit $roundOne
    Assert-Service $S0 'standalone'
    Event 'native-lightweight-close' 'WM_CLOSE ended every UI role and preserved S0' @{profile=$roundOne;generation=$S0.instance.id}

    # Saved lightweight mode must still open a real GUI on a manual no-argument launch.
    $tray=Launch-Desktop $roundOne
    $lightGUI=Show-VerifiedGUI $roundOne 'light-manual-reopen-one'
    Assert-NewGUI $lightGUI $initialGUI
    Assert-Service $S0 'standalone'
    if($Phase -eq 'full'){
        Tray-Menu $tray '快捷面板' 'light-first-panel'
        $panel=Bind-Role $roundOne 'quick-panel'
        $panelWindow=Find-Window $panel 'unic · 快捷面板' $true
        Capture-Owned $panelWindow $panel.pid 'light-first-panel'
        $uniquePIDs=@(@($tray.pid,$lightGUI.pid,$panel.pid,$S0.process.Id)|Select-Object -Unique)
        if($uniquePIDs.Count -ne 4){throw 'Lightweight roles do not have four independent OS processes'}
    }
    Close-OwnedGUI $lightGUI
    Wait-RolesExit $roundOne
    Assert-Service $S0 'standalone'
    $tray=Launch-Desktop $roundOne
    $again=Show-VerifiedGUI $roundOne 'light-manual-reopen-two'
    Assert-NewGUI $again $lightGUI
    Close-OwnedGUI $again
    Wait-RolesExit $roundOne
    Assert-Service $S0 'standalone'
    $ScenarioStatus.lightweight_close_reopen='PASS'
    Launch-BackgroundCoreOnly $roundOne $S0
    Stop-ObservedService $S0 'stop S0 only after lightweight manual/background preservation'

    if($Phase -eq 'full'){
        # A tray-owned S1 becomes standalone without changing its OS identity.
        $ScenarioStatus.lightweight_owned_detach='RUNNING'
        $roundTwo=New-TaskProfile 'b' 'lightweight'
        $tray=Launch-Desktop $roundTwo
        $S1=Observe-Service $roundTwo
        if($S1.instance.mode -ne 'desktop'){throw 'S1 is not initially tray-owned'}
        $ownedGUI=Show-VerifiedGUI $roundTwo 'owned-light-manual-gui'
        Tray-Menu $tray '快捷面板' 'owned-light-panel'
        $ownedPanel=Bind-Role $roundTwo 'quick-panel'
        $ownedPanelWindow=Find-Window $ownedPanel 'unic · 快捷面板' $true
        Capture-Owned $ownedPanelWindow $ownedPanel.pid 'owned-light-panel'
        Close-OwnedGUI $ownedGUI
        Wait-RolesExit $roundTwo
        Assert-Service $S1 'standalone'
        Event 'native-owned-detach' 'lightweight WM_CLOSE retained S1 PID and generation as standalone' @{instance_before=$S1.instance;instance_after=(API $roundTwo 'GET' '/v1/service')}
        $tray=Launch-Desktop $roundTwo
        $reopenedOwnedGUI=Show-VerifiedGUI $roundTwo 'detached-service-manual-gui'
        Assert-NewGUI $reopenedOwnedGUI $ownedGUI
        Assert-Service $S1 'standalone'
        Tray-Menu $tray '退出整个应用' 'detached-light-native-quit'
        Wait-RolesExit $roundTwo
        Assert-Service $S1 'standalone'
        $ScenarioStatus.lightweight_owned_detach='PASS'
        Stop-ObservedService $S1 'driver stops detached S1 after native Quit preserved it'

        # Quit before any detach must still stop a fresh, tray-owned generation.
        $ScenarioStatus.fresh_owned_quit='RUNNING'
        $roundThree=New-TaskProfile 'c' 'lightweight'
        $tray=Launch-Desktop $roundThree
        $S2=Observe-Service $roundThree
        if($S2.instance.mode -ne 'desktop'){throw 'Fresh S2 is not tray-owned'}
        $freshGUI=Show-VerifiedGUI $roundThree 'fresh-owned-gui'
        Tray-Menu $tray '快捷面板' 'fresh-owned-panel'
        $freshPanel=Bind-Role $roundThree 'quick-panel'
        $freshPanelWindow=Find-Window $freshPanel 'unic · 快捷面板' $true
        Capture-Owned $freshPanelWindow $freshPanel.pid 'fresh-owned-panel'
        $freshPIDs=@(@($tray.pid,$freshGUI.pid,$freshPanel.pid,$S2.process.Id)|Select-Object -Unique)
        if($freshPIDs.Count -ne 4){throw 'Fresh owned roles share an OS process'}
        Tray-Menu $tray '退出整个应用' 'fresh-owned-native-quit'
        Wait-RolesExit $roundThree
        if(-not $S2.process.WaitForExit(15000)){throw 'Fresh owned service survived native menu Quit'}
        Event 'native-owned-cleanup' 'native menu Quit stopped fresh S2 and all UI roles' @{service_pid=$S2.process.Id;generation=$S2.instance.id}
        $ScenarioStatus.fresh_owned_quit='PASS'
    }
    $Verdict='PASS'
}catch{$FailureText=$_.Exception.Message;Event 'failure' 'acceptance failed' @{message=$FailureText}}
finally{
    foreach($TaskProfile in @($TaskProfiles)){
        try{if($null -ne (Try-Role $TaskProfile 'tray')){$null=Role $TaskProfile 'tray' 'quit';Event 'cleanup-role-ipc' 'best-effort Quit after failure' @{profile=$TaskProfile}}}catch{$Cleanup='FAIL'}
    }
    foreach($serviceBinding in @($ServiceBindings)){
        if(-not $serviceBinding.process.HasExited){
            try{Stop-ObservedService $serviceBinding 'failure cleanup of exact driver-owned generation'}catch{
                # A retiring service may close its endpoint before releasing Engine.
                if(-not $serviceBinding.process.WaitForExit(15000)){$Cleanup='FAIL'}
            }
        }
    }
    if($null -ne $ExternalTarget -and -not $ExternalTarget.process.HasExited){
        try{$target=Get-Content -Raw -LiteralPath (Join-Path $TaskRoot 'target.json')|ConvertFrom-Json;$targetWindow=[IntPtr][long]$target.hwnd;if($target.pid -ne $ExternalTarget.process.Id -or [UnicNativeB973]::Owner($targetWindow) -ne $target.pid){throw 'Cleanup target identity mismatch'};[void][UnicNativeB973]::PostMessage($targetWindow,0x0010,[IntPtr]::Zero,[IntPtr]::Zero);if(-not $ExternalTarget.process.WaitForExit(5000)){throw 'Target still running'}}catch{$Cleanup='FAIL'}
    }
    foreach($bound in @($Handles)){if(-not $bound.process.WaitForExit(15000)){$Cleanup='FAIL'}}
    foreach($serviceProcess in @($Services)){if(-not $serviceProcess.WaitForExit(15000)){$Cleanup='FAIL'}}
    foreach($TaskProfile in @($TaskProfiles)){
        if(Test-Path -LiteralPath (Join-Path $TaskProfile 'unic.sock')){$Cleanup='FAIL'}
        foreach($roleName in @('gui','quick-panel','tray')){if($null -ne (Try-Role $TaskProfile $roleName)){$Cleanup='FAIL'}}
    }
    if($GlobalShortcutStatus -eq 'ATTEMPTED' -or $GlobalShortcutStatus -eq 'INJECTED_NOT_YET_OBSERVED'){$GlobalShortcutStatus='FAIL_OR_UNKNOWN'}
    if([UnicNativeB973]::MouseButtonsIdle()){[Windows.Forms.Cursor]::Position=$MousePosition}else{$Cleanup='FAIL';Event 'cleanup-warning' 'cursor not moved while a mouse button is held' $null}
    [void][UnicNativeB973]::SetThreadDpiAwarenessContext($PreviousDPIContext)
    if($Cleanup -eq 'PASS' -and $TaskRootCreated -and (Test-Path -LiteralPath $TaskRoot)){Remove-Item -Recurse -Force -LiteralPath $TaskRoot}
    if($Cleanup -ne 'PASS'){$Verdict='FAIL'}
    foreach($scenarioName in @($ScenarioStatus.Keys)){if($ScenarioStatus[$scenarioName] -eq 'RUNNING'){$ScenarioStatus[$scenarioName]='FAIL'}}
    Save-JSON 'receipt.json' @{verdict=$Verdict;requested_phase=$Phase;scenario_status=$ScenarioStatus;prior_panel_regression=@{run='native18';status='RED';observation='role.visible=true but owned Win32 panel HWND hidden';new_native_verification=$ScenarioStatus.normal_gui_absent_hotkey};cleanup=$Cleanup;failure=$FailureText;source_requested=$SourceSHA;native_menu_clicks=$NativeMenus;identities=@($Identities);events=@($Events);native_tray=@{method='native mouse and PID-bound menu';business_clicks=$NativeMenus};global_shortcut=@{status=$GlobalShortcutStatus;method='single SendInput chord; never replayed'};system_clipboard='NOT_RUN';cross_application_paste='NOT_RUN';task_root_removed=($TaskRootCreated -and -not (Test-Path -LiteralPath $TaskRoot))}
    $sums=@(Get-ChildItem -File -Recurse -LiteralPath $ArtifactDir|Where-Object{$_.Name -ne 'SHA256SUMS'}|Sort-Object FullName|ForEach-Object{(Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()+'  '+$_.FullName.Substring($ArtifactDir.TrimEnd('\').Length+1)})
    [IO.File]::WriteAllLines((Join-Path $ArtifactDir 'SHA256SUMS'),$sums,(New-Object Text.UTF8Encoding($false)))
}
if($Verdict -ne 'PASS' -and $Verdict -ne 'INSPECT_ONLY'){throw $FailureText}
