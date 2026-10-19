# Detailed capture and network-log acceptance through the native Windows GUI (the Windows counterpart of
# diagnostics_acceptance_run.py). Two portable, isolated peers pair through the product flow: peer A owns the GUI
# profile, peer B is a CLI profile in its own directory. Three GUI launches run the in-WebView scenario
# (frontend/src/e2e/diagnostics-acceptance.ts): capture (leaves the daemon running), reopen (quits fully), offline.
#
# Usage: windows_diagnostics_acceptance.ps1 -Binaries <dir with gui-go.exe, uniclip.exe, uniclipd.exe> -Work <dir> [-NoExpiryWait]
# Results and artifacts are written under <Work>\out. Exports are verified afterwards by the Python verifier.
param(
  [Parameter(Mandatory)][string]$Binaries,
  [Parameter(Mandatory)][string]$Work,
  [switch]$NoExpiryWait
)
$ErrorActionPreference = 'Stop'
$Passphrase = 'hunter22hunter22'
$root = Join-Path $Work 'acc'
$out = Join-Path $root 'out'
if (Test-Path $root) { Remove-Item $root -Recurse -Force }
New-Item -ItemType Directory -Force $out | Out-Null
foreach ($p in 'uc-gui-go-a', 'uc-gui-go-b') {
  New-Item -ItemType Directory -Force (Join-Path $root $p) | Out-Null
  foreach ($f in 'gui-go.exe', 'uniclip.exe', 'uniclipd.exe') {
    if ($p -eq 'uc-gui-go-b' -and $f -eq 'gui-go.exe') { continue }
    Copy-Item (Join-Path $Binaries $f) (Join-Path $root $p) -Force
  }
}
$gui = Join-Path $root 'uc-gui-go-a'
$peer = Join-Path $root 'uc-gui-go-b'
$results = @{}
$seenAll = @{} # step details stay in memory only; the raw evidence files hold them on disk
# `R` is a built-in alias (Invoke-History) and would win over a function, so the recorder is named Rec.
# Every result is appended as one small JSON line the moment it is known, so a later failure or hang cannot erase
# the earlier evidence. Nothing here serializes a large object graph.
function Rec($k, $v) {
  $results[$k] = $v
  $line = (@{ k = $k; v = $v } | ConvertTo-Json -Depth 3 -Compress)
  Add-Content -Encoding UTF8 (Join-Path $out 'results.jsonl') $line
}
function Mark($m) { Add-Content (Join-Path $out 'progress.txt') "$(Get-Date -Format o) $m" }
function Now { (Get-Date).ToUniversalTime().ToString('o') }

$names = 'UC_PORTABLE', 'UC_PROFILE', 'UNICLIPBOARD_ENV', 'UC_DISABLE_SYSTEM_CLIPBOARD', 'UC_GUI_GO_ISOLATED',
  'UC_GPUI_QUICK_PANEL', 'UC_GUI_GO_EVIDENCE', 'UC_GUI_GO_E2E_PHASE', 'UC_GUI_GO_EXIT_MODE', 'UC_GUI_GO_E2E_DIALOG_SAVE'
function Use-Env($profile, $extra = @{}) {
  foreach ($n in $names) { Remove-Item "Env:$n" -ErrorAction SilentlyContinue }
  $env:UC_PORTABLE = '1'; $env:UC_PROFILE = $profile; $env:UNICLIPBOARD_ENV = 'development'
  $env:UC_DISABLE_SYSTEM_CLIPBOARD = '1'; $env:UC_GUI_GO_ISOLATED = '1'; $env:UC_GPUI_QUICK_PANEL = '0'
  foreach ($k in $extra.Keys) { Set-Item "Env:$k" $extra[$k] }
}
function Uc($dir, $profile, [string[]]$cliArgs, [switch]$Fail) {
  Use-Env $profile
  $ErrorActionPreference = 'Continue' # native stderr must not become a terminating error
  $errFile = Join-Path $out 'uniclip.stderr.txt'
  $o = & (Join-Path $dir 'uniclip.exe') @cliArgs 2>$errFile | Out-String
  Add-Content (Join-Path $out 'progress.txt') "$(Get-Date -Format o) uniclip $($cliArgs -join ' ') exit=$LASTEXITCODE"
  if ($Fail -and $LASTEXITCODE -ne 0) { throw "uniclip $($cliArgs -join ' ') failed ($LASTEXITCODE): $o" }
  [pscustomobject]@{ Exit = $LASTEXITCODE; Out = $o }
}
function DaemonPids($dir) {
  Get-Process uniclipd -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($dir) } | ForEach-Object { $_.Id }
}

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public class W32 {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@
function Shot($proc, $file) {
  try {
    $proc.Refresh()
    $h = $proc.MainWindowHandle
    if ($h -eq [IntPtr]::Zero) { return $false }
    $r = New-Object W32+RECT
    [void][W32]::GetWindowRect($h, [ref]$r)
    $w = $r.R - $r.L; $ht = $r.B - $r.T
    if ($w -le 0 -or $ht -le 0) { return $false }
    $bmp = New-Object System.Drawing.Bitmap $w, $ht
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $dc = $g.GetHdc()
    [void][W32]::PrintWindow($h, $dc, 2)
    $g.ReleaseHdc($dc); $g.Dispose()
    $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    return $true
  } catch { return $false }
}

$shotSteps = 'diag-settings-open', 'diag-capture-started', 'diag-export-during-capture', 'diag-capture-stopped',
  'diag-capture-restarted', 'diag-capture-expired', 'diag-reopen-state', 'diag-export-after-reopen', 'diag-export-offline'
$marker = 'diag-accept-' + ([guid]::NewGuid().ToString('N').Substring(0, 20))
Rec 'marker' $marker

function Launch($name, $phase, $exitMode, $extra = @{}, $onStep = $null, $timeoutSec = 1500) {
  $evidence = Join-Path $out "$name.jsonl"
  Set-Content $evidence '' -NoNewline
  $e = @{ UC_GUI_GO_EVIDENCE = $evidence; UC_GUI_GO_E2E_PHASE = $phase; UC_GUI_GO_EXIT_MODE = $exitMode }
  foreach ($k in $extra.Keys) { $e[$k] = $extra[$k] }
  Use-Env 'gui-go-a' $e
  Rec "$name.launchedAt" (Now)
  $proc = Start-Process (Join-Path $gui 'gui-go.exe') -PassThru -WorkingDirectory $gui
  $seen = @{}; $shot = @{}; $cursor = 0; $lastPoll = [datetime]::MinValue
  $deadline = (Get-Date).AddSeconds($timeoutSec)
  $exitSeen = $false
  while ((Get-Date) -lt $deadline) {
    # One more pass after the process exits, so the last evidence lines are not lost.
    if ($proc.HasExited) { if ($exitSeen) { break }; $exitSeen = $true }
    $lines = @(Get-Content $evidence -Encoding UTF8 -ErrorAction SilentlyContinue)
    for (; $cursor -lt $lines.Count; $cursor++) {
      if (-not $lines[$cursor]) { continue }
      $row = $lines[$cursor] | ConvertFrom-Json
      if ($row.step -eq 'driver-error') { throw "driver error: $($row.detail | ConvertTo-Json -Depth 4 -Compress)" }
      $seen[$row.step] = @{ ok = $row.ok; seenAt = (Now); detail = $row.detail }
      # The step callback runs first: screenshot retries can take seconds and must not delay orchestrated traffic.
      if ($onStep) { & $onStep $row.step }
      if ($shotSteps -contains $row.step -and -not $shot[$row.step] -and -not $proc.HasExited) {
        $shot[$row.step] = $true
        for ($i = 0; $i -lt 8; $i++) {
          Start-Sleep -Seconds 1
          $f = Join-Path $out "$name-$($row.step).png"
          if ((Shot $proc $f) -and (Get-Item $f).Length -gt 60000) { break }
        }
      }
    }
    if ($name -eq 'capture' -and ((Get-Date) - $lastPoll).TotalSeconds -ge 5) {
      $lastPoll = Get-Date
      $c = Uc $gui 'gui-go-a' @('--json', 'debug', 'capture', 'status')
      Add-Content -Encoding UTF8 (Join-Path $out 'capture-timeline.jsonl') (@{ at = (Now); exit = $c.Exit; out = ($c.Out -replace '\s+', ' ') } | ConvertTo-Json -Compress)
      Use-Env 'gui-go-a' $e
    }
    Start-Sleep -Milliseconds 300
  }
  if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force; throw "$name`: timeout" }
  $seenAll[$name] = $seen
  foreach ($k in $seen.Keys) { Rec "$name.$k" @{ ok = $seen[$k].ok; seenAt = $seen[$k].seenAt } }
  Rec "$name.guiExit" $proc.ExitCode
  Rec "$name.exitedAt" (Now)
  return $seen
}

try {
  # Pair A and B through the production rendezvous service.
  [void](Uc $gui 'gui-go-a' @('space', 'init', '--passphrase', $Passphrase, '--device-name', 'diag-a') -Fail)
  # `start` right after `space init` can race the init daemon's shutdown and fail with engine error 1101; every
  # attempt is recorded so the race stays visible in the results instead of being hidden by the retry.
  $startOk = $false
  for ($a = 1; $a -le 8 -and -not $startOk; $a++) {
    $r = Uc $gui 'gui-go-a' @('start')
    Rec "startAttempt.$a" @{ exit = $r.Exit; at = (Now) }
    if ($r.Exit -eq 0) { $startOk = $true; break }
    # Keep the daemon's own failure record and the tail of the Engine log for every failed attempt.
    $logs = Join-Path $gui 'data\logs'
    Copy-Item (Join-Path $logs 'daemon-startup-failure.json') (Join-Path $out "start-failure-$a.json") -ErrorAction SilentlyContinue
    Get-ChildItem $logs -Filter 'engine.*.jsonl' -ErrorAction SilentlyContinue | Select-Object -First 1 | ForEach-Object {
      Get-Content $_.FullName -Tail 40 | Set-Content -Encoding UTF8 (Join-Path $out "start-failure-$a.engine-tail.jsonl") }
    Start-Sleep -Seconds 6
  }
  if (-not $startOk) { throw 'daemon start failed after 8 attempts' }
  Use-Env 'gui-go-a'
  $inv = Start-Process (Join-Path $gui 'uniclip.exe') -ArgumentList 'space', 'invite' -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput (Join-Path $out 'invite.txt') -RedirectStandardError (Join-Path $out 'invite.err')
  $code = $null
  for ($i = 0; $i -lt 300 -and -not $code; $i++) {
    Start-Sleep -Milliseconds 300
    $m = Select-String -Path (Join-Path $out 'invite.txt') -Pattern 'INVITATION_CODE=(.+)' -ErrorAction SilentlyContinue
    if ($m) { $code = $m.Matches[0].Groups[1].Value.Trim() }
  }
  if (-not $code) { throw 'no invitation code' }
  [void](Uc $peer 'gui-go-b' @('space', 'join', '--code', $code, '--passphrase', $Passphrase, '--device-name', 'diag-b') -Fail)
  $members = 0
  for ($i = 0; $i -lt 45 -and $members -lt 2; $i++) {
    $l = Uc $gui 'gui-go-a' @('--json', 'member', 'list')
    # Windows PowerShell 5.1 wraps a parsed JSON array in one object, so count the entries textually.
    $members = ([regex]::Matches($l.Out, '"device_id"')).Count
    if ($members -lt 2) { Start-Sleep -Seconds 2 }
  }
  if ($members -lt 2) { throw "peer did not join; last member list: $($l.Out)" }
  Stop-Process -Id $inv.Id -Force -ErrorAction SilentlyContinue
  Rec 'pairedAt' (Now)

  # 1. capture
  $sent = $false
  $onStep = {
    param($step)
    if ($step -eq 'diag-capture-started' -and -not $script:sent) {
      $script:sent = $true
      Rec 'sentAt' (Now)
      $s = Uc $peer 'gui-go-b' @('--json', 'send', $marker)
      Rec 'sendExit' $s.Exit
    }
  }
  $phase = "diag:capture:$marker" + $(if ($NoExpiryWait) { ':nowait' } else { '' })
  $seen = Launch 'capture' $phase 'keep' @{} $onStep 3000
  $need = @('diag-settings-open', 'diag-capture-started', 'diag-marker-received', 'diag-export-during-capture', 'diag-capture-stopped', 'diag-capture-restarted')
  if (-not $NoExpiryWait) { $need += 'diag-capture-expired' }
  foreach ($s in $need) { if (-not $seen[$s] -or -not $seen[$s].ok) { throw "capture step failed: $s" } }
  Rec 'daemonAliveAfterGuiClose' (@(DaemonPids $gui).Count -gt 0)
  # Exact equality of the received text (not a substring match): `get` prints the newest entry's text.
  $got = Uc $gui 'gui-go-a' @('get')
  Rec 'receivedText' $got.Out.Trim()
  Rec 'receivedExact' ($got.Out.Trim() -ceq $marker)
  Rec 'getExit' $got.Exit

  # 2. reopen, open a fresh Detailed capture, send fresh traffic from peer B, then quit fully (graceful daemon stop).
  $tailSent = $false
  $onTail = {
    param($step)
    if ($step -eq 'diag-tail-ready' -and -not $script:tailSent) {
      $script:tailSent = $true
      Rec 'tailSentAt' (Now)
      $codes = @(1..3 | ForEach-Object { (Uc $peer 'gui-go-b' @('--json', 'send', "$marker-tail-$_")).Exit })
      Rec 'tailSendExits' ($codes -join ',')
    }
  }
  $seen = Launch 'reopen' "diag:reopen:$marker" 'full' @{} $onTail
  $quitAt = Get-Date
  while (@(DaemonPids $gui).Count -gt 0 -and ((Get-Date) - $quitAt).TotalSeconds -lt 60) { Start-Sleep -Milliseconds 300 }
  Rec 'daemonStoppedByFullQuit' (@(DaemonPids $gui).Count -eq 0)
  Rec 'daemonStopSecondsAfterGuiExit' ([math]::Round(((Get-Date) - $quitAt).TotalSeconds, 1))
  $fin = $seenAll['reopen']['diag-tail-final'].detail
  $c = $fin.final.network.counters
  $dropped = [int64]$c.queueDropped + [int64]$c.closedDropped + [int64]$c.writeFailed
  $runId = $seenAll['capture']['diag-capture-started'].detail.snapshot.network.runId
  $runDir = Join-Path $gui "data\logs\network\daemon\$runId"
  $onDisk = 0
  Get-ChildItem $runDir -Filter 'network.*.jsonl' | ForEach-Object { $onDisk += @(Get-Content $_.FullName).Count }
  Rec 'tailAcceptedBefore' $fin.acceptedBefore
  Rec 'tailAcceptedFinal' $fin.acceptedFinal
  Rec 'tailDropped' $dropped
  Rec 'tailOnDisk' $onDisk
  Rec 'tailPersisted' (($onDisk + $dropped) -ge $fin.acceptedFinal)
  if (-not $seen['diag-tail-final'].ok) { throw 'no fresh accepted network records before the graceful exit' }

  # 3. new daemon run: export again, then the offline path
  $saved = Join-Path $out 'offline.zip'
  $stopped = $false
  $onStop = {
    param($step)
    if ($step -eq 'diag-ready-for-offline' -and -not $script:stopped) {
      $script:stopped = $true
      Rec 'stoppedAt' (Now)
      $pidsBefore = @(DaemonPids $gui)
      $t0 = Get-Date
      $s = Uc $gui 'gui-go-a' @('--json', 'stop')
      Rec 'offlineStopExit' $s.Exit
      Rec 'offlineStopOutput' ($s.Out -replace '\s+', ' ')
      Rec 'offlineStopCommandSeconds' ([math]::Round(((Get-Date) - $t0).TotalSeconds, 1))
      while (@(DaemonPids $gui).Count -gt 0 -and ((Get-Date) - $t0).TotalSeconds -lt 120) { Start-Sleep -Milliseconds 300 }
      Rec 'offlineDaemonGoneAfterSeconds' ([math]::Round(((Get-Date) - $t0).TotalSeconds, 1))
      Rec 'offlineDaemonStillAlive' (@(DaemonPids $gui).Count -gt 0)
    }
  }
  $seen = Launch 'offline' 'diag:offline' 'full' @{ UC_GUI_GO_E2E_DIALOG_SAVE = $saved } $onStop
  foreach ($s in 'diag-export-after-reopen', 'diag-daemon-unreachable', 'diag-export-offline') {
    if (-not $seen[$s] -or -not $seen[$s].ok) { throw "offline step failed: $s" }
  }
  Rec 'passed' $true
} catch {
  Rec 'error' ($_.Exception.Message + ' @' + $_.InvocationInfo.ScriptLineNumber)
} finally {
  Mark "finally; error=$($results['error'])"
  Get-Process gui-go -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($gui) } | Stop-Process -Force
  [void](Uc $gui 'gui-go-a' @('--json', 'stop'))
  [void](Uc $peer 'gui-go-b' @('--json', 'stop'))
  foreach ($n in 'capture', 'offline') {
    foreach ($k in 'diag-export-during-capture', 'diag-export-after-reopen') {
      $step = $seenAll[$n]
      $p = if ($step -and $step[$k]) { $step[$k].detail.path } else { $null }
      if ($p -and (Test-Path $p)) { Copy-Item $p (Join-Path $out "$n-$k.zip") -Force; Rec "export.$n.$k" "$n-$k.zip" }
    }
  }
  # Raw per-run counts, and a copy of the network and log directories for later verification.
  $logsDir = Join-Path $gui 'data\logs'
  if (Test-Path $logsDir) { Copy-Item (Join-Path $logsDir 'network') (Join-Path $out 'network-profile-a') -Recurse -Force -ErrorAction SilentlyContinue }
  Get-ChildItem (Join-Path $logsDir 'network\daemon') -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $recs = 0
    Get-ChildItem $_.FullName -Filter 'network.*.jsonl' | ForEach-Object { $recs += @(Get-Content $_.FullName).Count }
    Rec "networkRun.$($_.Name)" $recs
  }
  'done' | Set-Content (Join-Path $root 'acceptance.done')
}
