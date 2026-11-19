---
paths:
  - e2e/real
  - e2e/twohost/README.md
  - Makefile
  - tools/docscheck
verified: da729ef1dcf090e46ee76f5291e404adf42c0bce
---
# Real-machine CLI testing

The repository provides a node-configured harness for production CLI/service testing across local and SSH hosts.

## Current implementation

`e2e/real` builds one Go program with controller and native worker roles. A TOML inventory declares node labels and `local` or `ssh` transport. Operating-system roles are discovered, not encoded in scenarios. OpenSSH supplies host authentication, credentials and jump-host behavior. Remote hosts do not need Go or Python.

The controller snapshots tracked repository files and untracked production/harness inputs before building anything. Repository-internal file symlinks are materialized; external links are rejected. Workers and production programs are compiled from the private snapshot with CGO and VCS stamping explicitly disabled. The public manifest binds source hashes and actual artifact hashes. Later edits to the original checkout do not enter an active run.

The worker owns a temporary profile, private local socket and child service. It exposes a closed JSON-over-stdio control interface over the local process pipes or persistent SSH. The measured content path stays inside production unic/tailcat. Fixtures are prepared before submission timing begins. The production CLI submits text/PNG/files with `unic send --to all`, lists receives with `unic recv list`, and exports received history with `unic history export`. HTML plus its plain-text representation uses a test-only production-local-API injector. Every expected receiver must export matching part counts, MIME types, sizes and SHA256 values; dispatch admission alone is insufficient.

`doctor` checks connectivity, platform, run-salted machine identity, temporary storage and worker hashes. Aliases pointing at the same machine fail unless the operator explicitly selects the shared-host control mode, which is marked in the manifest. OS machine identities do not attest physical hardware or exclude virtualization.

`pairwise` runs each selected pair with other services stopped. `mesh` pairs all nodes and measures each sender against every other receiver. A two-node mesh also supplies its isolated pair baseline. `contract` covers commit/idempotency, explicit mutual approval, processor annotations and sync policy. `isolation` seeds history, restarts services, checks existing identity/trust/history without reapproving missing trust, checks offline recovery without backlog replay and checks revocation while surviving nodes continue syncing.

Pairing uses `unic device addr`, `device add` and `device approve`. Revocation uses `unic device remove`.
Commit lookup uses `unic dev submission`. Explicit `--to all` selects paired-device synchronization; omitting `--to` starts a temporary transfer outside this harness catalog.
The worker starts a standalone `unicd`, so CLI commands borrow its service rather than own a CLI-temporary generation.

Native clock exchanges bracket each payload cell. A remote-to-remote sample combines both node clock bounds, timestamp quantization and observed drift. Raw point estimates and bounds remain distinct from controller-monotonic export-verification time. Network route remains unknown unless independently observed.

## Invariants

- Test code never installs a service or uses a user's normal profile or system clipboard.
- Every run builds from one frozen source snapshot and verifies deployed program hashes.
- Host credentials and raw node paths remain in private artifacts, never public receipts.
- Pairing compares both fingerprints before approving either side; introductions do not bypass approval or forward content.
- A measured transfer passes only after every expected receiver exports matching content exactly once within the observation window.
- Expected measurement counts must match completed measurements before a run passes.
- Source, worker, deployment, synchronization and cleanup failures remain distinguishable failures; required hosts are not silently skipped.
- Service stop first checks its observed generation and owned PID, then uses the existing local stop API. Signal/termination fallbacks are recorded.
- The worker checks process exit and private socket unavailability before reporting a stop complete.
- Successful and failed runs remove their owned node profiles, remote bootstrap programs and controller private directory by default. Public evidence remains. `--keep-private` explicitly retains private artifacts after stopping services. Cleanup errors fail the run and preserve controller recovery inventory. Historical runs are not swept.
- Control EOF or an expired idle lease stops the owned service. Heartbeats keep idle workers alive during other nodes' work.
- A public report must include per-target outcomes, receiver timeouts, cleanup failures and timing uncertainty. No raw logs, pairing tickets, addresses or test content are copied into it. The sentinel scan covers captured daemon stdout/stderr, not persistent structured logs in the private profile.

## Code map

| Path | Responsibility |
|---|---|
| `e2e/real/main.go`, `config.go` | Commands, strict inventory, preflight, scenario selection and lifecycle |
| `e2e/real/source.go` | Snapshot isolation, target builds and source/artifact identity |
| `e2e/real/transport.go`, `protocol.go` | Local/SSH worker control, deployment and ordered responses |
| `e2e/real/worker.go` | Owned service, local API/CLI operations, exports and cleanup |
| `e2e/real/machine_*.go` | Platform-specific machine and disk probes |
| `e2e/real/fixtures.go` | Reproducible fixture bytes, valid PNG and multiple representations |
| `e2e/real/scenarios.go` | Pairing, isolated links, fanout, contracts and failure isolation |
| `e2e/real/report.go` | Public manifests, raw measurements, reports and checksums |

## Specifications and operation

See the [inventory and commands](../../e2e/real/README.md), [local service contract](../contracts/local-service-contract.md), [headless architecture](headless.md), and [pipeline architecture](pipeline.md).

The front-matter marker names the committed implementation baseline. Physical-host receipts bind the exact source hashes and programs tested by each historical run.

The documentation checker excludes generated `.cache` trees, as it excludes other build-output directories. Frozen source copies and test artifacts there are not living repository documentation.

## Gaps

Native clipboard, rendering, mobile adapters, concurrent senders, additional pairing graphs, sleep/wake and network switching remain outside this first catalog. Abrupt power loss and bounded idle crash detection are not part of cooperative service-stop acceptance. Controller operating systems require separate native acceptance even when their target workers have been verified. Actual platform/node coverage and retained failures are recorded in the [acceptance receipt](../exec/real-machine-harness.md).
