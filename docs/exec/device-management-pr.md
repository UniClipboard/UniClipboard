# Device management draft PR checkpoint

Date: 2026-11-22.
Branch: `feat/sync-plugin-workflow`.
Target: `v2`.

## Scope

The draft includes the external sync plugin workflow and the unified device
management presentation. Device management distinguishes trusted devices,
pending paths and external channels. It exposes sync-channel runtime status,
device rename, revision-checked removal and per-path controls. The local service
projects device fingerprints, custom names, reported names and revisions.

Behavior is documented in the [Desktop runtime architecture](../architecture/desktop-runtime.md),
[identity and trust architecture](../architecture/identity-and-trust.md), and
[local service contract](../contracts/local-service-contract.md).

## Current nonvisual verification

- `make headless-e2e`: PASS. `TestProductionHeadless` passed with real production
  binaries and two service processes. The manifest is
  `.cache/0015-acceptance/manifest.json`. The subprocess helper and separately
  gated migration suites were skipped as designed. This result does not cover
  those migration suites.
- `TMPDIR=/private/tmp go test -race ./app/service/internal/host -run
  '^TestPeer(RenameEndToEnd|UnpairCASEndToEnd|UnpairWithoutRevisionStillWorks|ClientUnixSocketWithRealHost)$'
  -count=1 -v`: PASS. All four cases passed, including real Engine instances and
  the Unix-socket client flow.
- `go build -o /private/tmp/unic-pr-desktop ./app/desktop/cmd/unic-desktop`:
  PASS. The application was not launched.
- `git diff --check` and `gofmt -l` for changed Go files: PASS.
- `GOTOOLCHAIN=go1.27.3 go run
  github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.15.0 run ./...`:
  PASS, zero issues. The initial lint run found QF1003 in the channel status
  presentation. Replacing the conditional chain with a tagged switch resolved
  it. The desktop build passed again after that change. The default installed
  lint executable could not read the repository configuration; an initial
  versioned invocation also failed to download its automatically selected Go
  toolchain. The passing invocation uses the repository's existing Go 1.27.3.
- `go run ./tools/docscheck`: PASS with five stale-page warnings. The Desktop
  runtime, identity and trust, and headless architecture verification markers
  were refreshed against implementation commit `e93c850d86a7d499aaecda877c0f6965c65eec44`.
  The remaining warnings concern observability, overview, pipeline, real-machine
  testing and releases; this checkpoint does not certify those whole pages.

## Acceptance limits

The earlier unexplained Tailcat failure and the merge hold remain recorded in
the [sync workflow checkpoint](sync-plugin-workflow.md#validation-hold).
The user authorized a draft PR while retaining that hold.
This checkpoint does not claim a new full race-suite run, native device-page
acceptance, OS clipboard acceptance, cross-application paste or user visual
approval. Earlier workflow evidence remains historical evidence for its recorded
source identity.
