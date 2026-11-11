# Security policy

## Scope

The active implementation is developed on `v2` while the retired implementation
is preserved on `v1`. Include the branch and exact commit or release version in
security reports. This policy does not promise maintenance for the retired line.

## Report a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/UniClipboard/UniClipboard/security/advisories/new).
Do not disclose an unfixed vulnerability in a public issue.
Include reproduction steps, affected platforms, impact and the relevant version.
Remove clipboard content, credentials, device addresses and pairing tickets from
attachments unless they are essential to the private report.

## Release verification

The current v2 local packaging command produces archives and a SHA-256 manifest.
These archives are unsigned. A checksum verifies integrity against the manifest;
it does not establish publisher authenticity.
See the [headless architecture](docs/architecture/headless.md) for current
packaging boundaries. Do not apply v1 updater keys or signing claims to v2.
