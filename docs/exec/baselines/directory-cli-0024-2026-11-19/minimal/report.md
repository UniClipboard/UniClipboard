# Real-machine CLI harness

Status: **passed**. Run: `874935887541aaa10b1665cd3906eb9c`.

Keep private artifacts: false. Controller private directory removed: true. Cleanup errors: 0.

Nodes: 2. Distinct operating-system machine identities: 2. Shared-host control: false.

Source HEAD: `4e3cf4d4b81dc103c59e5024f2dc9031c224f452`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Nodes

| Node | Execution | Target | Initial free bytes |
|---|---|---|---:|
| linux | ssh | linux/amd64 | 29599629312 |
| mac | local | darwin/arm64 | 157779877888 |

Verified measurement rows: 2 / 2. Planned payload upper bound: 0 bytes, excluding setup and transport overhead.

## Checks

- PASS: node connectivity, unique-machine policy, private workspace, free space and artifact hashes
- PASS: pairwise-linux-mac: every expected receiver exported matching parts exactly once

## Receive latency

Receive estimates include submitting-client startup, IPC, commit, transport and receiver storage. HTML uses the production local API injector. They exclude controller-side SSH, export and hash verification round trips. `verified_ms` separately includes those control costs. Native clock exchanges bracket each payload cell. Per-sample bounds include both node clock uncertainties, timestamp quantization and observed calibration drift. Bounds are clock bounds, not statistical confidence intervals. A bound reaching zero cannot establish a precise one-way latency. No network path is inferred from the SSH route.

| Scenario | Direction | Type | Nominal bytes / PNG edge px | n | Actual bytes | Receive median ms | Receive min–max ms | Max clock bound ±ms | Verified median ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| pairwise-linux-mac | linux → mac | directory | 0 | 1 | 226 | 210.93 | 210.93–210.93 | 118.98 | 425.53 |
| pairwise-linux-mac | mac → linux | directory | 0 | 1 | 226 | 201.78 | 201.78–201.78 | 118.98 | 477.87 |

## Limits

- System clipboard, native rendering, sleep/wake and network switching are not exercised.
- HTML submission uses a test injector through the production local API; other content uses the production CLI.
- Distinct OS machine identities do not attest bare-metal hardware or exclude virtual machines.
- Network paths remain unknown unless observed independently; SSH reachability does not prove a direct synchronization path.
- Negative checks use bounded observation windows. Stops use the observed service generation; Unix SIGTERM or Windows termination are fallbacks. Forced stops are recorded.
- Three samples do not support a reliable tail-latency percentile. Inspect all raw samples and bounds.
