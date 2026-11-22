# Wenshushu Action implementation receipt

## Implemented

The independent [plugin source](https://github.com/UniClipboard/unic-plugin-wenshushu/tree/fc968e8efd8e4ac204194c21c2c08e34e9c26e7e)
implements `unic.wenshushu/share` for text, PNG images and file selections.
Generic host changes implement Action source readers, binary RPC streams,
per-Action deadlines up to five minutes and opt-in caller cancellation.

## Executed evidence

| Boundary | Result |
| --- | --- |
| Host full `go test -race ./...`, `TMPDIR=/private/tmp` | Passed |
| Host real-process Action source E2E | Text/image/multipart/empty, dedup, timeout, cancellation and recovery passed |
| Independent plugin `GOWORK=off` race/process E2E | Passed |
| Live anonymous provider round trip | Text, valid PNG and 2.4 MB binary downloaded and compared byte for byte |
| Provider expiry | Approximately 24 hours observed |
| Immutable remote plugin install with `--build` | Passed after global PAX handling fix |
| Installed production plugin through actual CLI/service | Synthetic text upload and result history passed |
| GitHub plugin CI | [Run 44970382377 passed](https://github.com/UniClipboard/unic-plugin-wenshushu/actions/runs/44970382377) |
| Linux amd64 / Windows amd64 plugin cross-build | Passed; runtime not run |
| Native GUI/system clipboard application | Not run |
| Packaged plugin release | Not published |

The independent [acceptance receipt](https://github.com/UniClipboard/unic-plugin-wenshushu/blob/fc968e8efd8e4ac204194c21c2c08e34e9c26e7e/ACCEPTANCE.md)
records payload hashes and the provider failure investigations.

## Installation defect and regression

The actual GitHub source archive begins with a global PAX metadata header.
The extractor incorrectly selected that header as the repository root and
rejected the following archive. Global PAX metadata now affects neither root
selection nor filesystem creation. Materialized paths retain their existing
checks. Metadata-only source archives fail validation.

The real multi-process GitHub installation E2E now emits global PAX metadata.
It verifies source/prebuilt install, corrupt asset refusal, rate-limit handling
and rollback. Its isolated profile completes migration from the existing
synthetic v1 fixture. It does not probe or import the developer's real history.

Replay:

```sh
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_ARTIFACTS="$PWD/.cache/wenshushu-install" \
  go test -race ./e2e/cli -run '^TestGitHubPluginInstallation$' -count=1 -v
TMPDIR=/private/tmp UNIC_ACTION_RECEIPTS="$PWD/.cache/wenshushu-action" \
  go test -race ./e2e/actioncontent -count=1 -v
```

These commands generate JSON receipts. The installation receipt includes
`global_pax_metadata: true`, immutable fixture identities and request count.

## First failures

Preserved failures cover symlinked macOS cache paths, Unix socket path length,
provider admission/signing/batch mistakes and the global PAX archive defect.
Canonical short temporary directories resolved the platform test-path failures.
Provider fixes were followed by successful independent downloads. The PAX fix
was followed by both real GitHub installation and deterministic installation
E2E. No unresolved failure remains in the changed paths.

All upload probes used synthetic content. Telemetry and sync were disabled in
the installed-process profile. Real user clipboard history was not uploaded.
Private logs, archives, source fixtures and share URLs are excluded from Git.

## PR review follow-up

All supplied findings were checked against current code and reproduced where
behavior was affected. No finding was dismissed. The user approved the new
nested `action.close_part` RPC before its implementation; protocol version stays 1.

- Desktop Action jobs receive 330 seconds; ordinary mutations retain 15 seconds.
  A local-socket Desktop-worker E2E completes a 16-second Action and observes
  the actual request budget and the separate ordinary-mutation budget.
- Go input requirements consistently allow text/image/files; Lua remains text-only.
- Source-open/read failures and short EOF latch invocation failure even when an
  Actor swallows errors. Genuine source failure cannot commit a success item.
- Early part close releases its host reader and slot before acknowledging the
  close RPC. Sixteen partially read large parts complete in one Action. Process
  identity remains stable across early close and cancellation of a blocked stream.
- Both descriptor-returning source methods share connection-level serialization
  through `Expect`. A two-method framed-protocol E2E preserves ordered
  registration without changing stream-ID replay checks.
- Cancellation waits for an explicit process admission marker and asserts the
  cached `cancelled` error. The 75ms declared deadline asserts `timeout`.
  RPC timeout errors retain that code even when the wire budget expires before
  the Engine timer.
- E2E error checks, intentional cleanup discards and project import groups are explicit.

Validation: full race suite passed; Action-source and framed-protocol E2E passed
three consecutive runs; pinned golangci-lint 2.15.0 reported zero issues and its
format diff was empty. First failure artifacts remain in ignored
`.cache/review-fixes/`. Native GUI and system clipboard interaction were not run.

Replay:

```sh
TMPDIR=/private/tmp UNIC_ACTION_RECEIPTS="$PWD/.cache/review-replay" \
  go test -race ./e2e/actioncontent ./e2e/rpcstreams -count=3 -v
TMPDIR=/private/tmp go test -race ./app/desktop/internal/desktopui \
  -run '^TestActionWorkerBudgetE2E$' -count=1 -v
```
