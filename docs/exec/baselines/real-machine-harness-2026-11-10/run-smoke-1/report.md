# Real-machine CLI harness

Status: **failed**. Run: `c1af1df7b3b8531143bc78667009f2ba`.

Nodes: 3. Distinct operating-system machine identities: 3. Shared-host control: false.

Source HEAD: `b12cc742bc309d7563b04587c0b827b48aa7052e`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Checks

- PASS: node connectivity, unique-machine policy, private workspace, free space and artifact hashes
- PASS: contract: no-target local commit, same-key result/query and conflicting-input rejection
- PASS: contract: matching fingerprints, no pre-approval delivery and mutual explicit approval
- PASS: contract: four concurrent production CLI retries deliver once; receiver annotations remain local
- PASS: contract: sensitive processor preserves local history/tags/expiry and blocks sending
- PASS: contract: global disable and text filter preserve local commit; enable does not replay blocked content
- PASS: pairwise-mac-vps: every expected receiver exported matching parts exactly once
- PASS: pairwise-mac-windows: every expected receiver exported matching parts exactly once
- PASS: pairwise-vps-windows: every expected receiver exported matching parts exactly once
- PASS: mesh: every expected receiver exported matching parts exactly once
- PASS: isolation: all node identities, trust and history IDs survive service restart

Failure: timeout_offline_peer

## Receive latency

Receive estimates include submitting-client startup, IPC, commit, transport and receiver storage. HTML uses the production local API injector. They exclude controller-side SSH, export and hash verification round trips. `verified_ms` separately includes those control costs. Native clock exchanges bracket each payload cell. Per-sample bounds include both node clock uncertainties, timestamp quantization and observed calibration drift. Bounds are clock bounds, not statistical confidence intervals. A bound reaching zero cannot establish a precise one-way latency. No network path is inferred from the SSH route.

| Scenario | Direction | Type | Nominal bytes / PNG edge px | n | Actual bytes | Receive median ms | Receive min–max ms | Max clock bound ±ms | Verified median ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| mesh | mac → vps | file | 0 | 1 | 0 | 123.78 | 123.78–123.78 | 71.22 | 224.94 |
| mesh | mac → vps | file | 1024 | 1 | 1024 | 116.11 | 116.11–116.11 | 71.39 | 236.78 |
| mesh | mac → vps | html | 1024 | 1 | 2092 | 103.90 | 103.90–103.90 | 71.00 | 231.36 |
| mesh | mac → vps | png | 16 | 1 | 854 | 98.32 | 98.32–98.32 | 71.17 | 227.49 |
| mesh | mac → vps | text | 16 | 1 | 16 | 123.47 | 123.47–123.47 | 71.44 | 253.04 |
| mesh | mac → windows | file | 0 | 1 | 0 | 13.93 | 13.93–13.93 | 2.88 | 63.05 |
| mesh | mac → windows | file | 1024 | 1 | 1024 | 30.94 | 30.94–30.94 | 2.92 | 65.43 |
| mesh | mac → windows | html | 1024 | 1 | 2092 | 19.36 | 19.36–19.36 | 2.86 | 59.57 |
| mesh | mac → windows | png | 16 | 1 | 854 | 20.10 | 20.10–20.10 | 3.17 | 60.82 |
| mesh | mac → windows | text | 16 | 1 | 16 | 42.56 | 42.56–42.56 | 2.99 | 90.70 |
| mesh | vps → mac | file | 0 | 1 | 0 | 122.29 | 122.29–122.29 | 71.22 | 271.67 |
| mesh | vps → mac | file | 1024 | 1 | 1024 | 105.21 | 105.21–105.21 | 71.39 | 241.34 |
| mesh | vps → mac | html | 1024 | 1 | 2092 | 93.08 | 93.08–93.08 | 71.00 | 246.76 |
| mesh | vps → mac | png | 16 | 1 | 854 | 99.40 | 99.40–99.40 | 71.17 | 217.54 |
| mesh | vps → mac | text | 16 | 1 | 16 | 97.09 | 97.09–97.09 | 71.44 | 193.19 |
| mesh | vps → windows | file | 0 | 1 | 0 | 302.18 | 302.18–302.18 | 73.07 | 457.74 |
| mesh | vps → windows | file | 1024 | 1 | 1024 | 281.28 | 281.28–281.28 | 73.28 | 423.39 |
| mesh | vps → windows | html | 1024 | 1 | 2092 | 281.91 | 281.91–281.91 | 72.81 | 414.79 |
| mesh | vps → windows | png | 16 | 1 | 854 | 278.59 | 278.59–278.59 | 73.31 | 430.06 |
| mesh | vps → windows | text | 16 | 1 | 16 | 278.54 | 278.54–278.54 | 73.37 | 417.53 |
| mesh | windows → mac | file | 0 | 1 | 0 | 33.81 | 33.81–33.81 | 2.88 | 62.34 |
| mesh | windows → mac | file | 1024 | 1 | 1024 | 36.02 | 36.02–36.02 | 2.92 | 62.38 |
| mesh | windows → mac | html | 1024 | 1 | 2092 | 14.88 | 14.88–14.88 | 2.86 | 56.74 |
| mesh | windows → mac | png | 16 | 1 | 854 | 23.61 | 23.61–23.61 | 3.17 | 47.22 |
| mesh | windows → mac | text | 16 | 1 | 16 | 22.68 | 22.68–22.68 | 2.99 | 61.28 |
| mesh | windows → vps | file | 0 | 1 | 0 | 292.56 | 292.56–292.56 | 73.07 | 409.00 |
| mesh | windows → vps | file | 1024 | 1 | 1024 | 296.25 | 296.25–296.25 | 73.28 | 414.30 |
| mesh | windows → vps | html | 1024 | 1 | 2092 | 699.25 | 699.25–699.25 | 72.81 | 792.96 |
| mesh | windows → vps | png | 16 | 1 | 854 | 284.03 | 284.03–284.03 | 73.31 | 407.61 |
| mesh | windows → vps | text | 16 | 1 | 16 | 280.05 | 280.05–280.05 | 73.37 | 411.22 |
| pairwise-mac-vps | mac → vps | file | 0 | 1 | 0 | 114.06 | 114.06–114.06 | 71.47 | 231.81 |
| pairwise-mac-vps | mac → vps | file | 1024 | 1 | 1024 | 113.85 | 113.85–113.85 | 71.27 | 226.40 |
| pairwise-mac-vps | mac → vps | html | 1024 | 1 | 2092 | 124.59 | 124.59–124.59 | 70.99 | 237.28 |
| pairwise-mac-vps | mac → vps | png | 16 | 1 | 854 | 104.04 | 104.04–104.04 | 71.29 | 229.17 |
| pairwise-mac-vps | mac → vps | text | 16 | 1 | 16 | 101.83 | 101.83–101.83 | 71.28 | 229.95 |
| pairwise-mac-vps | vps → mac | file | 0 | 1 | 0 | 103.07 | 103.07–103.07 | 71.47 | 249.16 |
| pairwise-mac-vps | vps → mac | file | 1024 | 1 | 1024 | 104.58 | 104.58–104.58 | 71.27 | 249.60 |
| pairwise-mac-vps | vps → mac | html | 1024 | 1 | 2092 | 91.09 | 91.09–91.09 | 70.99 | 250.81 |
| pairwise-mac-vps | vps → mac | png | 16 | 1 | 854 | 93.05 | 93.05–93.05 | 71.29 | 199.07 |
| pairwise-mac-vps | vps → mac | text | 16 | 1 | 16 | 98.66 | 98.66–98.66 | 71.28 | 245.14 |
| pairwise-mac-windows | mac → windows | file | 0 | 1 | 0 | 21.03 | 21.03–21.03 | 3.09 | 63.72 |
| pairwise-mac-windows | mac → windows | file | 1024 | 1 | 1024 | 29.20 | 29.20–29.20 | 3.03 | 69.61 |
| pairwise-mac-windows | mac → windows | html | 1024 | 1 | 2092 | 25.35 | 25.35–25.35 | 3.15 | 53.60 |
| pairwise-mac-windows | mac → windows | png | 16 | 1 | 854 | 15.88 | 15.88–15.88 | 3.07 | 55.00 |
| pairwise-mac-windows | mac → windows | text | 16 | 1 | 16 | 20.18 | 20.18–20.18 | 3.07 | 61.52 |
| pairwise-mac-windows | windows → mac | file | 0 | 1 | 0 | 41.00 | 41.00–41.00 | 3.09 | 58.72 |
| pairwise-mac-windows | windows → mac | file | 1024 | 1 | 1024 | 30.00 | 30.00–30.00 | 3.03 | 56.41 |
| pairwise-mac-windows | windows → mac | html | 1024 | 1 | 2092 | 6.57 | 6.57–6.57 | 3.15 | 40.34 |
| pairwise-mac-windows | windows → mac | png | 16 | 1 | 854 | 23.53 | 23.53–23.53 | 3.07 | 52.20 |
| pairwise-mac-windows | windows → mac | text | 16 | 1 | 16 | 23.60 | 23.60–23.60 | 3.07 | 55.35 |
| pairwise-vps-windows | vps → windows | file | 0 | 1 | 0 | 277.27 | 277.27–277.27 | 72.74 | 425.12 |
| pairwise-vps-windows | vps → windows | file | 1024 | 1 | 1024 | 279.08 | 279.08–279.08 | 73.43 | 426.69 |
| pairwise-vps-windows | vps → windows | html | 1024 | 1 | 2092 | 301.70 | 301.70–301.70 | 73.64 | 430.62 |
| pairwise-vps-windows | vps → windows | png | 16 | 1 | 854 | 273.07 | 273.07–273.07 | 73.92 | 454.92 |
| pairwise-vps-windows | vps → windows | text | 16 | 1 | 16 | 271.52 | 271.52–271.52 | 73.14 | 415.91 |
| pairwise-vps-windows | windows → vps | file | 0 | 1 | 0 | 293.84 | 293.84–293.84 | 72.74 | 415.87 |
| pairwise-vps-windows | windows → vps | file | 1024 | 1 | 1024 | 294.18 | 294.18–294.18 | 73.43 | 420.94 |
| pairwise-vps-windows | windows → vps | html | 1024 | 1 | 2092 | 277.34 | 277.34–277.34 | 73.64 | 415.35 |
| pairwise-vps-windows | windows → vps | png | 16 | 1 | 854 | 286.72 | 286.72–286.72 | 73.92 | 407.51 |
| pairwise-vps-windows | windows → vps | text | 16 | 1 | 16 | 292.16 | 292.16–292.16 | 73.14 | 416.75 |

## Limits

- System clipboard, native rendering, sleep/wake and network switching are not exercised.
- HTML submission uses a test injector through the production local API; other content uses the production CLI.
- Distinct OS machine identities do not attest bare-metal hardware or exclude virtual machines.
- Network paths remain unknown unless observed independently; SSH reachability does not prove a direct synchronization path.
- Negative checks use bounded observation windows. Windows stop uses forced termination; Unix stop starts with SIGTERM.
- Three samples do not support a reliable tail-latency percentile. Inspect all raw samples and bounds.
