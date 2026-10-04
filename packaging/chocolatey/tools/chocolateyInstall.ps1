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
  # Cross-checked against the v1.1.0 release's minisign-signed SHA256SUMS.txt.
  checksum64     = '64932773bd70a1a3d3c53f43eb1144e68e60fd598c7347e63a2896a18b25f1cf'
  checksumType64 = 'sha256'
  silentArgs     = '/S'   # NSIS silent install
  validExitCodes = @(0)
  softwareName   = 'UniClipboard*'
}

Install-ChocolateyPackage @packageArgs
