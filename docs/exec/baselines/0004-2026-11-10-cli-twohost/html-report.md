# 0004 P1 two-host CLI acceptance

Status: **passed**. Source HEAD: `1e8dae8eb3a386fbc922b0c2dd101f8fe012e700`.

Scope: two physical hosts; HTML+text injected via production local API; official unic exports verify both representations.

Start: 2026-11-10T00:41:13+0800. End: 2026-11-10T00:43:33+0800.

The source manifest includes uncommitted production-source content. HEAD alone does not identify this build.

## Hosts and build

- go version go1.27.3 darwin/arm64
- macOS: Darwin arm64
- Windows: Microsoft Windows 10.0.26200  X64
- Verified snapshot source matches recorded hashes: True

## Functional checks

- PASS: fresh physical hosts mutually approved with matching fingerprints
- PASS: 18 HTML+text transfers preserve both MIME types and every exported byte

## Latency

controller monotonic time before submit until receiver history export SHA256 verification; includes CLI, SSH, polling, export and hashing; not pure network latency.

Each cell contains three samples in the full run. Median and observed minimum/maximum are reported; three samples do not support a tail-latency percentile.

The receipt estimate uses sender-local CLI start time and Engine receive time. It includes sender CLI startup, IPC, commit and receive processing. A native helper estimates host clock offset over a persistent SSH connection before each matrix cell; this is not a pure transport measurement.

Initial clock-estimate uncertainty: ±1.78 ms, plus 1 ms timestamp quantization. Each raw sample records its own uncertainty. Estimates below that bound do not resolve a precise one-way latency.

Maximum per-sample receipt-estimate uncertainty: ±2.85 ms. Final minus initial clock offset: -0.24 ms.

| Direction | Type | Bytes | n | Submit median ms | Verified median ms | Verified min–max ms | Receipt estimate median ms |
|---|---|---:|---:|---:|---:|---:|---:|
| mac_to_windows | html+text | 2092 | 3 | 9.02 | 4029.91 | 3687.79–4336.48 | 20.65 |
| mac_to_windows | html+text | 131116 | 3 | 17.94 | 3730.74 | 3727.90–3873.46 | 43.96 |
| mac_to_windows | html+text | 2097196 | 3 | 32.79 | 4040.01 | 3915.74–4190.13 | 326.20 |
| windows_to_mac | html+text | 2092 | 3 | 1516.58 | 1545.31 | 1499.28–1599.39 | 55.49 |
| windows_to_mac | html+text | 131116 | 3 | 1428.07 | 1462.55 | 1454.91–1470.11 | 91.42 |
| windows_to_mac | html+text | 2097196 | 3 | 1532.48 | 1580.94 | 1482.23–1935.86 | 450.91 |

## Limits

- HTML submission uses a test injector because unic submit has no HTML input.
- native clipboard capture and application are not exercised.
- Direct LAN versus DERP routing was not independently established.
- Negative transmission checks use bounded observation windows.
- Windows restart uses forced process termination; macOS uses SIGTERM.

## Evidence

Manifest SHA256: `432b585b274132899bede75ade8599c81299982caea01cea2402256209335ef7`.

The adjacent manifest contains source and binary hashes and every raw sample. Reproduce with the runner and instructions in `e2e/twohost/README.md`.
