# Security Policy

UniClipboard is a security-oriented, end-to-end-encrypted clipboard sync tool.
This document explains how to report vulnerabilities and how to verify the
integrity and authenticity of the binaries we publish.

## Supported Versions

UniClipboard is pre-1.0 and ships from a single active release line. Security
fixes land on the latest released minor; older builds are not maintained.
Please update to the latest release before reporting an issue.

| Version         | Supported          |
| --------------- | ------------------ |
| Latest `0.14.x` | :white_check_mark: |
| Older `0.x`     | :x:                |

## Reporting a Vulnerability

Please report security issues **privately** — do **not** open a public issue for
an unfixed vulnerability.

- Preferred: open a private report through GitHub Security Advisories on this
  repository (the **"Report a vulnerability"** button under the **Security**
  tab). This keeps the disclosure private until a fix is available.

We aim to acknowledge new reports within a few business days and will keep you
updated through triage, the fix, and coordinated disclosure. Thank you for
helping keep UniClipboard users safe.

## Verifying Release Downloads

UniClipboard uses two **independent** signing mechanisms, both built on
[minisign](https://jedisct1.github.io/minisign/)-compatible Ed25519 keys. The
two keys are intentionally separate so they can be rotated independently.

### 1. In-app auto-updater (always on)

The Tauri auto-updater cryptographically verifies every update bundle it
downloads against a public key embedded in the application. You do not need to
do anything: an update whose signature is missing or invalid is rejected
automatically.

For reference, the updater public key is:

```
untrusted comment: minisign public key: B2680836865C2738
RWQ4J1yGNghostY9tL54b8pVCWvFIc7ebO9iD11Hvf2fqcMYemYwtIWb
```

This key signs the **updater payloads only** — `*.app.tar.gz` (macOS),
`*.AppImage.tar.gz` (Linux) and `*.nsis.zip` (Windows) — whose detached `*.sig`
files are attached to every GitHub release. It is the same key shipped in the
application configuration, so it is fully public.

> On macOS, release builds are additionally Apple-notarized and code-signed, so
> Gatekeeper (`spctl --assess --type execute`) validates the `.app` directly.

### 2. Release artifacts (`SHA256SUMS`)

Starting with the first signed release, every GitHub release includes:

- `SHA256SUMS.txt` — SHA-256 checksums of every release artifact, and
- `SHA256SUMS.txt.minisig` — a minisign signature over that checksum file.

The release-artifact public key is:

```
untrusted comment: minisign public key: 0659AAD44E7EB54C
RWRMtX5O1KpZBhZHfGaa4gqlbwnzJMINb65be0QNzl8RKwK7VOwkMvO8
```

To verify a download:

```sh
# 1. Authenticate the checksum list against the release key
minisign -Vm SHA256SUMS.txt -P 'RWRMtX5O1KpZBhZHfGaa4gqlbwnzJMINb65be0QNzl8RKwK7VOwkMvO8'

# 2. Check your download's integrity against the (now-trusted) list
sha256sum --ignore-missing -c SHA256SUMS.txt          # Linux
# macOS: brew install coreutils, then:
# gsha256sum --ignore-missing -c SHA256SUMS.txt
```

If both checks pass, the file you downloaded is authentic and untampered.
