# Clipboard source metadata execution receipt

## Scope

Capture application identity and an optional icon. Preserve both through adapter IPC, Engine, history, plugin backends and peer synchronization.
The implementation uses existing native libraries and keeps the wire version at 1.
No native UI acceptance or device launch is part of this run.

## Failure inventory

- Owner unavailable, terminated, changed during read, or missing an application identifier.
- Icon unavailable, oversized, malformed, or mutated by an API caller.
- Foreground inference differs from the application that actually copied the content.
- Metadata lost in adapter, plugin RPC, wire conversion, policy filtering, restart, or restore.
- Metadata changes create duplicate history or application echoes overwrite provenance.
- Unknown provenance inherits stale metadata from the previous clipboard item.
- Metadata escapes storage budgets or leaks local paths and window titles.

## Implementation

The [source application contract](../contracts/source-application-contract.md) defines data and ownership.
The existing content hash and observation protocol remain unchanged.
Native extraction has a bounded memory cache. History owns persistent per-record icon snapshots.
There is no new dependency, configuration layer, or independent cache retention policy.

## Validation

Validated on macOS arm64 with Go 1.27.3. Initial source HEAD: `b4fff3b8f57d512274322de0ed0317e52a912c72`.
Acceptance ran before implementation commits. Artifact manifests identify the tested source files by SHA-256.

| Check | Result | Evidence |
| --- | --- | --- |
| New capture/sync/restart/restore/no-icon/unknown-source/metadata-only/quota scenario | Passed with race detection | `.build/source-metadata/final-cli/script-clipboard-source-metadata/manifest.json` |
| Nine CLI process scenarios | 9 passed with race detection | `.build/source-metadata/final-cli/*/manifest.json` |
| Production CLI/service acceptance | 14 checks passed with race detection; source unchanged during acceptance | `.build/source-metadata/production-v2/manifest.json` |
| macOS Desktop build | Passed; no application launched | `.build/source-metadata/build-proof.json` |
| Windows amd64 and Linux amd64 native Board package builds | Passed; cross-compilation only | `.build/source-metadata/build-proof.json` |
| All-package race compilation | Passed with no tests selected | `go test -race ./... -run '^$'` |
| Changed-package static analysis and diff whitespace check | Passed | `go vet` and `git diff --check` |
| Documentation check | Passed with 8 stale-page warnings | `go run ./tools/docscheck` |
| Pre-commit architecture verification marker attempt | Refused by docscheck because source was uncommitted | `go run ./tools/docscheck -bump docs/architecture/pipeline.md` |

The nine CLI scenarios are `clipboard-source-metadata`, `content-formats`, `history-restore-sync`,
`history-auto-cleanup`, `history-cache-quota`, `file-cache-retention`, `plugin-tag-and-action`,
`pair-and-sync-text`, and `temporary-transfer`.

An independent archive of the original HEAD reproduced the existing plugin email-tag assertion failure.
Its output was `builtin:text,email`; the assertion required `email` to be the first tag.
Only the E2E assertion was corrected to accept CSV tag membership. The final plugin Action scenario passed.
The production acceptance helper was also updated to verify all exported parts when testing rich text.

Native attribution, native icon extraction, and cross-device OS clipboard behavior were not run.
macOS foreground attribution remains an inference when a source marker is absent.
X11 owners without WM_CLASS and native Wayland produce unknown provenance.
This capture/sync acceptance did not exercise visual UI behavior. Publication is outside the scope of this acceptance receipt.
Temporary binaries, raw logs, and the original-source archive are removed after acceptance.
Only manifests, build hashes, this receipt, and implementation/contract files remain.

## Publication preparation

Implementation source commit: `7aff73d6caa87ea37c31d4e80603286bd42d4b6b`.
The [architecture verification marker](../architecture/pipeline.md) was refreshed after implementation commits.
Acceptance limits above remain unchanged. Native clipboard attribution and native-window acceptance remain not run.

## 2026-11-13 rebase integration

Rebased onto `origin/v2` at `16d0ecefc64f7e6f778e37185a018157726dcc23`.
Validated implementation source: `5e8a90ffaaf68393c2b35125982e7d2d1e1f39ca`. New manifests bind the rebased source by file hash; earlier acceptance records above are historical.

The integration retains directory copy markers, deferred references, manifest verification and non-file storage accounting together with application metadata.
Directory preparation copies the original source application into the immutable snapshot.
Windows source probing uses a distinct clipboard-sequence symbol alongside the upstream copy marker.
The upstream directory CLI fixture's two tag selectors now use `builtin:folder`, matching current classification.
An independent archive of the exact upstream base reproduced the original selector failure before this correction.

| Check | Result | Evidence |
| --- | --- | --- |
| Directory and source-metadata CLI process acceptance | 15 scenarios passed with race detection; frozen source unchanged | `.build/rebase-source-metadata/cli-final/receipt.json` |
| Directory application provenance | Explicit history send, receiver restart, automatic send and repeated same-directory copy passed | `directory-source-metadata` within the CLI receipt |
| Production headless acceptance | 14 checks passed with race detection; source unchanged during acceptance | `.build/rebase-source-metadata/headless/manifest.json` |
| Desktop source application presentation | 48 view subcases passed | `.build/desktop-e2e/20261112T170510Z-d05efecf/index.html` |
| Desktop production-service binding | Passed | `.build/desktop-e2e/20261112T170618Z-5faf2d35/index.html` |
| Desktop localization | Passed | `.build/desktop-e2e/20261112T170618Z-fd5babe7/index.html` |
| macOS Desktop build; Windows/Linux Board cross-compiles; changed-package vet | Passed | Local build and static checks |
| All-package race compilation | Passed with no tests selected | `go test -race ./... -run '^$'` |

The new directory case waits for a real transfer after receiver restart before testing automatic capture.
This avoids treating a service-ready socket as proof that a peer session has reconnected.
Native clipboard attribution, native icon extraction and native-window acceptance remain not run.
Architecture verification markers were refreshed after the rebased implementation commit.
