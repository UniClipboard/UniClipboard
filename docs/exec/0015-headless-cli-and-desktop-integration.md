# 0015 implementation and acceptance

## Source identity

Baseline branch: `v2`.
Baseline HEAD: `eeda052c613e53a49909d19d8ad22bfa88ec42fc`.
The workspace already contained uncommitted desktop, UI Kit and demo changes.
Those changes remain in the workspace. The GUI entry was moved with its existing content.
The initial acceptance and packaging runs preceded this implementation commit.
No tag, workflow dispatch, hosted CI run or publication was performed.
The added headless CI job uses the same pinned display-free Go image and uploads only the acceptance directory.

Production acceptance records the complete local source-file hash set before building and compares it after testing.
Both final manifests report `status=passed` and `source_unchanged=true`.
The artifact hashes identify the tested dirty-workspace build, rather than claiming the baseline commit contains this implementation.

## Implemented

- Separate `app/service/cmd/unic`, `app/service/cmd/unicd`, and `app/desktop/cmd/unic-desktop` programs.
- Shared DTOs and clients in `internal/localapi`; HTTP routes and socket listener in `app/service/internal/server`.
- Explicit text/stdin/PNG/file-reference submission, request identity, result query and bounded same-key retries.
- Transient Received listing, streamed reads and dismissal, including `persist=false` content.
- User-owned export with exclusive output creation, confined paths and partial-output cleanup.
- Action commit independent of optional system application, with retained committed results after application failure.
- One adapter generation, heartbeat expiry, baseline, command queue, lease ownership and stale-generation rejection.
- Read-back verification, capture/apply serialization, copy deduplication, reconnect baseline and backlog exclusion.
- Shared four-stream content limit, eight-second stream/write deadlines, upload and request limits.
- Desktop tray and notifications, hide-on-window-close, and a separate explicit application quit path.
- Native archive builder that reuses the same CLI/service bytes in both distributions and records source/binary/archive SHA-256 hashes.

History restore retains the newer `sync.on_restore` behavior. Action application does not trigger restore sync.
Service restart invalidates session leases. Persisted history keeps its own file ownership.
Headless file receive does not wait for a system clipboard baseline.

## Passed checks

| Check | Result |
|---|---|
| Production process E2E, macOS arm64, race-enabled | Passed; 13 acceptance groups |
| Production process E2E, Linux arm64, race-enabled | Passed; 13 acceptance groups |
| Final CLI process suite | All 16 cases passed |
| CLI/service transitive dependency inventory | No mygo, UI Kit or desktop application package |
| Linux runtime library inventory | No GTK, GDK, WebKit, X11, Wayland, GLFW or Qt library in the test image inventory |
| Changed service, server, CLI, desktop entry, shared packages and test-board race checks | Passed |
| Pinned golangci-lint v2.15.0, built with Go 1.27.3, changed packages | 0 issues |
| `go mod verify` | All modules verified |
| Native desktop entry build | Passed; no GUI launched |
| Both macOS arm64 distribution archives | Built; archive and binary checksums verified |
| Shared `unic`/`unicd` bytes across the two archives | Identical |
| Independent architecture/code review | No remaining severe finding after corrections; static review only |
| Documentation links and structure | Passed, with three pre-existing/stale-workspace warnings |

The Linux fixture uses `golang:1.27.3-bookworm`, image digest
`sha256:dc0963f32053e4bc451d24d87e3892d42d01021b61f093e98eb95c0352bb84b0`.
It has no display mount, virtual display or UI toolkit installation.
The directory-backed adapter runs as a separate process and does not access the real system clipboard.

The acceptance groups cover production CLI commands, mutual explicit pairing approval, text/PNG/file receive,
independent history and transient reads, export, source mutation, output symlinks, Action preflight,
copy echo prevention, automatic capture, adapter contention/crash/expiry, backlog exclusion,
fresh remote application, malformed report cleanup, stale generations, lease/stream limits and service restart.

## Existing failing checks

The full `go test -race ./...` run has nine failing Engine cases and one failing migration case.
All ten were reproduced from an independent `git archive HEAD` copy without this implementation:

- `TestBackendsGetIndependentViewsAndThresholds`
- `TestBackendPoliciesAreEffectiveValidatedAndHotUpdated`
- `TestHistorySyncSummaryFollowsTheOperation`
- `TestUserLimitsApplyToImport`
- `TestNewRemovesOnlyItsOwnLeftovers`
- `TestReceiveBetweenEngines`
- `TestReceiverHonoursItsOwnSettings`
- `TestReceivedFilesAreRemovedWithTheirContent`
- `TestCloseAndStartupClearTheReceiveDirectory`
- `TestRustFixtureMigrationResumeAndSourceIndependence`

The final 16-case CLI process suite passed all cases.
Earlier runs of `file-cache-scope` failed at `retry-copy alpha beta custom-ready` after a receiver restart.
The same failure was reproduced in the independent baseline copy.
A focused retry and the final complete run passed. Retain the earlier receipts as an observed baseline flake,
not as proof that this implementation repaired the network reconnect behavior.

The custom-receive expiry case now waits for its requested one-second expiry before asserting absence.
The CLI process harness now waits for adapter command availability, rather than only socket status.
The Action case explicitly requests `--apply`.

## Local artifacts and reproduction

- `.cache/0015-acceptance/manifest.json`: final macOS source/binary identities and checks.
- `.cache/0015-linux/acceptance/manifest.json`: final Linux source/binary identities and checks.
- `.cache/0015-linux-libraries.txt`: Linux runtime library inventory.
- `.cache/0015-releases/manifest-darwin-arm64.json`: source, binary and archive hashes.
- `.cache/0015-releases/unic-headless-darwin-arm64.tar.gz`: `unic` and `unicd`.
- `.cache/0015-releases/unic-desktop-darwin-arm64.tar.gz`: the same two files and `unic-desktop`.
- `.cache/0015-baseline-failures.log`, `.cache/0015-baseline-cli.log`: baseline reproduction.
- `.cache/0015-cli-final/`: final per-case source/binary identities and all 16 successful receipts.
- `.cache/0015-race.log`, `.cache/0015-focused-race.log`, `.cache/0015-cli-final.log`: local check receipts.
- `.cache/0015-cli.log`, `.cache/0015-cli-isolated.log`: earlier observed CLI reconnect failures.

```sh
make headless-e2e
make package-desktop
UNIC_E2E=1 go test -race ./e2e/cli -run '^TestCLI$' -count=1

docker run --rm \
  -v "$PWD:/work:ro" \
  -v "$PWD/.cache/0015-linux:/artifacts" \
  -v "$(go env GOMODCACHE):/go/pkg/mod:ro" \
  -w /work -e GOCACHE=/tmp/go-cache \
  -e UNIC_HEADLESS_E2E=1 -e UNIC_HEADLESS_ARTIFACTS=/artifacts/acceptance \
  golang:1.27.3-bookworm \
  go test -race ./e2e/headless -run TestProductionHeadless -v -count=1
```

Use a canonical temporary directory on macOS when running cache tests.
The managed-cache contract rejects symlinked roots.

## Remaining acceptance

Native macOS/Windows/Linux clipboard interaction and visual acceptance were not run.
No graphical application, browser or VM was launched for this task.
The archives are unsigned. Installers, service/login registration and store publication remain separate work.
Decision 0015 remains proposed until those acceptance boundaries are resolved.

The documentation `-bump` command was run and rejected uncommitted changes under its tracked source paths.
It requires a code commit before recording `verified`; committing unrelated existing UI changes was not performed.
The front-matter marker therefore remains the baseline. Current verification is tied to the source-hash manifests.

Manual native checks:

1. Start the built `unicd`, then start `unic-desktop` in a graphical login session.
2. Verify that the existing clipboard is not broadcast on startup.
3. Copy text, PNG and a file between two approved devices. Verify one receive and no echo.
4. Close the main window. Verify that capture and application continue.
5. Reopen the window from the tray and by launching the program again.
6. Use explicit Quit. Verify that the desktop process exits and headless submissions still work.
7. Restart the desktop after receiving content while offline. Verify that it does not apply the backlog.
8. Check permission-denied behavior and file use after clipboard replacement on each platform.
