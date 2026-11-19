# Real-machine CLI harness

Status: **failed**. Run: `af3d0084e97af059c16b4ce8c568cd67`.

Keep private artifacts: false. Controller private directory removed: true. Cleanup errors: 0.

Nodes: 2. Distinct operating-system machine identities: 2. Shared-host control: false.

Source HEAD: `4e3cf4d4b81dc103c59e5024f2dc9031c224f452`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Nodes

| Node | Execution | Target | Initial free bytes |
|---|---|---|---:|
| linux | ssh | linux/amd64 | 29591695360 |
| mac | local | darwin/arm64 | 157274161152 |

Verified measurement rows: 4 / 6. Planned payload upper bound: 33556480 bytes, excluding setup and transport overhead.

## Checks

- PASS: node connectivity, unique-machine policy, private workspace, free space and artifact hashes

Failure: linux: submit: submit_failed

## Receive latency

Receive estimates include submitting-client startup, IPC, commit, transport and receiver storage. HTML uses the production local API injector. They exclude controller-side SSH, export and hash verification round trips. `verified_ms` separately includes those control costs. Native clock exchanges bracket each payload cell. Per-sample bounds include both node clock uncertainties, timestamp quantization and observed calibration drift. Bounds are clock bounds, not statistical confidence intervals. A bound reaching zero cannot establish a precise one-way latency. No network path is inferred from the SSH route.

| Scenario | Direction | Type | Nominal bytes / PNG edge px | n | Actual bytes | Receive median ms | Receive min–max ms | Max clock bound ±ms | Verified median ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| pairwise-linux-mac | linux → mac | directory | 0 | 1 | 226 | 295.58 | 295.58–295.58 | 119.06 | 575.62 |
| pairwise-linux-mac | linux → mac | directory | 1024 | 1 | 1048576 | 20047.61 | 20047.61–20047.61 | 117.48 | 42168.62 |
| pairwise-linux-mac | mac → linux | directory | 0 | 1 | 226 | 192.05 | 192.05–192.05 | 119.06 | 513.56 |
| pairwise-linux-mac | mac → linux | directory | 1024 | 1 | 1048576 | 11741.64 | 11741.64–11741.64 | 117.48 | 51443.51 |

## Limits

- System clipboard, native rendering, sleep/wake and network switching are not exercised.
- HTML submission uses a test injector through the production local API; other content uses the production CLI.
- Distinct OS machine identities do not attest bare-metal hardware or exclude virtual machines.
- Network paths remain unknown unless observed independently; SSH reachability does not prove a direct synchronization path.
- Negative checks use bounded observation windows. Stops use the observed service generation; Unix SIGTERM or Windows termination are fallbacks. Forced stops are recorded.
- Three samples do not support a reliable tail-latency percentile. Inspect all raw samples and bounds.
