$ErrorActionPreference = 'Stop'

$packageArgs = @{
  packageName  = 'uniclipboard'
  softwareName = 'UniClipboard*'
  fileType     = 'exe'
  silentArgs   = '/S'   # NSIS silent uninstall
  validExitCodes = @(0)
}

[array]$key = Get-UninstallRegistryKey -SoftwareName $packageArgs['softwareName']

if ($key.Count -eq 1) {
  $key | ForEach-Object {
    $packageArgs['file'] = "$($_.UninstallString)"
    Uninstall-ChocolateyPackage @packageArgs
  }
} elseif ($key.Count -eq 0) {
  Write-Warning "$($packageArgs.packageName) has already been uninstalled by other means."
} elseif ($key.Count -gt 1) {
  Write-Warning "$($key.Count) matches found!"
  Write-Warning "To prevent accidental data loss, no programs will be uninstalled."
  $key | ForEach-Object { Write-Warning "- $($_.DisplayName)" }
}
