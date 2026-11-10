# Real-machine CLI harness acceptance

Date: 2026-11-10 (Asia/Shanghai). Implementation: `e2e/real`. Starting repository HEAD: `b12cc742bc309d7563b04587c0b827b48aa7052e`, branch `v2`. The working tree was not committed. Each run records its complete frozen source inventory and deployed binary hashes. Those hashes identify the tested inputs.

## Delivered behavior

A single controller uses a strict TOML inventory to deploy native workers to any configured local/SSH node set. Remote hosts need no compiler or Python. Workers own private profiles and run production `unic`/`unicd`. Existing OpenSSH configuration supplies credentials, ports and jump hosts. Add another node table to add a machine. No scenario source edit is required.

The suite provides isolated pairwise baselines, all-node fanout, pipeline contracts, persistent identity/trust/history checks, cooperative offline/recovery checks and relationship revocation. Text, valid PNG, zero-byte and larger files use production CLI submission. HTML plus plain text uses a production-local-API injector because the CLI has no HTML submission flag. Every receiving target exports through the production CLI and verifies SHA256, size, representation count and MIME type.

Read the [operator guide](../../e2e/real/README.md) and [architecture](../architecture/real-machine-testing.md). The earlier [0004 macOS/Windows receipt](baselines/0004-2026-11-10-cli-twohost.md) remains separate historical evidence, including successful 100 MiB LAN transfers.

## Machines and primary result

The controller ran on macOS arm64. Three independent OS installations participated: local macOS arm64, Windows amd64 on the LAN, and Linux amd64 on a US VPS. The operator supplied the two SSH aliases. The public receipts omit their addresses, SSH connection details, raw machine identifiers, keys and test payloads. Run-salted machine hashes distinguish the three OS identities. This does not attest bare-metal hardware.

[Full three-machine acceptance report](baselines/real-machine-harness-2026-11-10/three-acceptance-1/report.md): **passed, 336/336 measured receiver rows**. The run lasted from 2026-11-10 01:47:18 to 02:08:38 local time. It used two repetitions per cell.

| Content | Catalog | Verified receiver rows |
|---|---|---:|
| Text | 1 B, 1 KiB, 64 KiB, 1 MiB | 96 |
| PNG | 16, 256, 1024 pixel square edges; actual files 854–3,148,214 B | 72 |
| Files | 0 B, 1 KiB, 1 MiB, 16 MiB | 96 |
| HTML + plain text | Plain component 1 KiB, 64 KiB, 1 MiB | 72 |

Each of the three isolated pairs contributed 56 rows. The all-node mesh contributed 168 rows. All expected exported content hashes matched. All contract and isolation checks passed. Every worker removed its profile. No service required forced termination. The frozen source remained unchanged. The known-secret check covers captured daemon stdout/stderr; persistent structured logs remain private and were not scanned for this acceptance.

The full run predates the final ordered EOF handling, configurable wait limit and per-target outcome enhancements. Separate final-source runs exercise those changes and retain failures independently. Product input hashes under `engine/`, `internal/`, `plugin/`, `plugins/` and `app/service/` are identical across the full run and final smoke snapshot. Do not treat different harness snapshots as one run.

## Timing interpretation

The report includes every raw sample and clock bound. Receive time includes CLI startup, local IPC, processors, commit, transfer and receiver storage. HTML includes the local API injector instead of CLI startup. Controller-observed verification time also includes SSH, polling, export and hash verification.

Across the isolated pair cells, the largest clock uncertainty was approximately ±3.74 ms on macOS/Windows, ±71.94 ms on macOS/Linux and ±73.94 ms on Windows/Linux. These are clock bounds, not statistical confidence intervals. Two repetitions cannot establish a tail percentile. SSH reachability does not prove a direct unic path. The path remains `unknown`.

## Other acceptance and retained failures

| Run | Result and interpretation |
|---|---|
| [Three-node doctor](baselines/real-machine-harness-2026-11-10/doctor-2/report.md) | Passed native workers, unique-machine checks, deployment hashes and cleanup. |
| [Duplicate host rejection](baselines/real-machine-harness-2026-11-10/duplicate-rejected/report.md) | Expected failure: two aliases resolved to the same OS identity. Services were not started. |
| [Initial smoke](baselines/real-machine-harness-2026-11-10/run-smoke-1/report.md) | All 60 timing rows completed, but isolation failed after abrupt Windows termination. The harness incorrectly used forced termination for normal stop. It now calls the production stop API with the observed PID/generation and records fallbacks. This failed run remains failed. |
| [Two-host isolation](baselines/real-machine-harness-2026-11-10/isolation-two-1/report.md) | Passed after the cooperative-stop correction. No forced stops. |
| [100 MiB WAN expansion](baselines/real-machine-harness-2026-11-10/three-large-1/report.md) | Failed: the VPS did not complete the first Mac fanout within 180 seconds. This is not a successful 100 MiB WAN baseline or proof of a permanent size limit. This older harness did not retain partial target results. |
| [First final-source smoke](baselines/real-machine-harness-2026-11-10/final-three-smoke/report.md) | Failed: a 42 B mail fixture timed out at the VPS after mutual approval. Read-only database inspection found local sent/queued state at the sender and no receiver history row. Cause remains unresolved. The independent next run passed this mail case, but later failed another transfer. The original failure remains unresolved. |
| [Second final-source smoke](baselines/real-machine-harness-2026-11-10/final-three-smoke-2/report.md) | Failed after 20 successful measurement rows: Linux→Windows 16 B text did not arrive within 180 seconds. All contracts passed first. The failed target outcome preserves its expected hash and observed wait duration. No forced stops or cleanup errors occurred. |
| [Five-second WAN deadline](baselines/real-machine-harness-2026-11-10/three-timeout-1/report.md) | Expected failure on the final controller: Mac→Windows exported matching 16 MiB bytes in 2310 ms; Mac→Linux timed out in 5156 ms. Both outcomes were retained, zero complete timing rows were emitted, and the entire fanout failed. All three services stopped without force or cleanup errors. This proves partial success is retained without converting incomplete fanout to success. |
| Process acceptance | Passed under `go test -race`: a configured one-second worker receive wait failed in 1002 ms; actual native worker/service EOF cleanup verified PID exit, socket unavailability and profile removal; duplicate-host rejection retained a failed receipt; an offline four-node plan produced six pairs and twelve directions. [Machine-readable receipt](baselines/real-machine-harness-2026-11-10/process-acceptance-final/manifest.json.gz). |
| Existing headless regression | Passed under `-race`, including production CLI commit/export, explicit pairing, text/PNG/files, adapter isolation, restart and persistent history. [Machine-readable receipt](baselines/real-machine-harness-2026-11-10/headless-regression/manifest.json.gz). |

Failures stop their owned services but intentionally retain private profiles for inspection. Private failed-run profiles may contain test identity keys. They are not included in these published receipts. The implementation never retries a failed measured case to convert its result to success.

## Reproduce and verify

```sh
go build -o .build/unic-e2e ./e2e/real
cp e2e/real/lab.example.toml e2e/real/lab.local.toml
# Edit node tables to use your local host and SSH aliases.
.build/unic-e2e plan --lab e2e/real/lab.local.toml
.build/unic-e2e doctor --lab e2e/real/lab.local.toml
.build/unic-e2e run --lab e2e/real/lab.local.toml --suite sync
```

For the primary 336-row catalog, set two repetitions and remove 100 MiB from the example file sizes. For a short 60-row run, use one repetition, text `[16]`, PNG `[16]`, files `[0, 1024]` and HTML `[1024]`. The manifest records every effective suite value. `receive_timeout_seconds` accepts 1–900 seconds and defaults to 180. Each target timeout fails the fanout. Completed targets remain in `outcomes.jsonl` even when another target fails.

Published machine manifests use deterministic gzip compression. They contain source inventories, including historical filenames, rather than additional living documentation pages. Public raw measurements and reports remain uncompressed. Each receipt directory has `SHA256SUMS` for the published bytes. Check a receipt and inspect its full manifest with:

```sh
cd docs/exec/baselines/real-machine-harness-2026-11-10/three-acceptance-1
shasum -a 256 -c SHA256SUMS
gzip -dc manifest.json.gz > /tmp/unic-real-manifest.json
```

The public artifacts preserve hashes and measurements, not source bundles or binaries. Repeating the run creates a new source/artifact-bound receipt; it does not promise identical network latency.

## Verification boundaries

No unit tests were added. The new automated harness test crosses compiled process, stdio, production CLI and local-service boundaries. Three-host execution validates macOS, Windows and Linux workers with a macOS controller. Four or more machines are supported by inventory expansion but were not physically executed. Windows/Linux controllers require separate native acceptance.

Native clipboard, desktop UI/rendering, mobile hosts, concurrent senders, other pairing graphs, abrupt power loss, sleep/wake and network switching were not exercised. No GUI was launched. The unresolved small-message timeout prevents a claim of stable WAN synchronization across repeated runs.

`go vet ./e2e/real` and `git diff --check` passed. `go run ./tools/docscheck` passed with seven stale-page warnings. The documentation checker excludes generated `.cache` directories, matching its other build-directory exclusions. The requested `-bump` initially refused uncommitted harness inputs. After implementation commit `bec97adea`, the pipeline and real-machine architecture markers were updated to that commit. Historical receipts continue to identify their original source snapshots.

## Automatic cleanup update

The harness now removes owned node profiles, remote bootstrap programs and controller private source/build/log directories on success and failure. Public receipts remain. `--keep-private` explicitly retains stopped profiles and private artifacts for diagnosis. Cleanup errors fail the run and preserve the controller recovery inventory. This policy applies to new runs; the historical receipts above retain their original cleanup behavior. No historical profiles or source bundles are deleted by a new run.

Forced controller termination, power loss and unreachable remote hosts can prevent confirmed cleanup. Consult `cleanup_errors`, each node's `cleanup.removed`, `keep_private` and `controller_private_removed` instead of assuming cleanup completed.

Validation: the gated process E2E passed under `-race` with seven checks. It covers default cleanup on success and duplicate-machine failure, explicit retention on both statuses, configured receive timeout, real service EOF cleanup, and four-node offline planning. Its [source-bound receipt](baselines/real-machine-harness-2026-11-10/automatic-cleanup/cleanup-process-final/manifest.json.gz) remains public.

A [three-machine failure run](baselines/real-machine-harness-2026-11-10/automatic-cleanup/cleanup-three-1/report.md) paired macOS, Windows and Linux, then intentionally used a five-second receive deadline for a 16 MiB WAN transfer. The VPS timed out. The harness retained the failed public receipt, stopped all services without force, removed all three profiles and both SSH bootstrap directories, and removed the controller private directory. An independent [post-run filesystem check](baselines/real-machine-harness-2026-11-10/automatic-cleanup/cleanup-three-1/cleanup-verification.json) verified all five observed node directories were absent. No cleanup errors occurred. This validates cleanup after failure, not successful WAN transfer.

`go vet ./e2e/real`, `git diff --check` and the documentation check passed. Documentation still reports seven stale-page warnings. The `verified` bump initially rejected uncommitted inputs and succeeded after the implementation was committed normally.
