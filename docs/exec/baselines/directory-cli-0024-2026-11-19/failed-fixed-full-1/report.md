# Real-machine CLI harness

Status: **failed**. Run: `46f51d081f7ee283a8d962f86ebbc7ac`.

Keep private artifacts: false. Controller private directory removed: true. Cleanup errors: 0.

Nodes: 2. Distinct operating-system machine identities: 2. Shared-host control: false.

Source HEAD: `4e3cf4d4b81dc103c59e5024f2dc9031c224f452`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Nodes

| Node | Execution | Target | Initial free bytes |
|---|---|---|---:|
| linux | ssh | linux/amd64 | 29563351040 |
| mac | local | darwin/arm64 | 156091912192 |

Verified measurement rows: 4 / 6. Planned payload upper bound: 33556480 bytes, excluding setup and transport overhead.

## Checks

- PASS: node connectivity, unique-machine policy, private workspace, free space and artifact hashes

Failure: linux: submit: directory_operation_exceeded_180s_budget

Target failure: linux → mac, directory, 16777216 bytes: linux: submit: directory_operation_exceeded_180s_budget

## Receive latency

Receive estimates include submitting-client startup, IPC, commit, transport and receiver storage. HTML uses the production local API injector. They exclude controller-side SSH, export and hash verification round trips. `verified_ms` separately includes those control costs. Native clock exchanges bracket each payload cell. Per-sample bounds include both node clock uncertainties, timestamp quantization and observed calibration drift. Bounds are clock bounds, not statistical confidence intervals. A bound reaching zero cannot establish a precise one-way latency. No network path is inferred from the SSH route.

| Scenario | Direction | Type | Nominal bytes / PNG edge px | n | Actual bytes | Receive median ms | Receive min–max ms | Max clock bound ±ms | Verified median ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| pairwise-linux-mac | linux → mac | directory | 0 | 1 | 226 | 215.96 | 215.96–215.96 | 123.41 | 588.40 |
| pairwise-linux-mac | linux → mac | directory | 1024 | 1 | 1048576 | 18837.86 | 18837.86–18837.86 | 118.76 | 36841.85 |
| pairwise-linux-mac | mac → linux | directory | 0 | 1 | 226 | 196.91 | 196.91–196.91 | 123.41 | 531.58 |
| pairwise-linux-mac | mac → linux | directory | 1024 | 1 | 1048576 | 19074.68 | 19074.68–19074.68 | 118.76 | 58537.47 |

## Limits

- System clipboard, native rendering, sleep/wake and network switching are not exercised.
- HTML submission uses a test injector through the production local API; other content uses the production CLI.
- Distinct OS machine identities do not attest bare-metal hardware or exclude virtual machines.
- Network paths remain unknown unless observed independently; SSH reachability does not prove a direct synchronization path.
- Negative checks use bounded observation windows. Stops use the observed service generation; Unix SIGTERM or Windows termination are fallbacks. Forced stops are recorded.
- Three samples do not support a reliable tail-latency percentile. Inspect all raw samples and bounds.
