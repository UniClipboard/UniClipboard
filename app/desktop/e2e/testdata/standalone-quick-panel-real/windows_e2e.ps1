[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$BinDir,
    [Parameter(Mandatory=$true)][string]$OutDir,
    [Parameter(Mandatory=$true)][string]$WorkDir,
    [string]$GuestId = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'N03 requires native Windows interactive desktop'
}
if ([Threading.Thread]::CurrentThread.ApartmentState -ne 'STA') {
    throw 'Run with: powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File windows_e2e.ps1 ...'
}

Add-Type -AssemblyName System.Drawing, System.Windows.Forms

$NativeCode = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class UnicN03Win32 {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public UIntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public UIntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION data; }

    [DllImport("user32.dll", SetLastError=true)] static extern uint SendInput(uint count, INPUT[] values, int size);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint process);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int nCmdShow);

    public static int Owner(IntPtr hwnd) {
        if (hwnd == IntPtr.Zero) return 0;
        uint pid;
        GetWindowThreadProcessId(hwnd, out pid);
        return (int)pid;
    }

    public static void SendKey(ushort vk, bool ctrl, bool shift) {
        var list = new System.Collections.Generic.List<INPUT>();
        Action<ushort, uint> add = (k, flags) => {
            INPUT inp = new INPUT();
            inp.type = 1;
            inp.data.ki.wVk = k;
            inp.data.ki.dwFlags = flags;
            list.Add(inp);
        };
        if (ctrl) add(0x11, 0);
        if (shift) add(0x10, 0);
        add(vk, 0);
        add(vk, 2);
        if (shift) add(0x10, 2);
        if (ctrl) add(0x11, 2);
        INPUT[] arr = list.ToArray();
        uint sent = SendInput((uint)arr.Length, arr, Marshal.SizeOf(typeof(INPUT)));
        if (sent != (uint)arr.Length) {
            throw new Exception("SendInput sent " + sent + "/" + arr.Length);
        }
    }
}
'@
Add-Type -TypeDefinition $NativeCode

$BinDir = [IO.Path]::GetFullPath($BinDir)
$OutDir = [IO.Path]::GetFullPath($OutDir)
$WorkDir = [IO.Path]::GetFullPath($WorkDir)

if (Test-Path -LiteralPath $OutDir) { Remove-Item -LiteralPath $OutDir -Recurse -Force }
if (Test-Path -LiteralPath $WorkDir) { Remove-Item -LiteralPath $WorkDir -Recurse -Force }
[void][IO.Directory]::CreateDirectory($OutDir)
[void][IO.Directory]::CreateDirectory($WorkDir)
$DiscDir = Join-Path $WorkDir 'd'
[void][IO.Directory]::CreateDirectory($DiscDir)

$UnicExe = Join-Path $BinDir 'unic.exe'
$UnicdExe = Join-Path $BinDir 'unicd.exe'
$DesktopExe = Join-Path $BinDir 'unic-desktop.exe'
$HelperExe = Join-Path $BinDir 'windows-native-helper.exe'

foreach ($b in @($UnicExe, $UnicdExe, $DesktopExe, $HelperExe)) {
    if (-not (Test-Path -LiteralPath $b)) { throw "Missing required binary: $b" }
}

function Get-Sha256Hex([byte[]]$Bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($Bytes)) -replace '-', '').ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}
function Get-FileSha256([string]$Path) {
    return Get-Sha256Hex ([IO.File]::ReadAllBytes($Path))
}
function Make-TestPng() {
    $bmp = New-Object System.Drawing.Bitmap(64, 64, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        for ($y = 0; $y -lt 64; $y++) {
            for ($x = 0; $x -lt 64; $x++) {
                $bmp.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(255, (($x * 3 + 17) % 256), (($y * 5 + 31) % 256), ((($x -bxor $y) * 4) % 256)))
            }
        }
        $ms = New-Object IO.MemoryStream
        try {
            $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            return $ms.ToArray()
        } finally {
            $ms.Dispose()
        }
    } finally {
        $bmp.Dispose()
    }
}
function Quote-Arg([string]$Value) {
    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') { return $Value }
    return '"' + [regex]::Replace([regex]::Replace($Value, '(\\*)"', '$1$1\"'), '(\\+)$', '$1$1') + '"'
}

$Timeline = New-Object Collections.ArrayList
$SpawnedProcs = New-Object Collections.ArrayList
$Manifest = [ordered]@{
    case = 'N03-windows11-standalone-quick-panel'
    status = 'FAIL'
    guest_id = $GuestId
    started_at_utc = [DateTime]::UtcNow.ToString('o')
    environment = [ordered]@{
        os_version = [Environment]::OSVersion.VersionString
        is_64bit_os = [Environment]::Is64BitOperatingSystem
        arch = $env:PROCESSOR_ARCHITECTURE
        session_id = [Diagnostics.Process]::GetCurrentProcess().SessionId
    }
    binary_sha256 = [ordered]@{}
    checks = New-Object Collections.ArrayList
    process_snapshots = [ordered]@{}
    clipboard_readbacks = [ordered]@{}
    paste_receipts = [ordered]@{}
    timeline = $Timeline
}
foreach ($f in (Get-ChildItem -LiteralPath $BinDir -File | Sort-Object Name)) {
    $Manifest.binary_sha256[$f.Name] = Get-FileSha256 $f.FullName
}

function Log-Step([string]$Step, $Detail = $null) {
    $entry = [ordered]@{ at = [Math]::Round(([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000.0), 3); step = $Step }
    if ($null -ne $Detail) { $entry.detail = $Detail }
    [void]$Timeline.Add($entry)
    $dStr = if ($null -ne $Detail) { $Detail | ConvertTo-Json -Compress -Depth 8 } else { '' }
    Write-Host "[N03] ${Step}: $dStr"
}

function Start-ProfileProc([string]$Program, [string[]]$ArgList, [string]$ProfileDir, [string]$LogFile = '') {
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $Program
    $psi.Arguments = ($ArgList | ForEach-Object { Quote-Arg $_ }) -join ' '
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.StandardOutputEncoding = New-Object Text.UTF8Encoding($false)
    $psi.StandardErrorEncoding = New-Object Text.UTF8Encoding($false)
    foreach ($k in @($psi.EnvironmentVariables.Keys)) {
        if ($k.StartsWith('UNIC_') -or $k.StartsWith('MYGO_')) { $psi.EnvironmentVariables.Remove($k) }
    }
    if ($ProfileDir) {
        $psi.EnvironmentVariables['UNIC_HOME'] = $ProfileDir
        $psi.EnvironmentVariables['UNIC_SOCK'] = Join-Path $ProfileDir 'unic.sock'
        $psi.EnvironmentVariables['UNIC_RENDEZVOUS_URL'] = 'http://127.0.0.1:1'
        $psi.EnvironmentVariables['HOME'] = $DiscDir
        $psi.EnvironmentVariables['USERPROFILE'] = $DiscDir
        $psi.EnvironmentVariables['LOCALAPPDATA'] = Join-Path $DiscDir 'local'
        $psi.EnvironmentVariables['APPDATA'] = Join-Path $DiscDir 'roaming'
        $psi.EnvironmentVariables['XDG_CONFIG_HOME'] = Join-Path $DiscDir 'config'
        $psi.EnvironmentVariables['XDG_DATA_HOME'] = Join-Path $DiscDir 'data'
    }
    $p = New-Object Diagnostics.Process
    $p.StartInfo = $psi
    if (-not $p.Start()) { throw "Failed to start $Program" }
    $outTask = $p.StandardOutput.ReadToEndAsync()
    $errTask = $p.StandardError.ReadToEndAsync()
    $rec = [pscustomobject]@{ proc = $p; out = $outTask; err = $errTask; log = $LogFile; program = $Program }
    [void]$SpawnedProcs.Add($rec)
    return $rec
}

function Invoke-ProfileCmd([string]$Program, [string[]]$ArgList, [string]$ProfileDir, [int]$TimeoutMs = 30000) {
    $rec = Start-ProfileProc $Program $ArgList $ProfileDir
    if (-not $rec.proc.WaitForExit($TimeoutMs)) {
        try { $rec.proc.Kill() } catch {}
        throw "Timed out waiting for $Program $($ArgList -join ' ')"
    }
    $stdout = $rec.out.Result
    $stderr = $rec.err.Result
    if ($rec.proc.ExitCode -ne 0) {
        throw "$Program exited $($rec.proc.ExitCode): $($stderr.Trim()) | $($stdout.Trim())"
    }
    return $stdout
}

function As-JsonArray($Items) {
    $list = New-Object Collections.ArrayList
    if ($null -ne $Items) {
        foreach ($x in @($Items)) { [void]$list.Add($x) }
    }
    return ,$list
}

function Api-Req([string]$ProfileDir, [string]$Method, [string]$Path, $BodyObj = $null) {
    $bodyJson = if ($null -eq $BodyObj) { 'null' } else { ConvertTo-Json -InputObject $BodyObj -Compress -Depth 12 }
    $sock = Join-Path $ProfileDir 'unic.sock'
    $raw = Invoke-ProfileCmd $HelperExe @('api', '--endpoint', $sock, '--method', $Method, '--path', $Path, '--params', $bodyJson) $ProfileDir 20000
    if ([string]::IsNullOrWhiteSpace($raw)) { return $null }
    return $raw | ConvertFrom-Json
}

function Role-Cmd([string]$ProfileDir, [string]$Role, [string]$Command) {
    $raw = Invoke-ProfileCmd $DesktopExe @('--role', $Role, '--command', $Command) $ProfileDir 20000
    return $raw | ConvertFrom-Json
}

function Role-Probe([string]$ProfileDir, [string]$Role) {
    try { return Role-Cmd $ProfileDir $Role 'probe' } catch { return $null }
}

function Wait-Until([string]$Desc, [scriptblock]$Cond, [int]$Seconds = 20) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    $lastErr = $null
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $v = & $Cond
            if ($v) { return $v }
        } catch {
            $lastErr = $_.Exception.Message
        }
        Start-Sleep -Milliseconds 100
    }
    throw "Timed out waiting for ${Desc} (last error: ${lastErr})"
}

function Get-TaskUnicProcs() {
    $rows = New-Object Collections.ArrayList
    $wmi = Get-CimInstance Win32_Process -Filter "Name='unicd.exe' OR Name='unic-desktop.exe'"
    foreach ($p in $wmi) {
        if ($p.ExecutablePath -and $p.ExecutablePath.StartsWith($BinDir, [StringComparison]::OrdinalIgnoreCase)) {
            [void]$rows.Add([ordered]@{ pid = [int]$p.ProcessId; name = $p.Name; cmd = $p.CommandLine })
        }
    }
    return $rows
}

function Init-Profile([string]$ShortName, [bool]$SyncEnabled) {
    $p = Join-Path $WorkDir $ShortName
    [void][IO.Directory]::CreateDirectory($p)
    [IO.File]::WriteAllText((Join-Path $p 'desktop-setup.json'), "{`"complete`":true}`n", (New-Object Text.UTF8Encoding($false)))
    $syncStr = if ($SyncEnabled) { 'true' } else { 'false' }
    $cfg = "[telemetry]`nenabled = false`n[sync]`nenabled = $syncStr`n[desktop]`nmode = 'normal'`nquick_panel_shortcut = ''`n"
    [IO.File]::WriteAllText((Join-Path $p 'config.toml'), $cfg, (New-Object Text.UTF8Encoding($false)))
    $res = Invoke-ProfileCmd $UnicExe @('migrate', '--initialize-empty', '--json') $p 25000
    Log-Step "init_profile:$ShortName" ($res | ConvertFrom-Json)
    return $p
}

# Build external WinForms target script
$TargetScript = Join-Path $WorkDir 'target_editor.ps1'
$TargetState = Join-Path $WorkDir 'target_state.json'
$TargetCmd = Join-Path $WorkDir 'target_cmd.txt'
$TargetScriptBody = @'
param(
    [Parameter(Mandatory=$true)][string]$StatePath,
    [Parameter(Mandatory=$true)][string]$CmdPath
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing

$form = New-Object Windows.Forms.Form
$form.Text = 'UnicN03ExternalTarget'
$form.StartPosition = 'Manual'
$form.Left = 140
$form.Top = 100
$form.Width = 560
$form.Height = 320
$form.WindowState = [Windows.Forms.FormWindowState]::Maximized

$tb = New-Object Windows.Forms.TextBox
$tb.Multiline = $true
$tb.Dock = 'Fill'
$tb.Font = New-Object Drawing.Font('Segoe UI', 12)
$form.Controls.Add($tb)

function Write-State {
    $obj = [ordered]@{
        pid = [Diagnostics.Process]::GetCurrentProcess().Id
        hwnd = $form.Handle.ToInt64()
        text = $tb.Text
        window_state = $form.WindowState.ToString()
        width = $form.Width
        height = $form.Height
        left = $form.Left
        top = $form.Top
        focused = $form.ContainsFocus
    }
    $tmp = $StatePath + '.tmp'
    [IO.File]::WriteAllText($tmp, ($obj | ConvertTo-Json -Compress), (New-Object Text.UTF8Encoding($false)))
    try { [IO.File]::Copy($tmp, $StatePath, $true) } catch {}
}

$tb.Add_TextChanged({ Write-State })
$form.Add_Shown({
    $tb.Focus() | Out-Null
    $form.Activate() | Out-Null
    Write-State
})
$timer = New-Object Windows.Forms.Timer
$timer.Interval = 80
$timer.Add_Tick({
    if (Test-Path -LiteralPath $CmdPath) {
        $cmd = ([IO.File]::ReadAllText($CmdPath)).Trim()
        Remove-Item -LiteralPath $CmdPath -Force -ErrorAction SilentlyContinue
        if ($cmd -eq 'clear') {
            $tb.Text = ''
            $tb.SelectionStart = 0
            $tb.Focus() | Out-Null
        } elseif ($cmd -eq 'normal') {
            $form.WindowState = [Windows.Forms.FormWindowState]::Normal
            $form.Left = 160
            $form.Top = 120
            $form.Width = 540
            $form.Height = 320
            $tb.Focus() | Out-Null
            $form.Activate() | Out-Null
        } elseif ($cmd -eq 'close') {
            $form.Close()
        }
    }
    Write-State
})
$timer.Start()
[void]$form.ShowDialog()
'@
[IO.File]::WriteAllText($TargetScript, $TargetScriptBody, (New-Object Text.UTF8Encoding($false)))

# Build independent STA clipboard helper script (avoids any in-process clipboard caching)
$ClipHelperScript = Join-Path $WorkDir 'clip_helper.ps1'
$ClipHelperBody = @'
param(
    [Parameter(Mandatory=$true)][ValidateSet('read-text','read-png-b64','read-files','set-text')][string]$Op,
    [string]$Value = ''
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
if ($Op -eq 'read-text') {
    $t = [Windows.Forms.Clipboard]::GetText([Windows.Forms.TextDataFormat]::UnicodeText)
    Write-Output ([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($t)))
} elseif ($Op -eq 'read-png-b64') {
    $data = [Windows.Forms.Clipboard]::GetDataObject()
    if ($null -eq $data) { throw 'Clipboard DataObject is null' }
    $bytes = $null
    if ($data.GetDataPresent('PNG')) {
        $obj = $data.GetData('PNG')
        if ($obj -is [IO.MemoryStream]) { $bytes = $obj.ToArray() }
        elseif ($obj -is [byte[]]) { $bytes = $obj }
    }
    if ($null -eq $bytes -and [Windows.Forms.Clipboard]::ContainsImage()) {
        $img = [Windows.Forms.Clipboard]::GetImage()
        $ms = New-Object IO.MemoryStream
        try {
            $img.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            $bytes = $ms.ToArray()
        } finally {
            $ms.Dispose()
            $img.Dispose()
        }
    }
    if ($null -eq $bytes) { throw 'Clipboard has no PNG/Image data' }
    Write-Output ([Convert]::ToBase64String($bytes))
} elseif ($Op -eq 'read-files') {
    $files = [Windows.Forms.Clipboard]::GetFileDropList()
    $arr = @()
    foreach ($f in $files) { $arr += $f }
    Write-Output (ConvertTo-Json -InputObject $arr -Compress)
} elseif ($Op -eq 'set-text') {
    [Windows.Forms.Clipboard]::SetText($Value, [Windows.Forms.TextDataFormat]::UnicodeText)
    Write-Output 'OK'
}
'@
[IO.File]::WriteAllText($ClipHelperScript, $ClipHelperBody, (New-Object Text.UTF8Encoding($false)))

function Read-TargetState() {
    for ($i = 0; $i -lt 20; $i++) {
        try {
            $fs = New-Object IO.FileStream($TargetState, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
            try {
                $sr = New-Object IO.StreamReader($fs, [Text.Encoding]::UTF8)
                $raw = $sr.ReadToEnd()
                $sr.Dispose()
            } finally {
                $fs.Dispose()
            }
            if (-not [string]::IsNullOrWhiteSpace($raw)) {
                return ($raw | ConvertFrom-Json)
            }
        } catch {}
        Start-Sleep -Milliseconds 40
    }
    throw "Failed to read $TargetState after retries"
}

function Run-ClipHelper([string]$Op, [string]$Value = '') {
    $argsList = @('-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass', '-File', $ClipHelperScript, '-Op', $Op)
    if ($Value -ne '') { $argsList += @('-Value', $Value) }
    $out = Invoke-ProfileCmd 'powershell.exe' $argsList '' 15000
    if ($Op -eq 'read-text') {
        return [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($out.Trim()))
    }
    return $out
}

try {
    # 1. Init main + peer profiles & receive real managed cached file
    $MainProf = Init-Profile 'm' $true
    $PeerProf = Init-Profile 'p' $true

    $MainDaemon = Start-ProfileProc $UnicdExe @() $MainProf 'main-unicd.log'
    $MainSvc = Wait-Until 'main unicd ready' { Api-Req $MainProf 'GET' '/v1/service' } 25
    Log-Step 'main_unicd_started' $MainSvc

    $PeerDaemon = Start-ProfileProc $UnicdExe @() $PeerProf 'peer-unicd.log'
    $PeerSvc = Wait-Until 'peer unicd ready' { Api-Req $PeerProf 'GET' '/v1/service' } 25
    Log-Step 'peer_unicd_started' $PeerSvc

    $Ticket = (Api-Req $PeerProf 'POST' '/v1/pairing-ticket' @{}).ticket
    [void](Api-Req $MainProf 'POST' '/v1/peers' @{ addr = $Ticket })
    $ApproveAll = {
        param([string]$Prof)
        $pend = Api-Req $Prof 'GET' '/v1/pending'
        if ($pend) {
            foreach ($req in @($pend)) {
                $binds = As-JsonArray $req.bindings
                [void](Api-Req $Prof 'POST' "/v1/pending/$($req.id)/approve" @{
                    fingerprint = $req.fingerprint
                    revision = $req.revision
                    bindings = $binds
                })
            }
        }
        return $true
    }
    Wait-Until 'peers connected' {
        [void](& $ApproveAll $PeerProf)
        [void](& $ApproveAll $MainProf)
        $st = Api-Req $MainProf 'GET' '/v1/status'
        if ($st -and $st.peers) {
            foreach ($peer in @($st.peers)) {
                if ($peer.connected) { return $true }
            }
        }
        return $false
    } 30
    Log-Step 'paired_main_and_peer' @{ connected = $true }

    $ManagedPayloadBytes = [Text.Encoding]::UTF8.GetBytes("N03-windows11-managed-file-payload-$([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())`n")
    $ManagedSha = Get-Sha256Hex $ManagedPayloadBytes
    $SrcFile = Join-Path $WorkDir 'win11-managed-test.txt'
    [IO.File]::WriteAllBytes($SrcFile, $ManagedPayloadBytes)
    [void](Api-Req $PeerProf 'POST' '/v1/submissions' @{
        request_id = 'n03-peer-file'
        snapshot = @{
            files = (As-JsonArray @(@{ name = 'win11-managed-test.txt'; path = $SrcFile; size = $ManagedPayloadBytes.Length }))
        }
    })

    $ManagedItem = Wait-Until 'main received managed file' {
        $h = Api-Req $MainProf 'GET' '/v1/history'
        if ($h -and $h.items) {
            foreach ($it in @($h.items)) {
                if ($it.origin -eq 'remote_device' -and ($it.kind -eq 'file' -or $it.kind -eq 'files')) { return $it }
            }
        }
        return $null
    } 30
    Log-Step 'managed_file_received' @{ id = $ManagedItem.id; revision = $ManagedItem.revision; sha256 = $ManagedSha }

    # Stop peer and restart main unicd so receive-list TTL ref is cleared
    try { [void](Api-Req $PeerProf 'POST' '/v1/service/stop' @{ id = $PeerSvc.id }) } catch {}
    $PeerDaemon.proc.WaitForExit(10000) | Out-Null
    [void](Api-Req $MainProf 'POST' '/v1/service/stop' @{ id = $MainSvc.id })
    $MainDaemon.proc.WaitForExit(10000) | Out-Null

    $MainDaemon = Start-ProfileProc $UnicdExe @() $MainProf 'main-unicd-2.log'
    $MainSvc = Wait-Until 'main unicd restarted' { Api-Req $MainProf 'GET' '/v1/service' } 25
    Log-Step 'main_unicd_restarted' $MainSvc

    # Seed PNG (row 1) and UTF-8 Chinese text (row 0)
    $PngBytes = Make-TestPng
    $PngSha = Get-Sha256Hex $PngBytes
    $PngB64 = [Convert]::ToBase64String($PngBytes)
    [void](Api-Req $MainProf 'POST' '/v1/submissions' @{
        request_id = 'n03-png'
        snapshot = @{
            parts = (As-JsonArray @(@{ format = 'image/png'; data = $PngB64 }))
        }
    })
    Start-Sleep -Milliseconds 150

    $TextPayload = "Windows 11 " + ([char]0x5FEB) + ([char]0x901F) + ([char]0x9762) + ([char]0x677F) + ([char]0x81EA) + ([char]0x52A8) + ([char]0x7C98) + ([char]0x8D34) + ([char]0x9A8C) + ([char]0x8BC1) + " #N03"
    $TextUtf8 = [Text.Encoding]::UTF8.GetBytes($TextPayload)
    $TextSha = Get-Sha256Hex $TextUtf8
    [void](Api-Req $MainProf 'POST' '/v1/submissions' @{
        request_id = 'n03-text'
        snapshot = @{
            parts = (As-JsonArray @(@{ format = 'text/plain'; data = [Convert]::ToBase64String($TextUtf8) }))
        }
    })

    Wait-Until '3 history items seeded' {
        $h = Api-Req $MainProf 'GET' '/v1/history'
        return ($h.items.Count -ge 3)
    } 15
    Log-Step 'seeded_history' @{ text = $TextPayload; text_sha = $TextSha; png_sha = $PngSha; file_sha = $ManagedSha }

    # 2. Launch external MAXIMIZED WinForms TextBox target window
    $TargetProc = Start-ProfileProc 'powershell.exe' @('-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass', '-File', $TargetScript, '-StatePath', $TargetState, '-CmdPath', $TargetCmd) '' 'target-editor.log'
    $TState = Wait-Until 'external WinForms target shown and foreground' {
        if (-not (Test-Path -LiteralPath $TargetState)) { return $null }
        $s = Read-TargetState
        $hwnd = [IntPtr][int64]$s.hwnd
        [void][UnicN03Win32]::SetForegroundWindow($hwnd)
        if ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -eq [int]$s.pid) { return $s }
        return $null
    } 20
    $TargetHwnd = [IntPtr][int64]$TState.hwnd
    $TargetPid = [int]$TState.pid
    if (-not [UnicN03Win32]::IsZoomed($TargetHwnd)) { throw 'Expected external target to start maximized' }
    Log-Step 'external_target_ready' $TState

    # 3. Cold-start production unic-desktop.exe --quick-panel -> verify AllowForeground + standalone processes
    [void](Invoke-ProfileCmd $DesktopExe @('--quick-panel') $MainProf 20000)
    $PanelRole = Wait-Until 'quick-panel role ready' { Role-Probe $MainProf 'quick-panel' } 15
    $PanelPid = [int]$PanelRole.pid
    Wait-Until 'quick-panel acquired foreground on cold start' {
        return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -eq $PanelPid)
    } 10

    $ProcsStandalone = Get-TaskUnicProcs
    $Manifest.process_snapshots['win11_standalone_panel'] = $ProcsStandalone
    if ($null -ne (Role-Probe $MainProf 'tray')) { throw 'Unexpected tray role in standalone mode' }
    if ($null -ne (Role-Probe $MainProf 'gui')) { throw 'Unexpected gui role in standalone mode' }
    [void]$Manifest.checks.Add('win11-cold-start-standalone-panel-and-allow-foreground')
    Log-Step 'cold_start_panel_foreground' @{ panel_pid = $PanelPid; procs = $ProcsStandalone.Count }

    # Repeat --quick-panel keeps visible & foreground, same PID
    [void](Invoke-ProfileCmd $DesktopExe @('--quick-panel') $MainProf 20000)
    Start-Sleep -Milliseconds 300
    if ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -ne $PanelPid) {
        throw 'Repeat --quick-panel lost foreground'
    }
    if ((Role-Probe $MainProf 'quick-panel').pid -ne $PanelPid) {
        throw 'Repeat --quick-panel spawned duplicate PID'
    }
    [void]$Manifest.checks.Add('win11-repeat-quick-panel-keeps-foreground-same-pid')
    Log-Step 'repeat_quick_panel_ok' @{ panel_pid = $PanelPid }

    # Physical Return -> cross-app single-paste into MAXIMIZED target without unmaximizing!
    Start-Sleep -Milliseconds 350
    [UnicN03Win32]::SendKey(0x0D, $false, $false)
    Wait-Until 'external WinForms target received single paste' {
        $s = Read-TargetState
        return ($s.text -eq $TextPayload)
    } 12
    Start-Sleep -Milliseconds 900
    $AfterPaste1 = Read-TargetState
    if ($AfterPaste1.text -ne $TextPayload) { throw "Paste mismatch or duplicate: $($AfterPaste1.text)" }
    if (-not [UnicN03Win32]::IsZoomed($TargetHwnd)) { throw 'Non-minimized maximized target was unmaximized by paste!' }
    $ClipText1 = Run-ClipHelper 'read-text'
    if ((Get-Sha256Hex ([Text.Encoding]::UTF8.GetBytes($ClipText1))) -ne $TextSha) { throw 'OLE text readback mismatch' }
    $Manifest.paste_receipts['maximized_target_single_paste'] = [ordered]@{
        target_pid = $TargetPid
        target_hwnd = $TState.hwnd
        still_maximized = [UnicN03Win32]::IsZoomed($TargetHwnd)
        inserted_text = $AfterPaste1.text
        expected_sha256 = $TextSha
        actual_sha256 = Get-Sha256Hex ([Text.Encoding]::UTF8.GetBytes($AfterPaste1.text))
        single_insertion_verified = $true
    }
    $Manifest.clipboard_readbacks['text_after_paste_dismiss'] = [ordered]@{
        expected_sha256 = $TextSha
        actual_sha256 = Get-Sha256Hex ([Text.Encoding]::UTF8.GetBytes($ClipText1))
        match = $true
    }
    [void]$Manifest.checks.Add('win11-enter-cross-app-single-paste-and-maximized-geometry-preserved')
    Log-Step 'maximized_target_paste_ok' $Manifest.paste_receipts['maximized_target_single_paste']

    # 4. Standby hot re-entry + minimized target restore!
    [IO.File]::WriteAllText($TargetCmd, 'clear', (New-Object Text.UTF8Encoding($false)))
    Wait-Until 'target cleared' {
        $s = Read-TargetState
        return ($s.text -eq '')
    } 8
    [void][UnicN03Win32]::SetForegroundWindow($TargetHwnd)
    Wait-Until 'target foreground before hot re-entry' {
        return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -eq $TargetPid)
    } 8

    [void](Invoke-ProfileCmd $DesktopExe @('--quick-panel') $MainProf 20000)
    Wait-Until 'quick-panel foreground on standby hot re-entry' {
        return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -eq $PanelPid)
    } 10
    if ((Role-Probe $MainProf 'quick-panel').pid -ne $PanelPid) { throw 'Standby hot re-entry changed panel PID' }
    [void]$Manifest.checks.Add('win11-standby-hot-reentry-reuses-pid-and-allow-foreground')

    # Minimize the captured target while panel stays open & foreground -> Return must SW_RESTORE and paste!
    [void][UnicN03Win32]::ShowWindow($TargetHwnd, 6) # SW_MINIMIZE
    Wait-Until 'target minimized' { return [UnicN03Win32]::IsIconic($TargetHwnd) } 8
    # Keep panel foreground
    Start-Sleep -Milliseconds 300
    [UnicN03Win32]::SendKey(0x0D, $false, $false)
    Wait-Until 'minimized target restored and pasted' {
        if ([UnicN03Win32]::IsIconic($TargetHwnd)) { return $false }
        $s = Read-TargetState
        return ($s.text -eq $TextPayload)
    } 12
    Start-Sleep -Milliseconds 700
    $AfterPaste2 = Read-TargetState
    if ($AfterPaste2.text -ne $TextPayload) { throw "Restored target paste mismatch: $($AfterPaste2.text)" }
    $Manifest.paste_receipts['minimized_target_restore_and_paste'] = [ordered]@{
        target_hwnd = $TState.hwnd
        restored_from_iconic = (-not [UnicN03Win32]::IsIconic($TargetHwnd))
        inserted_text = $AfterPaste2.text
        match = $true
    }
    [void]$Manifest.checks.Add('win11-minimized-target-restored-and-pasted')
    Log-Step 'minimized_target_restore_paste_ok' $Manifest.paste_receipts['minimized_target_restore_and_paste']

    # 5. Windowless PNG copy (Down -> Ctrl+C) and independent OLE clipboard readback after dismiss
    [void](Invoke-ProfileCmd $DesktopExe @('--quick-panel') $MainProf 20000)
    Wait-Until 'panel foreground for png' { return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -eq $PanelPid) } 10
    Start-Sleep -Milliseconds 350
    [UnicN03Win32]::SendKey(0x28, $false, $false) # VK_DOWN
    Start-Sleep -Milliseconds 200
    [UnicN03Win32]::SendKey(0x43, $true, $false) # Ctrl+C
    Wait-Until 'panel dismissed after png copy' { return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -ne $PanelPid) } 10
    Start-Sleep -Milliseconds 250
    $ReadPngB64 = (Run-ClipHelper 'read-png-b64').Trim()
    $ReadPngBytes = [Convert]::FromBase64String($ReadPngB64)
    $ReadPngSha = Get-Sha256Hex $ReadPngBytes
    if ($ReadPngSha -ne $PngSha) { throw "OLE PNG SHA mismatch: expected $PngSha got $ReadPngSha (len=$($ReadPngBytes.Length))" }
    $Manifest.clipboard_readbacks['png_after_dismiss'] = [ordered]@{
        expected_sha256 = $PngSha
        actual_sha256 = $ReadPngSha
        bytes = $ReadPngBytes.Length
        match = $true
    }
    [void]$Manifest.checks.Add('win11-windowless-ole-png-copy-and-readback')
    Log-Step 'png_ole_readback_ok' $Manifest.clipboard_readbacks['png_after_dismiss']

    # 6. Windowless managed file copy (Down x2 -> Ctrl+C) -> F13 delete while held -> external OLE replacement reclaims!
    [void](Invoke-ProfileCmd $DesktopExe @('--quick-panel') $MainProf 20000)
    Wait-Until 'panel foreground for file' { return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -eq $PanelPid) } 10
    Start-Sleep -Milliseconds 350
    [UnicN03Win32]::SendKey(0x28, $false, $false)
    Start-Sleep -Milliseconds 150
    [UnicN03Win32]::SendKey(0x28, $false, $false)
    Start-Sleep -Milliseconds 200
    [UnicN03Win32]::SendKey(0x43, $true, $false)
    Wait-Until 'panel dismissed after file copy' { return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -ne $PanelPid) } 10
    Start-Sleep -Milliseconds 250
    $DropFiles = (Run-ClipHelper 'read-files') | ConvertFrom-Json
    if ($DropFiles -is [string]) { $DropFiles = @($DropFiles) }
    if ($DropFiles.Count -ne 1) { throw "Expected 1 OLE FileDropList entry, got $($DropFiles.Count)" }
    $ManagedPath = [string]$DropFiles[0]
    if (-not (Test-Path -LiteralPath $ManagedPath)) { throw "Managed file missing: $ManagedPath" }
    if (-not $ManagedPath.StartsWith((Join-Path $MainProf 'inbox'), [StringComparison]::OrdinalIgnoreCase)) {
        throw "Managed file not inside profile inbox: $ManagedPath"
    }
    $ActualFileSha = Get-FileSha256 $ManagedPath
    if ($ActualFileSha -ne $ManagedSha) { throw "Managed file SHA mismatch: expected $ManagedSha got $ActualFileSha" }
    $Manifest.clipboard_readbacks['managed_file_after_dismiss'] = [ordered]@{
        path = $ManagedPath
        expected_sha256 = $ManagedSha
        actual_sha256 = $ActualFileSha
        match = $true
    }
    [void]$Manifest.checks.Add('win11-windowless-ole-managed-file-copy-and-readback')
    Log-Step 'managed_file_ole_readback_ok' $Manifest.clipboard_readbacks['managed_file_after_dismiss']

    # Delete history item while held on OLE clipboard -> file stays pinned
    [void](Api-Req $MainProf 'DELETE' "/v1/history/$($ManagedItem.id)?revision=$($ManagedItem.revision)")
    Start-Sleep -Milliseconds 600
    if (-not (Test-Path -LiteralPath $ManagedPath)) { throw 'Managed file was deleted prematurely while held on OLE clipboard' }

    # Independent external OLE clipboard replacement -> triggers WM_CLIPBOARDUPDATE -> release -> disk file reclaimed!
    [void](Run-ClipHelper 'set-text' 'N03-WIN11-EXTERNAL-REPLACEMENT')
    Wait-Until 'managed file reclaimed after external OLE replacement' { -not (Test-Path -LiteralPath $ManagedPath) } 15
    $CacheAfter = Api-Req $MainProf 'GET' '/v1/file-cache'
    $Manifest.clipboard_readbacks['external_replacement_and_reclaim'] = [ordered]@{
        external_clip = (Run-ClipHelper 'read-text')
        managed_file_removed = (-not (Test-Path -LiteralPath $ManagedPath))
        cache_state_after_release = $CacheAfter
    }
    [void]$Manifest.checks.Add('win11-external-ole-replacement-releases-lease-and-reclaims-cache')
    Log-Step 'managed_file_reclaimed_ok' $Manifest.clipboard_readbacks['external_replacement_and_reclaim']

    # 7. Tray coexistence (zero echo) + Tray exit preserves standalone unicd & quick-panel
    $TrayProc = Start-ProfileProc $DesktopExe @('--role', 'tray') $MainProf 'main-tray.log'
    Wait-Until 'tray role ready' { Role-Probe $MainProf 'tray' } 20
    $Manifest.process_snapshots['win11_with_tray_coexistence'] = Get-TaskUnicProcs
    $HistBefore = (Api-Req $MainProf 'GET' '/v1/history').items.Count

    [void](Invoke-ProfileCmd $DesktopExe @('--quick-panel') $MainProf 20000)
    Wait-Until 'panel foreground with tray' { return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -eq $PanelPid) } 10
    Start-Sleep -Milliseconds 350
    [UnicN03Win32]::SendKey(0x43, $true, $false) # Ctrl+C text
    Wait-Until 'panel dismissed with tray' { return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -ne $PanelPid) } 10
    Start-Sleep -Milliseconds 1500
    $HistAfter = (Api-Req $MainProf 'GET' '/v1/history').items.Count
    if ($HistAfter -ne $HistBefore) { throw "Tray coexistence caused clipboard echo: $HistBefore -> $HistAfter" }
    [void]$Manifest.checks.Add('win11-tray-coexistence-zero-echo')
    Log-Step 'tray_coexistence_zero_echo' @{ history_count = $HistAfter }

    # Standalone tray process exit (distinct from full-app `quit` command) preserves standalone unicd & quick-panel
    try { $TrayProc.proc.Kill() } catch {}
    $TrayProc.proc.WaitForExit(10000) | Out-Null
    Wait-Until 'tray exited' { $null -eq (Role-Probe $MainProf 'tray') } 15
    Start-Sleep -Milliseconds 500
    $SvcAfter = Api-Req $MainProf 'GET' '/v1/service'
    $PanelAfter = Role-Probe $MainProf 'quick-panel'
    if ($SvcAfter.id -ne $MainSvc.id -or $SvcAfter.pid -ne $MainSvc.pid) { throw 'Tray exit disrupted standalone unicd' }
    if ($null -eq $PanelAfter -or $PanelAfter.pid -ne $PanelPid) { throw 'Tray exit disrupted standalone quick-panel' }
    $Manifest.process_snapshots['win11_after_tray_exit'] = Get-TaskUnicProcs

    [void](Invoke-ProfileCmd $DesktopExe @('--quick-panel') $MainProf 20000)
    Wait-Until 'panel foreground after tray exit' { return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -eq $PanelPid) } 10
    Start-Sleep -Milliseconds 350
    [UnicN03Win32]::SendKey(0x28, $false, $false)
    Start-Sleep -Milliseconds 200
    [UnicN03Win32]::SendKey(0x43, $true, $false)
    Wait-Until 'panel dismissed after post-tray png copy' { return ([UnicN03Win32]::Owner([UnicN03Win32]::GetForegroundWindow()) -ne $PanelPid) } 10
    Start-Sleep -Milliseconds 250
    $PostTrayPngSha = Get-Sha256Hex ([Convert]::FromBase64String((Run-ClipHelper 'read-png-b64').Trim()))
    if ($PostTrayPngSha -ne $PngSha) { throw 'Post-tray PNG OLE copy mismatch' }
    [void]$Manifest.checks.Add('win11-tray-exit-preserves-standalone-unicd-and-panel-copy')
    Log-Step 'post_tray_exit_copy_ok' @{ png_sha256 = $PostTrayPngSha }

    $Manifest.status = 'PASS'
    $Manifest.finished_at_utc = [DateTime]::UtcNow.ToString('o')
} catch {
    $Manifest.error = $_.Exception.ToString()
    Log-Step 'FAILED' $_.Exception.Message
    throw
} finally {
    try { [IO.File]::WriteAllText($TargetCmd, 'close', (New-Object Text.UTF8Encoding($false))) } catch {}
    Start-Sleep -Milliseconds 300
    foreach ($rec in $SpawnedProcs) {
        try {
            if (-not $rec.proc.HasExited) { $rec.proc.Kill() }
        } catch {}
        if ($rec.log) {
            try {
                $outText = ''
                if ($rec.out.IsCompleted) { $outText += $rec.out.Result }
                if ($rec.err.IsCompleted) { $outText += "`n--- STDERR ---`n" + $rec.err.Result }
                [IO.File]::WriteAllText((Join-Path $OutDir $rec.log), $outText, (New-Object Text.UTF8Encoding($false)))
            } catch {}
        }
    }
    # Ensure no stray unicd/unic-desktop from this BinDir survives
    foreach ($p in (Get-TaskUnicProcs)) {
        try { Stop-Process -Id $p.pid -Force -ErrorAction SilentlyContinue } catch {}
    }
    $mPath = Join-Path $OutDir 'manifest.json'
    [IO.File]::WriteAllText($mPath, ($Manifest | ConvertTo-Json -Depth 20), (New-Object Text.UTF8Encoding($false)))
    $sumLines = New-Object Collections.ArrayList
    foreach ($f in (Get-ChildItem -LiteralPath $OutDir -File | Sort-Object Name)) {
        if ($f.Name -eq 'SHA256SUMS') { continue }
        [void]$sumLines.Add("$(Get-FileSha256 $f.FullName)  $($f.Name)")
    }
    [IO.File]::WriteAllText((Join-Path $OutDir 'SHA256SUMS'), ($sumLines -join "`n") + "`n", (New-Object Text.UTF8Encoding($false)))
}
