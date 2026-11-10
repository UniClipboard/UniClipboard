# Real-machine CLI harness

Status: **failed**. Run: `69f29c988cc21c4b77639c37910fc30b`.

Nodes: 3. Distinct operating-system machine identities: 3. Shared-host control: false.

Source HEAD: `b12cc742bc309d7563b04587c0b827b48aa7052e`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Nodes

| Node | Execution | Target | Initial free bytes |
|---|---|---|---:|
| mac | local | darwin/arm64 | 139598811136 |
| vps | ssh | linux/amd64 | 2802225152 |
| windows | ssh | windows/amd64 | 62401527808 |

Verified measurement rows: 0 / 60. Planned payload upper bound: 12632784 bytes, excluding setup and transport overhead.

## Checks

- PASS: node connectivity, unique-machine policy, private workspace, free space and artifact hashes
- PASS: contract: no-target local commit, same-key result/query and conflicting-input rejection
- PASS: contract: matching fingerprints, no pre-approval delivery and mutual explicit approval

Failure: vps: wait: receive_timeout

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
