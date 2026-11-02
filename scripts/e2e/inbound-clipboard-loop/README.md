# Native clipboard loop regression

This runner pairs two real `uniclipd` processes through the production invitation service. Each process uses an independent native named NSPasteboard, portable development profile, and encrypted history. The existing injection library redirects only these owned processes. The user's general clipboard and system key store are not used.

This is isolated native process integration. It covers real AppKit, polling watchers, Engine, transport, history and file materialization. It does not replace product E2E on two independent desktops with their general pasteboards. Shared virtual-machine clipboard services and Universal Clipboard must be excluded in that separate acceptance environment.

## Run

From the Desktop repository root on macOS with Python 3.11+ and the Apple command-line tools:

```bash
cargo build --locked -p uc-daemon --bin uniclipd
mkdir -p target/clipboard-loop-tools
clang -fobjc-arc -framework AppKit -DPRIVATE_PASTEBOARD_TOOL scripts/e2e/clipboard-startup/private-pasteboard.m -o target/clipboard-loop-tools/private-pasteboard
clang -fobjc-arc -dynamiclib -framework AppKit scripts/e2e/clipboard-startup/private-pasteboard.m -o target/clipboard-loop-tools/private-pasteboard.dylib
python3 scripts/e2e/inbound-clipboard-loop/run.py --daemon target/debug/uniclipd --tools target/clipboard-loop-tools --artifacts target/clipboard-loop-evidence --recopy
```

Network access to the production invitation service is required. Pairing uses the same initialize, issue-invitation, redeem and durable-admission polling endpoints as the Go CLI. A missing pairing prerequisite is `BLOCKED`, exits nonzero, and never becomes a successful test.

## Assertions

The four base fixtures are synthetic text, a PNG image, one file, and multiple files. Each fixture has three scenarios with `--recopy`: A to B, deliberate identical-content native recopy on B, and a distinct local text copy on B to A. Twelve scenarios must pass. The recopy assertion allows history entry reuse; it requires a new OS ownership version, an actual watcher/Engine cycle, and completed peer delivery.

For a base receive, the runner verifies native content digests (canonical RGB pixels for images and every file's bytes), actual history and remote source, the receiver's target native ownership observation, consumption of the queued snapshot, completion of the synchronous Engine processing cycle, and no local capture or return dispatch. Source acceptance is a positive assertion only. An attempted return dispatch fails even when accepted=0. Legal active-state broadcasts are not treated as local recapture.

The host completion receipt relies on the pinned Engine awaiting `process_change` before polling `HostClipboardChangeStream::next` again. It is not a fence for arbitrary detached work. Microsecond cycle boundaries prevent ambiguity between millisecond host-log timestamps and Engine capture timestamps. The runner additionally checks permitted outbound entry identities through process shutdown and verifies final history. Any missing target receipt or detected observation error fails. This is bounded scenario evidence, not proof about all future events or adversarial concurrency.

The runner compares decrypted text history to the native text digest and scans database/WAL bytes for the exact synthetic text canaries. The UI `isEncrypted` projection is not a storage-encryption contract. The canary scan is not a complete AEAD, storage or privacy audit.

## Original-defect reproduction

Build receiver source `9e41d15f7ecf2be4e3bd03327625fd2b4adbbbd5` in a separate clean worktree. Keep the sender and tools fixed:

```bash
python3 scripts/e2e/inbound-clipboard-loop/run.py --daemon target/debug/uniclipd --receiver-daemon /absolute/path/to/old-uniclipd --receiver-source-sha 9e41d15f7ecf2be4e3bd03327625fd2b4adbbbd5 --tools target/clipboard-loop-tools --artifacts target/clipboard-loop-baseline --expect-echo
```

This positive reproduction is text-only. It requires the old receiver to initiate `dispatch_capture` for an entry whose source is local after receiving the synthetic content. It reports `EXPECTED_FAILURE_REPRODUCED`; it does not count the old version as passing the no-loop scenario. A missing new diagnostic field alone does not reproduce the defect.

## Evidence and cleanup

Each invocation writes a unique run directory with `manifest.json`, `assertions.json`, `commands.json`, `events.jsonl` and `SHA256SUMS`. Receipts include source SHA/dirty state, Engine pin, binary/tool hashes, OS/architecture, profiles, typed results, content digests for synthetic fixtures, observation identities, and child exit status. A receiver with a different binary requires its own declared source SHA.

Only allowlisted diagnostic fields are exported. Credentials, invitation codes, HTTP bodies, clipboard text, paths, previews, raw logs, databases and key material are not exported. Runtime profiles, private raw logs, and synthetic fixture files live in an owned temporary directory and are removed after children stop. Forced termination or cleanup errors fail the run. Native tools check AppKit return values and inspect without clearing the board.

The `clipboard-loop` job in `pr-check.yml` routes relevant Rust, Engine pin, native helper, runner and CI changes to `clipboard-loop-e2e.yml`. That reusable workflow can also be dispatched manually. It uploads completed and failed receipts. No UI skin, capability probe, or successful test collection is counted as native execution.

Unexecuted acceptance must remain explicit: two-desktop general pasteboards, real screenshot-application behavior, Windows/Linux runtime, and adversarial concurrent clipboard writes.
