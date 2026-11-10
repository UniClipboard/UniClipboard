# 0004 P1 two-host CLI acceptance

Status: **passed**. Source HEAD: `1e8dae8eb3a386fbc922b0c2dd101f8fe012e700`.

Scope: two physical hosts, production CLI, real tailcat; no system clipboard.

Start: 2026-11-10T00:25:11+0800. End: 2026-11-10T00:40:29+0800.

The source manifest includes uncommitted production-source content. HEAD alone does not identify this build.

## Hosts and build

- go version go1.27.3 darwin/arm64
- macOS: Darwin arm64
- Windows: Microsoft Windows 10.0.26200  X64
- Verified snapshot source matches recorded hashes: True

## Functional checks

- PASS: matching production binaries transferred and verified; no UI dependency
- PASS: isolated headless daemons run on macOS and Windows
- PASS: unpaired submission commits locally with no_targets
- PASS: fingerprints match; no transmission before mutual approval
- PASS: mutual CLI approval establishes real two-host tailcat connection
- PASS: UTF-8 content verified; concurrent retries deduplicate; changed input conflicts; receive tags remain local
- PASS: capture processor preserves local content, tags and expiry while blocking sync
- PASS: sync.enabled: local commit succeeds, remote blocked, no replay on enable
- PASS: plugins.instances."builtin.tailcat".sync.text: local commit succeeds, remote blocked, no replay on enable
- PASS: bidirectional size matrix: every sample exported and SHA256 verified
- PASS: completed matrix daemon logs are empty: no content, address or ticket output
- PASS: same matrix profiles preserve identities, trust and every history ID across restart
- PASS: new bidirectional content transfers and exports correctly after restart
- PASS: revocation blocks later remote content while local commit succeeds

## Latency

controller monotonic time before submit until receiver history export SHA256 verification; includes CLI, SSH, polling, export and hashing; not pure network latency.

Each cell contains three samples in the full run. Median and observed minimum/maximum are reported; three samples do not support a tail-latency percentile.

The receipt estimate uses sender-local CLI start time and Engine receive time. It includes sender CLI startup, IPC, commit and receive processing. A native helper estimates host clock offset over a persistent SSH connection before each matrix cell; this is not a pure transport measurement.

Initial clock-estimate uncertainty: ±1.72 ms, plus 1 ms timestamp quantization. Each raw sample records its own uncertainty. Estimates below that bound do not resolve a precise one-way latency.

SSH clock-probe round-trip median: 370.44 ms. No overhead is subtracted from verified latency.

Maximum per-sample receipt-estimate uncertainty: ±2.93 ms. Final minus initial clock offset: -1.90 ms.

| Direction | Type | Bytes | n | Submit median ms | Verified median ms | Verified min–max ms | Receipt estimate median ms |
|---|---|---:|---:|---:|---:|---:|---:|
| mac_to_windows | file | 0 | 3 | 11.77 | 4232.78 | 3770.30–4729.24 | 19.89 |
| mac_to_windows | file | 1024 | 3 | 13.60 | 3923.13 | 3820.69–4086.46 | 20.44 |
| mac_to_windows | file | 1048576 | 3 | 13.41 | 3984.47 | 3781.49–4061.57 | 177.73 |
| mac_to_windows | file | 16777216 | 3 | 14.00 | 5911.42 | 5796.94–21514.84 | 1967.19 |
| mac_to_windows | file | 104857600 | 3 | 39.79 | 17189.45 | 16000.96–23031.89 | 13123.86 |
| mac_to_windows | png | 852 | 3 | 15.98 | 3927.27 | 3808.54–4006.34 | 19.10 |
| mac_to_windows | png | 196987 | 3 | 20.92 | 3851.26 | 3765.48–3939.60 | 61.49 |
| mac_to_windows | png | 3147775 | 3 | 40.72 | 4089.59 | 3989.55–5570.88 | 473.06 |
| mac_to_windows | text | 1 | 3 | 13.68 | 3945.27 | 3909.65–3951.82 | 18.76 |
| mac_to_windows | text | 1024 | 3 | 10.12 | 3873.49 | 3691.51–3898.16 | 13.96 |
| mac_to_windows | text | 65536 | 3 | 17.94 | 3975.32 | 3697.23–4127.58 | 41.93 |
| mac_to_windows | text | 1048576 | 3 | 32.88 | 3885.85 | 3813.35–3897.64 | 263.21 |
| windows_to_mac | file | 0 | 3 | 1448.95 | 1481.06 | 1447.08–1525.68 | 63.44 |
| windows_to_mac | file | 1024 | 3 | 1434.92 | 1472.02 | 1457.61–1480.90 | 62.04 |
| windows_to_mac | file | 1048576 | 3 | 1515.26 | 1544.66 | 1481.34–1569.11 | 212.88 |
| windows_to_mac | file | 16777216 | 3 | 1849.50 | 2286.36 | 2139.20–2627.26 | 1721.61 |
| windows_to_mac | file | 104857600 | 3 | 1835.22 | 11477.82 | 10523.38–11526.62 | 10752.73 |
| windows_to_mac | png | 852 | 3 | 1522.32 | 1554.51 | 1461.12–1595.68 | 56.66 |
| windows_to_mac | png | 196987 | 3 | 1432.93 | 1470.25 | 1459.17–1540.95 | 109.66 |
| windows_to_mac | png | 3147775 | 3 | 1448.02 | 1482.34 | 1463.94–1511.64 | 505.99 |
| windows_to_mac | text | 1 | 3 | 1435.82 | 1467.10 | 1465.71–1474.83 | 59.18 |
| windows_to_mac | text | 1024 | 3 | 1493.11 | 1518.82 | 1487.81–1525.98 | 57.21 |
| windows_to_mac | text | 65536 | 3 | 1480.71 | 1507.15 | 1491.27–1583.10 | 84.76 |
| windows_to_mac | text | 1048576 | 3 | 1463.90 | 1509.00 | 1464.72–1741.82 | 188.21 |

## Limits

- HTML/rich-text submission is not exposed by the production CLI.
- native capture/application and echo are not exercised.
- in-flight queue replacement, slow observers and cancellation races require separate acceptance.
- Direct LAN versus DERP routing was not independently established.
- Negative transmission checks use bounded observation windows.
- Windows restart uses forced process termination; macOS uses SIGTERM.

## Evidence

Manifest SHA256: `4dad670a731141e7c6657383a79fd864183fbb9d01b775da9bb4b497cc0cd642`.

The adjacent manifest contains source and binary hashes and every raw sample. Reproduce with the runner and instructions in `e2e/twohost/README.md`.
