# Runs on the Windows test machine, inside the logged-in desktop session: builds the Rust half, stages the binaries and
# runs windows_native_panel_run.py. Started by windows_native_panel_remote.py (as a scheduled task bound to the desktop
# session); it can also be run by hand from a desktop terminal. Everything lives under -Work.
#
#   <Work>\src       the source tree (the part of the repository the build needs)
#   <Work>\python    an embeddable Python, the interpreter of the E2E scripts
#   <Work>\go        gui-go.exe and uniclip.exe, built on another machine for this architecture
#   <Work>\stage     the four executables of the run
#   <Work>\w         the sandbox root of the run (a fixed path: Windows Firewall keys its decision by program path)
#   <Work>\run       host.log, e2e.log, status.txt (RUNNING / PASSED / FAILED / ERROR), out\ (assertions and screenshots)
param(
    [string]$Work = 'C:\uc-e2e',
    [switch]$FirewallPromptOk,
    [switch]$ClobberClipboard
)
$ErrorActionPreference = 'Stop'
$run = "$Work\run"
$log = "$run\host.log"
function Step([string]$message) { "$(Get-Date -Format 'HH:mm:ss') $message" | Add-Content -Encoding utf8 $log }
# Runs a native command with its output (stderr included: cargo reports progress there) appended to a log; returns the exit code.
function Invoke-Logged([string]$logFile, [string]$program, [string[]]$programArguments) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & $program @programArguments 2>&1 | ForEach-Object { "$_" } | Add-Content -Encoding utf8 $logFile
    $ErrorActionPreference = $previous
    return $LASTEXITCODE
}
function Status([string]$value) { Set-Content -Encoding ascii "$run\status.txt" $value }
Status 'RUNNING'
try {
    $py = "$Work\python\python.exe"
    $e2e = "$Work\src\apps\gui-go\e2e"
    $stage = "$Work\stage"
    Step "host $env:COMPUTERNAME, session $((Get-Process -Id $PID).SessionId), user $env:USERNAME"

    Step 'build: cargo (uniclipd, uniclip-quick-panel)'
    Set-Location "$Work\src"
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    if ((Get-Command clang -ErrorAction SilentlyContinue) -eq $null -and $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
        throw 'building the Rust half on Windows ARM64 needs clang on PATH (the ring crate)'
    }
    $code = Invoke-Logged "$run\build.log" $py @("$e2e\build_windows.py", '--mode', 'e2e', '--part', 'rust')
    if ($code -ne 0) { throw "build_windows.py exited with $code (see build.log)" }

    Step 'stage'
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item "$Work\go\*.exe" $stage
    Copy-Item "$Work\src\target\gui-go\windows-e2e\*.exe" $stage
    Get-ChildItem $stage | ForEach-Object { Step ("  {0} {1} {2}" -f $_.Name, $_.Length, (Get-FileHash $_.FullName -Algorithm SHA256).Hash) }

    Step 'e2e'
    $env:UC_GUI_GO_E2E_DEDICATED_HOST = '1'
    $env:UC_GUI_GO_E2E_SANDBOX_ROOT = "$Work\w"
    if ($FirewallPromptOk) { $env:UC_GUI_GO_E2E_FIREWALL_PROMPT_OK = '1' }
    New-Item -ItemType Directory -Force "$Work\w" | Out-Null
    $arguments = @("$e2e\windows_native_panel_run.py", '--out', "$run\out", '--binaries', $stage)
    if ($ClobberClipboard) { $arguments += '--clobber-clipboard' }
    $code = Invoke-Logged "$run\e2e.log" $py $arguments
    Step "e2e exit $code"
    Status $(if ($code -eq 0) { 'PASSED' } else { 'FAILED' })
} catch {
    Step "ERROR: $_"
    Status 'ERROR'
}
