# Real-machine CLI harness

Status: **passed**. Run: `78710219616790c1e27f111665e7655f`.

Keep private artifacts: false. Controller private directory removed: true. Cleanup errors: 0.

Nodes: 2. Distinct operating-system machine identities: 2. Shared-host control: false.

Source HEAD: `4e3cf4d4b81dc103c59e5024f2dc9031c224f452`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Nodes

| Node | Execution | Target | Initial free bytes |
|---|---|---|---:|
| linux | ssh | linux/amd64 | 29562859520 |
| mac | local | darwin/arm64 | 153045655552 |

Verified measurement rows: 0 / 0. Planned payload upper bound: 0 bytes, excluding setup and transport overhead.

## Checks

- PASS: node connectivity, unique-machine policy, private workspace, free space and artifact hashes
- PASS: directory: real-host bounds, offline recovery, cancellation, receive interruption, revocation and user export ownership

## Directory checks

| Node | Case | Result | Measured ms | Detail |
|---|---|---|---:|---|
| linux | cancel-scanning | PASS | 103.93 |  |
| linux | cancel-preparing | PASS | 97.73 |  |
| linux | mutate-preparing | PASS | 0.00 | source_changed |
| linux | respond | PASS | 50.94 |  |
| linux | depth | PASS | 0.00 | limit_exceeded |
| linux | entries | PASS | 0.00 | limit_exceeded |
| linux | files | PASS | 0.00 | limit_exceeded |
| linux | symlink | PASS | 0.00 | unsupported |
| linux | remove-source | PASS | 0.00 | cli_rejected_before_admission |
| linux | file-size | PASS | 0.00 | limit_exceeded |
| linux | offline-restart-no-replay | PASS | 0.00 | fresh explicit send required after reconnect |
| mac | export-ownership-delete-restart | PASS | 0.00 | root collision rejected; user copies survived history deletion and service restart |
| linux | cancel | PASS | 7938.89 | payload observed in receiver staging; no complete receipt; staging released |
| linux | receiver-stop | PASS | 5046.18 | payload observed in receiver staging; no complete receipt; staging released |
| linux | revoke | PASS | 3505.61 | payload observed in receiver staging; no complete receipt; staging released |
| linux | sender-crash | PASS | 32487.11 | payload observed in receiver staging; no complete receipt; staging released |
| mac | cancel-scanning | PASS | 103.99 |  |
| mac | cancel-preparing | PASS | 27.06 |  |
| mac | mutate-preparing | PASS | 0.00 | source_changed |
| mac | respond | PASS | 368.98 |  |
| mac | depth | PASS | 0.00 | limit_exceeded |
| mac | entries | PASS | 0.00 | limit_exceeded |
| mac | files | PASS | 0.00 | limit_exceeded |
| mac | symlink | PASS | 0.00 | unsupported |
| mac | remove-source | PASS | 0.00 | cli_rejected_before_admission |
| mac | file-size | PASS | 0.00 | limit_exceeded |
| mac | offline-restart-no-replay | PASS | 0.00 | fresh explicit send required after reconnect |
| linux | export-ownership-delete-restart | PASS | 0.00 | root collision rejected; user copies survived history deletion and service restart |
| mac | cancel | PASS | 2433.56 | payload observed in receiver staging; no complete receipt; staging released |
| mac | receiver-stop | PASS | 6623.09 | payload observed in receiver staging; no complete receipt; staging released |
| mac | revoke | PASS | 2611.54 | payload observed in receiver staging; no complete receipt; staging released |
| mac | sender-crash | PASS | 33030.88 | payload observed in receiver staging; no complete receipt; staging released |

See target outcomes for full independent expected/received tree inventories and sampled resources. RSS is sampled, not an allocator high-water mark. Open staging/fixture descriptors are measured on Linux only; -1 means unavailable. Submit and verified times include CLI/control costs. Pure network transfer and exact scan/preparation durations are not inferred from combined timers.

## Receive latency

Receive estimates include submitting-client startup, IPC, commit, transport and receiver storage. HTML uses the production local API injector. They exclude controller-side SSH, export and hash verification round trips. `verified_ms` separately includes those control costs. Native clock exchanges bracket each payload cell. Per-sample bounds include both node clock uncertainties, timestamp quantization and observed calibration drift. Bounds are clock bounds, not statistical confidence intervals. A bound reaching zero cannot establish a precise one-way latency. No network path is inferred from the SSH route.

| Scenario | Direction | Type | Nominal bytes / PNG edge px | n | Actual bytes | Receive median ms | Receive min–max ms | Max clock bound ±ms | Verified median ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|

## Limits

- System clipboard, native rendering, sleep/wake and network switching are not exercised.
- HTML submission uses a test injector through the production local API; other content uses the production CLI.
- Distinct OS machine identities do not attest bare-metal hardware or exclude virtual machines.
- Network paths remain unknown unless observed independently; SSH reachability does not prove a direct synchronization path.
- Negative checks use bounded observation windows. Stops use the observed service generation; Unix SIGTERM or Windows termination are fallbacks. Forced stops are recorded.
- Three samples do not support a reliable tail-latency percentile. Inspect all raw samples and bounds.
