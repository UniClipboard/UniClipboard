param(
  [Parameter(Mandatory=$true)][string]$Installer,
  [Parameter(Mandatory=$true)][string]$Newer,
  [Parameter(Mandatory=$true)][string]$Receipt,
  [Parameter(Mandatory=$true)][string]$Target
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Use only an ephemeral GitHub-hosted Windows runner.' }
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard'
if (Test-Path $key) { throw 'Refuse an existing UniClipboard installation.' }
$work = Join-Path $env:RUNNER_TEMP ('unic-install-' + [guid]::NewGuid().ToString('N'))
$install = Join-Path $work 'Installation with spaces'
$profile = Join-Path $work 'profile'
$oldHome = $env:UNIC_HOME
$oldPath = $env:PATH
$oldUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$oldMachinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
New-Item -ItemType Directory -Path $profile -Force | Out-Null
$env:UNIC_HOME = $profile
$Installer = (Resolve-Path $Installer).Path
$Newer = (Resolve-Path $Newer).Path
$checks = @()
function Install-Package([string]$File) {
  $process = Start-Process -FilePath $File -ArgumentList "/S /MANAGED=winget /D=$install" -Wait -PassThru
  if ($process.ExitCode -ne 0) { throw "Installer failed: $($process.ExitCode)" }
}
try {
  Install-Package $Installer
  $value = (& (Join-Path $install 'unic-desktop.exe') --version --json | ConvertFrom-Json)
  if ($value.version -ne '2.0.0' -or $value.build_number -ne '1') { throw 'Wrong installed Desktop identity.' }
  $checks += 'installation'
  New-Item -ItemType Directory -Path (Join-Path $install 'data') | Out-Null
  Set-Content -Path (Join-Path $install 'data/preserve.txt') -Value 'synthetic-user-data'
  Install-Package $Installer
  $checks += 'reinstallation'
  & (Join-Path $install 'unic.exe') status --json | Set-Content (Join-Path $profile 'status.json')
  if ($LASTEXITCODE -ne 0) { throw 'Bundled CLI/service lifecycle failed.' }
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  while ((Test-Path (Join-Path $profile 'unic.sock')) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
  if (Test-Path (Join-Path $profile 'unic.sock')) { throw 'Temporary service did not release its socket.' }
  $checks += 'temporary_service_shutdown'
  Install-Package $Newer
  $value = (& (Join-Path $install 'unic-desktop.exe') --version --json | ConvertFrom-Json)
  if ($value.build_number -ne '2') { throw 'Upgrade did not select the later build.' }
  $checks += 'upgrade'
  if ((Get-Content (Join-Path $install 'update-owner') -Raw).Trim() -ne 'winget') { throw 'Update ownership was lost.' }
  if ($env:PATH -ne $oldPath -or [Environment]::GetEnvironmentVariable('Path', 'User') -ne $oldUserPath -or [Environment]::GetEnvironmentVariable('Path', 'Machine') -ne $oldMachinePath) { throw 'Desktop modified public CLI ownership.' }
  $checks += 'private_cli_ownership'
  $uninstaller = Join-Path $install 'uninstall.exe'
  $process = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) { throw 'Uninstall failed.' }
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  while (((Test-Path (Join-Path $install 'unic-desktop.exe')) -or (Test-Path (Join-Path $install 'uninstall.exe')) -or (Test-Path $key)) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
  if ((Test-Path (Join-Path $install 'unic-desktop.exe')) -or (Test-Path (Join-Path $install 'uninstall.exe')) -or (Test-Path $key)) { throw 'Owned executable, uninstaller or registration remains.' }
  $checks += 'removal'
  if ((Get-Content (Join-Path $install 'data/preserve.txt') -Raw).Trim() -ne 'synthetic-user-data') { throw 'Uninstall removed user data.' }
  $checks += 'data_preservation'
  New-Item -ItemType Directory -Path $Receipt -Force | Out-Null
  @{
    status='passed'; target=$Target; format='setup.exe'; scope='native NSIS lifecycle with an unsigned later-build fixture';
    artifacts_sha256=@((Get-FileHash $Installer -Algorithm SHA256).Hash.ToLower(), (Get-FileHash $Newer -Algorithm SHA256).Hash.ToLower());
    assertions=$checks; gui='not_run'; v1_replacement='not_run'; update_owner='winget'
  } | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $Receipt 'report.json')
  $digest = (Get-FileHash (Join-Path $Receipt 'report.json') -Algorithm SHA256).Hash.ToLower()
  "$digest  report.json" | Set-Content -Encoding utf8 (Join-Path $Receipt 'SHA256SUMS')
} catch {
  New-Item -ItemType Directory -Path $Receipt -Force | Out-Null
  @{status='failed'; target=$Target; format='setup.exe'; assertions=$checks; error_type=$_.Exception.GetType().FullName} | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $Receipt 'report.json')
  throw
} finally {
  if (Test-Path $key) {
    $uninstaller = Join-Path $install 'uninstall.exe'
    if (Test-Path $uninstaller) { Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait | Out-Null }
  }
  $env:UNIC_HOME = $oldHome
  if (Test-Path $work) { Remove-Item -Recurse -Force $work }
  if (Test-Path (Join-Path $Receipt 'report.json')) {
    $digest = (Get-FileHash (Join-Path $Receipt 'report.json') -Algorithm SHA256).Hash.ToLower()
    "$digest  report.json" | Set-Content -Encoding utf8 (Join-Path $Receipt 'SHA256SUMS')
  }
}
