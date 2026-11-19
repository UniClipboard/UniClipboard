# Real-machine CLI harness

Status: **failed**. Run: `01033529c1df1156abf4d9d4df582080`.

Keep private artifacts: false. Controller private directory removed: true. Cleanup errors: 0.

Nodes: 2. Distinct operating-system machine identities: 2. Shared-host control: false.

Source HEAD: `4e3cf4d4b81dc103c59e5024f2dc9031c224f452`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Nodes

| Node | Execution | Target | Initial free bytes |
|---|---|---|---:|
| linux | ssh | linux/amd64 | 29576654848 |
| mac | local | darwin/arm64 | 151975198720 |

Verified measurement rows: 6 / 6. Planned payload upper bound: 33556480 bytes, excluding setup and transport overhead.

## Checks

- PASS: node connectivity, unique-machine policy, private workspace, free space and artifact hashes
- PASS: pairwise-linux-mac: every expected receiver exported matching parts exactly once

Failure: directory_resources_not_released

## Directory checks

| Node | Case | Result | Measured ms | Detail |
|---|---|---|---:|---|
| linux | cancel-scanning | PASS | 105.72 |  |
| linux | cancel-preparing | PASS | 87.01 |  |
| linux | mutate-preparing | PASS | 0.00 | source_changed |
| linux | respond | PASS | 53.61 |  |
| linux | depth | PASS | 0.00 | limit_exceeded |
| linux | entries | PASS | 0.00 | limit_exceeded |
| linux | files | PASS | 0.00 | limit_exceeded |
| linux | symlink | PASS | 0.00 | unsupported |
| linux | remove-source | PASS | 0.00 | cli_rejected_before_admission |
| linux | file-size | PASS | 0.00 | limit_exceeded |
| linux | offline-restart-no-replay | PASS | 0.00 | fresh explicit send required after reconnect |
| mac | export-ownership-delete-restart | PASS | 0.00 | root collision rejected; user copies survived history deletion and service restart |
| linux | cancel | PASS | 6723.69 | payload observed in receiver staging; no complete receipt; staging released |
| linux | receiver-stop | PASS | 5278.35 | payload observed in receiver staging; no complete receipt; staging released |

See target outcomes for full independent expected/received tree inventories and sampled resources. RSS is sampled, not an allocator high-water mark. Open staging/fixture descriptors are measured on Linux only; -1 means unavailable. Submit and verified times include CLI/control costs. Pure network transfer and exact scan/preparation durations are not inferred from combined timers.

## Receive latency

Receive estimates include submitting-client startup, IPC, commit, transport and receiver storage. HTML uses the production local API injector. They exclude controller-side SSH, export and hash verification round trips. `verified_ms` separately includes those control costs. Native clock exchanges bracket each payload cell. Per-sample bounds include both node clock uncertainties, timestamp quantization and observed calibration drift. Bounds are clock bounds, not statistical confidence intervals. A bound reaching zero cannot establish a precise one-way latency. No network path is inferred from the SSH route.

| Scenario | Direction | Type | Nominal bytes / PNG edge px | n | Actual bytes | Receive median ms | Receive min–max ms | Max clock bound ±ms | Verified median ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| pairwise-linux-mac | linux → mac | directory | 0 | 1 | 226 | 624.10 | 624.10–624.10 | 129.35 | 1251.22 |
| pairwise-linux-mac | linux → mac | directory | 1024 | 1 | 1048576 | 14316.36 | 14316.36–14316.36 | 117.73 | 33773.63 |
| pairwise-linux-mac | linux → mac | directory | 16777216 | 1 | 16777216 | 100900.81 | 100900.81–100900.81 | 120.23 | 101295.98 |
| pairwise-linux-mac | mac → linux | directory | 0 | 1 | 226 | 242.90 | 242.90–242.90 | 129.35 | 619.42 |
| pairwise-linux-mac | mac → linux | directory | 1024 | 1 | 1048576 | 10180.19 | 10180.19–10180.19 | 117.73 | 50417.19 |
| pairwise-linux-mac | mac → linux | directory | 16777216 | 1 | 16777216 | 73207.23 | 73207.23–73207.23 | 120.23 | 73868.12 |

## Limits

- System clipboard, native rendering, sleep/wake and network switching are not exercised.
- HTML submission uses a test injector through the production local API; other content uses the production CLI.
- Distinct OS machine identities do not attest bare-metal hardware or exclude virtual machines.
- Network paths remain unknown unless observed independently; SSH reachability does not prove a direct synchronization path.
- Negative checks use bounded observation windows. Stops use the observed service generation; Unix SIGTERM or Windows termination are fallbacks. Forced stops are recorded.
- Three samples do not support a reliable tail-latency percentile. Inspect all raw samples and bounds.
