# Real-machine CLI harness

Status: **failed**. Run: `b922ef3a29a476175afed1f03965931d`.

Nodes: 3. Distinct operating-system machine identities: 3. Shared-host control: false.

Source HEAD: `b12cc742bc309d7563b04587c0b827b48aa7052e`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Nodes

| Node | Execution | Target | Initial free bytes |
|---|---|---|---:|
| mac | local | darwin/arm64 | 138656673792 |
| vps | ssh | linux/amd64 | 2686349312 |
| windows | ssh | windows/amd64 | 62272208896 |

Verified measurement rows: 20 / 60. Planned payload upper bound: 12632784 bytes, excluding setup and transport overhead.

## Checks

- PASS: node connectivity, unique-machine policy, private workspace, free space and artifact hashes
- PASS: contract: no-target local commit, same-key result/query and conflicting-input rejection
- PASS: contract: matching fingerprints, no pre-approval delivery and mutual explicit approval
- PASS: contract: four concurrent production CLI retries deliver once; receiver annotations remain local
- PASS: contract: sensitive processor preserves local history/tags/expiry and blocks sending
- PASS: contract: global disable and text filter preserve local commit; enable does not replay blocked content
- PASS: pairwise-mac-vps: every expected receiver exported matching parts exactly once
- PASS: pairwise-mac-windows: every expected receiver exported matching parts exactly once

Failure: windows: wait: receive_timeout

Target failure: vps → windows, text, 16 bytes: windows: wait: receive_timeout

## Receive latency

Receive estimates include submitting-client startup, IPC, commit, transport and receiver storage. HTML uses the production local API injector. They exclude controller-side SSH, export and hash verification round trips. `verified_ms` separately includes those control costs. Native clock exchanges bracket each payload cell. Per-sample bounds include both node clock uncertainties, timestamp quantization and observed calibration drift. Bounds are clock bounds, not statistical confidence intervals. A bound reaching zero cannot establish a precise one-way latency. No network path is inferred from the SSH route.

| Scenario | Direction | Type | Nominal bytes / PNG edge px | n | Actual bytes | Receive median ms | Receive min–max ms | Max clock bound ±ms | Verified median ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| pairwise-mac-vps | mac → vps | file | 0 | 1 | 0 | 107.42 | 107.42–107.42 | 71.14 | 218.92 |
| pairwise-mac-vps | mac → vps | file | 1024 | 1 | 1024 | 125.07 | 125.07–125.07 | 72.20 | 235.17 |
| pairwise-mac-vps | mac → vps | html | 1024 | 1 | 2092 | 129.69 | 129.69–129.69 | 71.42 | 226.09 |
| pairwise-mac-vps | mac → vps | png | 16 | 1 | 854 | 107.05 | 107.05–107.05 | 71.53 | 245.62 |
| pairwise-mac-vps | mac → vps | text | 16 | 1 | 16 | 102.38 | 102.38–102.38 | 71.91 | 228.93 |
| pairwise-mac-vps | vps → mac | file | 0 | 1 | 0 | 100.86 | 100.86–100.86 | 71.14 | 198.99 |
| pairwise-mac-vps | vps → mac | file | 1024 | 1 | 1024 | 102.23 | 102.23–102.23 | 72.20 | 260.04 |
| pairwise-mac-vps | vps → mac | html | 1024 | 1 | 2092 | 91.97 | 91.97–91.97 | 71.42 | 204.23 |
| pairwise-mac-vps | vps → mac | png | 16 | 1 | 854 | 144.24 | 144.24–144.24 | 71.53 | 250.37 |
| pairwise-mac-vps | vps → mac | text | 16 | 1 | 16 | 96.68 | 96.68–96.68 | 71.91 | 239.33 |
| pairwise-mac-windows | mac → windows | file | 0 | 1 | 0 | 31.82 | 31.82–31.82 | 3.55 | 71.36 |
| pairwise-mac-windows | mac → windows | file | 1024 | 1 | 1024 | 37.38 | 37.38–37.38 | 2.90 | 72.04 |
| pairwise-mac-windows | mac → windows | html | 1024 | 1 | 2092 | 26.78 | 26.78–26.78 | 2.88 | 121.93 |
| pairwise-mac-windows | mac → windows | png | 16 | 1 | 854 | 29.27 | 29.27–29.27 | 3.02 | 68.42 |
| pairwise-mac-windows | mac → windows | text | 16 | 1 | 16 | 13.38 | 13.38–13.38 | 3.28 | 291.11 |
| pairwise-mac-windows | windows → mac | file | 0 | 1 | 0 | 34.90 | 34.90–34.90 | 3.55 | 57.40 |
| pairwise-mac-windows | windows → mac | file | 1024 | 1 | 1024 | 38.07 | 38.07–38.07 | 2.90 | 59.85 |
| pairwise-mac-windows | windows → mac | html | 1024 | 1 | 2092 | 10.08 | 10.08–10.08 | 2.88 | 41.05 |
| pairwise-mac-windows | windows → mac | png | 16 | 1 | 854 | 23.38 | 23.38–23.38 | 3.02 | 59.57 |
| pairwise-mac-windows | windows → mac | text | 16 | 1 | 16 | 25.20 | 25.20–25.20 | 3.28 | 55.69 |

## Limits

- System clipboard, native rendering, sleep/wake and network switching are not exercised.
- HTML submission uses a test injector through the production local API; other content uses the production CLI.
- Distinct OS machine identities do not attest bare-metal hardware or exclude virtual machines.
- Network paths remain unknown unless observed independently; SSH reachability does not prove a direct synchronization path.
- Negative checks use bounded observation windows. Stops use the observed service generation; Unix SIGTERM or Windows termination are fallbacks. Forced stops are recorded.
- Three samples do not support a reliable tail-latency percentile. Inspect all raw samples and bounds.
