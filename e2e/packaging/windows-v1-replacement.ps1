param(
  [Parameter(Mandatory=$true)][string]$V1Installer,
  [Parameter(Mandatory=$true)][string]$V1Sha256,
  [Parameter(Mandatory=$true)][string]$Installer,
  [Parameter(Mandatory=$true)][string]$Fixture,
  [Parameter(Mandatory=$true)][string]$FixtureMetadata,
  [Parameter(Mandatory=$true)][string]$Root,
  [Parameter(Mandatory=$true)][string]$Receipt,
  [string]$Target = 'windows-arm64',
  [switch]$LegacyUpdater
)
$ErrorActionPreference = 'Stop'
if (Test-Path $Root) { throw 'Require a fresh task-owned root.' }
if ($env:GITHUB_ACTIONS -eq 'true') {
  if ((Split-Path $Root) -ne $env:RUNNER_TEMP) { throw 'Use a runner-owned temporary directory.' }
} else {
  if ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value -ne 'S-1-5-18') { throw 'Use only the isolated SYSTEM account in an explicitly authorized VM.' }
  if ($Root -notmatch '^C:\\ProgramData\\UniClipboard-Acceptance-[a-zA-Z0-9-]+$') { throw 'Require a fresh task-owned root.' }
}
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard'
if (Test-Path $key) { throw 'Refuse an existing installation.' }
if (Get-Process -Name UniClipboard,uniclipd,uniclip-quick-panel,unic-desktop,unicd -ErrorAction SilentlyContinue) { throw 'Refuse running applications in any session.' }
if ((Get-FileHash $V1Installer -Algorithm SHA256).Hash.ToLower() -ne $V1Sha256) { throw 'Official v1 asset hash mismatch.' }
$install = Join-Path $Root 'Installation with spaces'
$legacy = Join-Path $Root 'v1-data'
$profile = Join-Path $Root 'v2-data'
$checks = @()
function Stop-LaunchedDesktop {
  foreach ($process in (Get-Process -Name unic-desktop,unicd -ErrorAction SilentlyContinue)) {
    if ($process.Path -and (Split-Path $process.Path) -eq $install) {
      Stop-Process -InputObject $process -ErrorAction SilentlyContinue
      $null = $process.WaitForExit(10000)
    }
  }
}
$oldHome = $env:UNIC_HOME
$oldPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$oldMachinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
New-Item -ItemType Directory $Root | Out-Null
$env:UNIC_HOME = $profile
function Snapshot([string]$Directory) {
  return (@(Get-ChildItem $Directory -Recurse -File | Sort-Object FullName | ForEach-Object {
    $_.FullName.Substring($Directory.Length) + ':' + (Get-FileHash $_.FullName -Algorithm SHA256).Hash
  }) -join "`n")
}
function Install([string]$File, [string]$Options) {
  $p = Start-Process -FilePath $File -ArgumentList "/S $Options /D=$install" -PassThru
  if (-not $p.WaitForExit(60000)) { $p.Kill(); throw 'Task installer timed out.' }
  if ($p.ExitCode -ne 0) { throw "Task installer failed: $($p.ExitCode)" }
}
try {
  # /UPDATE skips the unrelated WebView2 runtime bootstrap. No /R means no GUI launch.
  Install $V1Installer '/UPDATE'
  $v1 = Join-Path $install 'UniClipboard.exe'
  if (-not (Test-Path $v1)) { throw 'Actual v1 executable is missing.' }
  $v1Version = (Get-ItemProperty $key).DisplayVersion
  if ($v1Version -notlike '1.6.2*') { throw 'Wrong actual v1 version.' }
  $v1Binary = (Get-FileHash $v1 -Algorithm SHA256).Hash.ToLower()
  $checks += 'official_v1_installed'
  Copy-Item $Fixture $legacy -Recurse
  $before = Snapshot $legacy
  if ($LegacyUpdater) {
    if ($v1Version -ne '1.6.2-alpha.2') { throw 'The legacy updater origin must be 1.6.2-alpha.2.' }
    $env:UNIC_HOME = Join-Path $Root 'restart-profile'
    try {
      $p = Start-Process -FilePath $Installer -ArgumentList '/P /R /UPDATE /ARGS --resident-restart' -PassThru
      if (-not $p.WaitForExit(60000)) { $p.Kill(); throw 'Legacy updater installer timed out.' }
      if ($p.ExitCode -ne 0) { throw "Legacy updater installer failed: $($p.ExitCode)" }
      $deadline = [DateTime]::UtcNow.AddSeconds(15)
      $desktopPath = Join-Path $install 'unic-desktop.exe'
      do {
        $restarted = Get-Process -Name unic-desktop -ErrorAction SilentlyContinue |
          Where-Object { $_.Path -eq $desktopPath }
        if (-not $restarted) { Start-Sleep -Milliseconds 100 }
      } while (-not $restarted -and [DateTime]::UtcNow -lt $deadline)
      if (-not $restarted) { throw 'The legacy installer invocation did not relaunch v2.' }
      $checks += 'legacy_updater_passive_arguments_accepted'
      $checks += 'legacy_updater_relaunched_installed_v2'
    } finally {
      Stop-LaunchedDesktop
      $env:UNIC_HOME = $profile
    }
  } else {
    Install $Installer ''
  }
  if ((Get-ItemProperty $key).InstallLocation.Trim('"') -ne $install) { throw 'v2 did not replace the v1 installation location.' }
  foreach ($name in @('UniClipboard.exe','uniclipd.exe','uniclip-quick-panel.exe')) {
    if (Test-Path (Join-Path $install $name)) { throw 'A retired v1 executable remains.' }
  }
  $identity = (& (Join-Path $install 'unic-desktop.exe') --version --json | ConvertFrom-Json)
  if ($identity.version -ne '2.0.0') { throw 'Wrong v2 identity.' }
  $checks += 'v2_replaces_actual_v1_programs'
  if ((Snapshot $legacy) -ne $before) { throw 'Program replacement changed v1 data.' }
  $checks += 'v1_data_preserved_after_replacement'
  $fixtureData = Get-Content $FixtureMetadata -Raw | ConvertFrom-Json
  $result = $fixtureData.passphrase | & (Join-Path $install 'unic.exe') migrate --source $legacy --target $profile --passphrase-stdin --json
  if ($LASTEXITCODE -ne 0) { throw 'Synthetic v1 history migration failed.' }
  $migration = @($result)[-1] | ConvertFrom-Json
  if ($migration.status -ne 'complete' -or $migration.imported -ne 1 -or $migration.verified -ne 1) { throw 'Migration did not reopen and verify synthetic history.' }
  $checks += 'synthetic_v1_history_imported_and_reopened'
  $profileBefore = Snapshot $profile
  $p = Start-Process (Join-Path $install 'uninstall.exe') -ArgumentList '/S' -PassThru
  if (-not $p.WaitForExit(30000) -or $p.ExitCode -ne 0) { throw 'Uninstall failed.' }
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  while (((Test-Path $key) -or (Test-Path (Join-Path $install 'uninstall.exe'))) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
  if ((Test-Path $key) -or (Test-Path (Join-Path $install 'unic-desktop.exe'))) { throw 'Program registration remains after uninstall.' }
  if ((Snapshot $legacy) -ne $before -or (Snapshot $profile) -ne $profileBefore) { throw 'Uninstall changed v1/v2 user data.' }
  if ([Environment]::GetEnvironmentVariable('Path', 'User') -ne $oldPath -or [Environment]::GetEnvironmentVariable('Path', 'Machine') -ne $oldMachinePath) { throw 'Public CLI PATH changed.' }
  $checks += 'uninstall_preserves_v1_and_migrated_v2_data'
  $report = @{status='passed'; assertions=$checks; target=$Target; v1_version=$v1Version; v1_binary_sha256=$v1Binary; v1_installer_sha256=$V1Sha256; v2_installer_sha256=(Get-FileHash $Installer -Algorithm SHA256).Hash.ToLower(); v2_identity=$identity; gui='interaction_not_run'; scope='actual official v1 NSIS replacement; synthetic frozen encrypted v3 history; WebView2 bootstrap excluded'}
} catch {
  $report = @{status='failed'; assertions=$checks; error_type=$_.Exception.GetType().FullName; error=$_.Exception.Message; v1_version=$v1Version}
  throw
} finally {
  try {
    if (Test-Path $key) {
      $p = Start-Process (Join-Path $install 'uninstall.exe') -ArgumentList '/S' -PassThru
      if (-not $p.WaitForExit(30000)) { $p.Kill(); throw 'Task uninstall cleanup timed out.' }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ((Test-Path (Join-Path $install 'uninstall.exe')) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
    if (Test-Path $Root) { Remove-Item $Root -Recurse -Force }
  } catch {
    $report.status = 'failed'
    $report.cleanup_error_type = $_.Exception.GetType().FullName
  }
  $env:UNIC_HOME = $oldHome
  New-Item -ItemType Directory $Receipt -Force | Out-Null
  $report.cleanup_root_removed = -not (Test-Path $Root)
  $report.cleanup_registration_removed = -not (Test-Path $key)
  if (-not $report.cleanup_registration_removed) { $report.status = 'failed' }
  $report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 (Join-Path $Receipt 'report.json')
  ((Get-FileHash (Join-Path $Receipt 'report.json') -Algorithm SHA256).Hash.ToLower() + '  report.json') | Set-Content -Encoding utf8 (Join-Path $Receipt 'SHA256SUMS')
}
if ($report.status -ne 'passed') { throw 'Native replacement cleanup failed; inspect the receipt.' }
