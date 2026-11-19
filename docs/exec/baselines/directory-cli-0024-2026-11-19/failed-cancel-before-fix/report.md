# Real-machine CLI harness

Status: **failed**. Run: `fe059b77cef7c38b8706c4445bb845d2`.

Keep private artifacts: false. Controller private directory removed: true. Cleanup errors: 0.

Nodes: 2. Distinct operating-system machine identities: 2. Shared-host control: false.

Source HEAD: `4e3cf4d4b81dc103c59e5024f2dc9031c224f452`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Nodes

| Node | Execution | Target | Initial free bytes |
|---|---|---|---:|
| linux | ssh | linux/amd64 | 29569105920 |
| mac | local | darwin/arm64 | 156603121664 |

Verified measurement rows: 0 / 0. Planned payload upper bound: 0 bytes, excluding setup and transport overhead.

## Checks

- PASS: node connectivity, unique-machine policy, private workspace, free space and artifact hashes

Failure: mac_directory_resources_not_released: {"rss_bytes":52396032,"open_files":-1,"open_staging_or_fixture_files":-1,"incoming_files":1,"incoming_bytes":807672,"transient_directories":0}

## Directory checks

| Node | Case | Result | Measured ms | Detail |
|---|---|---|---:|---|
| linux | cancel-scanning | PASS | 105.83 |  |
| linux | cancel-preparing | PASS | 97.54 |  |
| linux | mutate-preparing | PASS | 0.00 | source_changed |
| linux | respond | PASS | 44.50 |  |
| linux | depth | PASS | 0.00 | limit_exceeded |
| linux | entries | PASS | 0.00 | limit_exceeded |
| linux | files | PASS | 0.00 | limit_exceeded |
| linux | symlink | PASS | 0.00 | unsupported |
| linux | remove-source | PASS | 0.00 | cli_rejected_before_admission |
| linux | file-size | PASS | 0.00 | limit_exceeded |
| linux | offline-restart-no-replay | PASS | 0.00 | fresh explicit send required after reconnect |
| mac | export-ownership-delete-restart | PASS | 0.00 | root collision rejected; user copies survived history deletion and service restart |

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
