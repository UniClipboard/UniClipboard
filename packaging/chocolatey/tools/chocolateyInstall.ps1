$ErrorActionPreference = 'Stop'

# Version comes from the nuspec at install time (Chocolatey injects it). The
# publish workflow then only needs to bump the nuspec <version> and checksum64.
$version = $env:ChocolateyPackageVersion
$packageArgs = @{
  packageName    = 'uniclipboard'
  fileType       = 'exe'
  # Tauri NSIS installer. ARM64 Windows runs the x64 build under emulation, so a
  # single x64 installer covers both; add url/checksum for arm64 if you want a
  # native ARM64 install path.
  url64bit       = "https://github.com/UniClipboard/UniClipboard/releases/download/v$version/UniClipboard_${version}_x64-setup.exe"
  # Placeholder. Fill before pushing — see ..\README.md (Get-RemoteChecksum or
  # the minisign-signed SHA256SUMS.txt from the release). Do NOT trust a hash
  # produced by an untrusted shell.
  checksum64     = 'REPLACE_WITH_SHA256'
  checksumType64 = 'sha256'
  silentArgs     = '/S'   # NSIS silent install
  validExitCodes = @(0)
  softwareName   = 'UniClipboard*'
}

Install-ChocolateyPackage @packageArgs
