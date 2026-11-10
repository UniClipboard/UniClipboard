# Real-machine CLI harness

Status: **passed**. Run: `71a9a4a14adac06b5090804a8dc9ee99`.

Nodes: 3. Distinct operating-system machine identities: 3. Shared-host control: false.

Source HEAD: `b12cc742bc309d7563b04587c0b827b48aa7052e`. The complete source hashes include working-tree inputs captured before building. Build VCS stamping and CGO are disabled explicitly; actual binary hashes are authoritative.

## Nodes

| Node | Execution | Target | Initial free bytes |
|---|---|---|---:|
| mac | local | darwin/arm64 | 140583329792 |
| vps | ssh | linux/amd64 | 2918060032 |
| windows | ssh | windows/amd64 | 62278868992 |

Verified measurement rows: 336 / 336. Planned payload upper bound: 690613368 bytes, excluding setup and transport overhead.

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
- PASS: isolation: stopping one node preserves surviving-node sync; reconnect does not replay offline content and accepts new content
- PASS: isolation: revocation blocks its target without breaking surviving nodes or local commits

## Receive latency

Receive estimates include submitting-client startup, IPC, commit, transport and receiver storage. HTML uses the production local API injector. They exclude controller-side SSH, export and hash verification round trips. `verified_ms` separately includes those control costs. Native clock exchanges bracket each payload cell. Per-sample bounds include both node clock uncertainties, timestamp quantization and observed calibration drift. Bounds are clock bounds, not statistical confidence intervals. A bound reaching zero cannot establish a precise one-way latency. No network path is inferred from the SSH route.

| Scenario | Direction | Type | Nominal bytes / PNG edge px | n | Actual bytes | Receive median ms | Receive min–max ms | Max clock bound ±ms | Verified median ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| mesh | mac → vps | file | 0 | 2 | 0 | 124.25 | 109.09–139.41 | 71.19 | 238.20 |
| mesh | mac → vps | file | 1024 | 2 | 1024 | 117.67 | 117.33–118.01 | 71.98 | 234.53 |
| mesh | mac → vps | file | 1048576 | 2 | 1048576 | 5886.00 | 5657.07–6114.94 | 71.77 | 6037.94 |
| mesh | mac → vps | file | 16777216 | 2 | 16777216 | 25066.25 | 20838.30–29294.19 | 71.82 | 25264.16 |
| mesh | mac → vps | html | 1024 | 2 | 2092 | 110.57 | 101.68–119.46 | 71.52 | 225.69 |
| mesh | mac → vps | html | 1048576 | 2 | 2097196 | 2542.85 | 2428.86–2656.84 | 71.45 | 2664.60 |
| mesh | mac → vps | html | 65536 | 2 | 131116 | 627.28 | 616.19–638.36 | 71.14 | 799.44 |
| mesh | mac → vps | png | 1024 | 2 | 3148214 | 11843.99 | 10263.23–13424.74 | 71.08 | 11986.58 |
| mesh | mac → vps | png | 16 | 2 | 854 | 109.01 | 100.14–117.87 | 71.45 | 236.13 |
| mesh | mac → vps | png | 256 | 2 | 197021 | 857.99 | 769.40–946.58 | 71.37 | 953.76 |
| mesh | mac → vps | text | 1 | 2 | 1 | 115.59 | 95.39–135.79 | 71.23 | 244.76 |
| mesh | mac → vps | text | 1024 | 2 | 1024 | 104.05 | 100.52–107.59 | 71.35 | 240.03 |
| mesh | mac → vps | text | 1048576 | 2 | 1048576 | 1457.18 | 1454.98–1459.38 | 71.60 | 1586.77 |
| mesh | mac → vps | text | 65536 | 2 | 65536 | 449.75 | 443.82–455.68 | 72.00 | 581.63 |
| mesh | mac → windows | file | 0 | 2 | 0 | 28.44 | 22.78–34.10 | 2.99 | 68.30 |
| mesh | mac → windows | file | 1024 | 2 | 1024 | 28.67 | 26.83–30.51 | 2.89 | 78.34 |
| mesh | mac → windows | file | 1048576 | 2 | 1048576 | 138.19 | 135.25–141.12 | 6.21 | 215.95 |
| mesh | mac → windows | file | 16777216 | 2 | 16777216 | 3763.88 | 3762.33–3765.44 | 2.97 | 3872.74 |
| mesh | mac → windows | html | 1024 | 2 | 2092 | 18.65 | 16.54–20.76 | 2.99 | 54.39 |
| mesh | mac → windows | html | 1048576 | 2 | 2097196 | 303.17 | 285.18–321.16 | 3.16 | 367.10 |
| mesh | mac → windows | html | 65536 | 2 | 131116 | 41.23 | 39.64–42.81 | 3.19 | 142.55 |
| mesh | mac → windows | png | 1024 | 2 | 3148214 | 820.24 | 797.48–842.99 | 3.00 | 904.74 |
| mesh | mac → windows | png | 16 | 2 | 854 | 25.88 | 17.01–34.74 | 3.21 | 69.73 |
| mesh | mac → windows | png | 256 | 2 | 197021 | 93.44 | 86.35–100.53 | 2.88 | 142.55 |
| mesh | mac → windows | text | 1 | 2 | 1 | 35.96 | 18.26–53.66 | 3.20 | 93.85 |
| mesh | mac → windows | text | 1024 | 2 | 1024 | 26.00 | 20.46–31.54 | 2.94 | 66.78 |
| mesh | mac → windows | text | 1048576 | 2 | 1048576 | 361.04 | 327.84–394.24 | 2.98 | 542.18 |
| mesh | mac → windows | text | 65536 | 2 | 65536 | 49.63 | 37.70–61.56 | 2.97 | 137.72 |
| mesh | vps → mac | file | 0 | 2 | 0 | 118.12 | 116.49–119.76 | 71.19 | 258.33 |
| mesh | vps → mac | file | 1024 | 2 | 1024 | 129.26 | 123.01–135.51 | 71.98 | 280.67 |
| mesh | vps → mac | file | 1048576 | 2 | 1048576 | 3855.94 | 1961.27–5750.61 | 71.77 | 3985.15 |
| mesh | vps → mac | file | 16777216 | 2 | 16777216 | 17619.36 | 16626.42–18612.30 | 71.82 | 17750.31 |
| mesh | vps → mac | html | 1024 | 2 | 2092 | 98.66 | 97.27–100.05 | 71.52 | 199.78 |
| mesh | vps → mac | html | 1048576 | 2 | 2097196 | 1624.88 | 1622.13–1627.64 | 71.45 | 1740.94 |
| mesh | vps → mac | html | 65536 | 2 | 131116 | 929.12 | 609.33–1248.91 | 71.14 | 1061.26 |
| mesh | vps → mac | png | 1024 | 2 | 3148214 | 2678.00 | 1617.94–3738.07 | 71.08 | 2794.77 |
| mesh | vps → mac | png | 16 | 2 | 854 | 107.94 | 106.09–109.78 | 71.45 | 209.62 |
| mesh | vps → mac | png | 256 | 2 | 197021 | 792.48 | 777.00–807.96 | 71.37 | 894.29 |
| mesh | vps → mac | text | 1 | 2 | 1 | 101.75 | 97.27–106.23 | 71.23 | 249.84 |
| mesh | vps → mac | text | 1024 | 2 | 1024 | 105.97 | 100.37–111.57 | 71.35 | 197.08 |
| mesh | vps → mac | text | 1048576 | 2 | 1048576 | 1513.57 | 1227.05–1800.09 | 71.60 | 1640.40 |
| mesh | vps → mac | text | 65536 | 2 | 65536 | 453.11 | 446.31–459.92 | 72.00 | 560.42 |
| mesh | vps → windows | file | 0 | 2 | 0 | 272.10 | 227.96–316.23 | 73.14 | 397.84 |
| mesh | vps → windows | file | 1024 | 2 | 1024 | 240.49 | 228.74–252.24 | 73.83 | 370.04 |
| mesh | vps → windows | file | 1048576 | 2 | 1048576 | 4399.15 | 3052.82–5745.48 | 76.96 | 4523.58 |
| mesh | vps → windows | file | 16777216 | 2 | 16777216 | 36755.79 | 32342.35–41169.23 | 73.76 | 36934.72 |
| mesh | vps → windows | html | 1024 | 2 | 2092 | 213.03 | 212.14–213.92 | 73.46 | 349.67 |
| mesh | vps → windows | html | 1048576 | 2 | 2097196 | 9085.53 | 5120.78–13050.29 | 73.57 | 9236.71 |
| mesh | vps → windows | html | 65536 | 2 | 131116 | 1634.02 | 1588.81–1679.23 | 73.30 | 1784.04 |
| mesh | vps → windows | png | 1024 | 2 | 3148214 | 4006.25 | 3723.69–4288.81 | 73.04 | 4132.96 |
| mesh | vps → windows | png | 16 | 2 | 854 | 215.85 | 213.50–218.20 | 73.62 | 353.90 |
| mesh | vps → windows | png | 256 | 2 | 197021 | 1817.24 | 1770.22–1864.26 | 73.23 | 1962.42 |
| mesh | vps → windows | text | 1 | 2 | 1 | 246.70 | 210.72–282.67 | 73.40 | 396.76 |
| mesh | vps → windows | text | 1024 | 2 | 1024 | 220.23 | 209.63–230.83 | 73.27 | 363.54 |
| mesh | vps → windows | text | 1048576 | 2 | 1048576 | 2703.87 | 2684.39–2723.34 | 73.54 | 2849.72 |
| mesh | vps → windows | text | 65536 | 2 | 65536 | 2436.47 | 1088.66–3784.28 | 73.93 | 2549.98 |
| mesh | windows → mac | file | 0 | 2 | 0 | 39.14 | 36.64–41.64 | 2.99 | 65.95 |
| mesh | windows → mac | file | 1024 | 2 | 1024 | 49.14 | 37.97–60.31 | 2.89 | 73.28 |
| mesh | windows → mac | file | 1048576 | 2 | 1048576 | 373.42 | 217.78–529.06 | 6.21 | 403.54 |
| mesh | windows → mac | file | 16777216 | 2 | 16777216 | 2254.21 | 2099.94–2408.47 | 2.97 | 2324.46 |
| mesh | windows → mac | html | 1024 | 2 | 2092 | 16.19 | 16.07–16.32 | 2.99 | 55.35 |
| mesh | windows → mac | html | 1048576 | 2 | 2097196 | 353.58 | 318.04–389.13 | 3.16 | 416.92 |
| mesh | windows → mac | html | 65536 | 2 | 131116 | 61.98 | 50.16–73.80 | 3.19 | 120.57 |
| mesh | windows → mac | png | 1024 | 2 | 3148214 | 772.18 | 732.99–811.37 | 3.00 | 813.85 |
| mesh | windows → mac | png | 16 | 2 | 854 | 25.42 | 24.02–26.82 | 3.21 | 54.82 |
| mesh | windows → mac | png | 256 | 2 | 197021 | 88.63 | 79.10–98.15 | 2.88 | 134.95 |
| mesh | windows → mac | text | 1 | 2 | 1 | 27.76 | 26.30–29.21 | 3.20 | 62.38 |
| mesh | windows → mac | text | 1024 | 2 | 1024 | 30.53 | 28.39–32.67 | 2.94 | 57.05 |
| mesh | windows → mac | text | 1048576 | 2 | 1048576 | 210.10 | 182.36–237.84 | 2.98 | 266.59 |
| mesh | windows → mac | text | 65536 | 2 | 65536 | 56.68 | 52.38–60.97 | 2.97 | 121.66 |
| mesh | windows → vps | file | 0 | 2 | 0 | 238.42 | 231.42–245.42 | 73.14 | 345.05 |
| mesh | windows → vps | file | 1024 | 2 | 1024 | 242.87 | 228.20–257.55 | 73.83 | 382.25 |
| mesh | windows → vps | file | 1048576 | 2 | 1048576 | 7324.95 | 3730.81–10919.09 | 76.96 | 7471.78 |
| mesh | windows → vps | file | 16777216 | 2 | 16777216 | 47045.50 | 35479.73–58611.27 | 73.76 | 47240.75 |
| mesh | windows → vps | html | 1024 | 2 | 2092 | 217.98 | 213.10–222.85 | 73.46 | 366.37 |
| mesh | windows → vps | html | 1048576 | 2 | 2097196 | 9980.42 | 5197.37–14763.46 | 73.57 | 10117.37 |
| mesh | windows → vps | html | 65536 | 2 | 131116 | 1568.43 | 1448.61–1688.25 | 73.30 | 1681.23 |
| mesh | windows → vps | png | 1024 | 2 | 3148214 | 10388.18 | 4599.49–16176.87 | 73.04 | 10516.96 |
| mesh | windows → vps | png | 16 | 2 | 854 | 215.96 | 212.56–219.36 | 73.62 | 308.75 |
| mesh | windows → vps | png | 256 | 2 | 197021 | 1594.94 | 1415.46–1774.41 | 73.23 | 1725.89 |
| mesh | windows → vps | text | 1 | 2 | 1 | 214.34 | 213.80–214.89 | 73.40 | 306.40 |
| mesh | windows → vps | text | 1024 | 2 | 1024 | 217.34 | 214.98–219.70 | 73.27 | 306.85 |
| mesh | windows → vps | text | 1048576 | 2 | 1048576 | 4232.04 | 3753.28–4710.80 | 73.54 | 4367.16 |
| mesh | windows → vps | text | 65536 | 2 | 65536 | 743.65 | 742.86–744.45 | 73.93 | 877.45 |
| pairwise-mac-vps | mac → vps | file | 0 | 2 | 0 | 103.28 | 103.00–103.56 | 71.08 | 262.95 |
| pairwise-mac-vps | mac → vps | file | 1024 | 2 | 1024 | 109.46 | 107.32–111.59 | 71.34 | 229.59 |
| pairwise-mac-vps | mac → vps | file | 1048576 | 2 | 1048576 | 1398.48 | 1343.09–1453.88 | 71.34 | 1519.56 |
| pairwise-mac-vps | mac → vps | file | 16777216 | 2 | 16777216 | 19883.45 | 8921.79–30845.12 | 71.94 | 20099.66 |
| pairwise-mac-vps | mac → vps | html | 1024 | 2 | 2092 | 101.49 | 89.27–113.71 | 71.61 | 219.51 |
| pairwise-mac-vps | mac → vps | html | 1048576 | 2 | 2097196 | 5848.80 | 2222.42–9475.19 | 71.16 | 5981.48 |
| pairwise-mac-vps | mac → vps | html | 65536 | 2 | 131116 | 738.78 | 619.23–858.34 | 71.01 | 860.71 |
| pairwise-mac-vps | mac → vps | png | 1024 | 2 | 3148214 | 5679.70 | 5305.57–6053.82 | 71.01 | 5814.08 |
| pairwise-mac-vps | mac → vps | png | 16 | 2 | 854 | 102.76 | 98.56–106.96 | 71.43 | 229.35 |
| pairwise-mac-vps | mac → vps | png | 256 | 2 | 197021 | 779.36 | 768.48–790.24 | 71.25 | 880.61 |
| pairwise-mac-vps | mac → vps | text | 1 | 2 | 1 | 96.59 | 96.45–96.73 | 70.80 | 228.61 |
| pairwise-mac-vps | mac → vps | text | 1024 | 2 | 1024 | 116.42 | 96.08–136.77 | 71.47 | 241.31 |
| pairwise-mac-vps | mac → vps | text | 1048576 | 2 | 1048576 | 1204.84 | 1197.50–1212.17 | 71.15 | 1323.20 |
| pairwise-mac-vps | mac → vps | text | 65536 | 2 | 65536 | 447.73 | 444.15–451.31 | 71.50 | 569.54 |
| pairwise-mac-vps | vps → mac | file | 0 | 2 | 0 | 108.01 | 100.42–115.61 | 71.08 | 241.83 |
| pairwise-mac-vps | vps → mac | file | 1024 | 2 | 1024 | 115.64 | 102.22–129.05 | 71.34 | 252.15 |
| pairwise-mac-vps | vps → mac | file | 1048576 | 2 | 1048576 | 3020.93 | 1441.98–4599.88 | 71.34 | 3156.42 |
| pairwise-mac-vps | vps → mac | file | 16777216 | 2 | 16777216 | 19059.39 | 16064.27–22054.51 | 71.94 | 19194.11 |
| pairwise-mac-vps | vps → mac | html | 1024 | 2 | 2092 | 103.17 | 99.91–106.42 | 71.61 | 227.13 |
| pairwise-mac-vps | vps → mac | html | 1048576 | 2 | 2097196 | 9292.52 | 7964.39–10620.65 | 71.16 | 9419.57 |
| pairwise-mac-vps | vps → mac | html | 65536 | 2 | 131116 | 620.42 | 618.66–622.18 | 71.01 | 756.88 |
| pairwise-mac-vps | vps → mac | png | 1024 | 2 | 3148214 | 5288.91 | 3402.09–7175.72 | 71.01 | 5426.18 |
| pairwise-mac-vps | vps → mac | png | 16 | 2 | 854 | 99.91 | 96.55–103.28 | 71.43 | 242.57 |
| pairwise-mac-vps | vps → mac | png | 256 | 2 | 197021 | 770.95 | 766.81–775.10 | 71.25 | 873.46 |
| pairwise-mac-vps | vps → mac | text | 1 | 2 | 1 | 94.42 | 93.54–95.30 | 70.80 | 221.70 |
| pairwise-mac-vps | vps → mac | text | 1024 | 2 | 1024 | 93.73 | 93.36–94.10 | 71.47 | 211.95 |
| pairwise-mac-vps | vps → mac | text | 1048576 | 2 | 1048576 | 1208.12 | 1197.51–1218.72 | 71.15 | 1346.30 |
| pairwise-mac-vps | vps → mac | text | 65536 | 2 | 65536 | 443.99 | 442.24–445.75 | 71.50 | 534.68 |
| pairwise-mac-windows | mac → windows | file | 0 | 2 | 0 | 22.36 | 21.88–22.84 | 2.91 | 60.44 |
| pairwise-mac-windows | mac → windows | file | 1024 | 2 | 1024 | 20.29 | 16.34–24.23 | 2.86 | 60.47 |
| pairwise-mac-windows | mac → windows | file | 1048576 | 2 | 1048576 | 181.08 | 164.98–197.17 | 3.20 | 237.71 |
| pairwise-mac-windows | mac → windows | file | 16777216 | 2 | 16777216 | 1654.81 | 1577.21–1732.42 | 3.01 | 1783.57 |
| pairwise-mac-windows | mac → windows | html | 1024 | 2 | 2092 | 10.74 | 10.64–10.85 | 3.04 | 50.77 |
| pairwise-mac-windows | mac → windows | html | 1048576 | 2 | 2097196 | 244.96 | 236.15–253.77 | 3.01 | 322.62 |
| pairwise-mac-windows | mac → windows | html | 65536 | 2 | 131116 | 44.62 | 36.47–52.77 | 3.45 | 145.74 |
| pairwise-mac-windows | mac → windows | png | 1024 | 2 | 3148214 | 418.60 | 366.69–470.51 | 2.82 | 474.46 |
| pairwise-mac-windows | mac → windows | png | 16 | 2 | 854 | 20.68 | 15.74–25.61 | 3.07 | 70.09 |
| pairwise-mac-windows | mac → windows | png | 256 | 2 | 197021 | 69.54 | 50.85–88.22 | 3.10 | 129.96 |
| pairwise-mac-windows | mac → windows | text | 1 | 2 | 1 | 16.10 | 13.17–19.04 | 3.01 | 57.12 |
| pairwise-mac-windows | mac → windows | text | 1024 | 2 | 1024 | 25.08 | 21.24–28.92 | 3.73 | 61.12 |
| pairwise-mac-windows | mac → windows | text | 1048576 | 2 | 1048576 | 197.86 | 191.70–204.01 | 3.04 | 297.99 |
| pairwise-mac-windows | mac → windows | text | 65536 | 2 | 65536 | 54.32 | 47.36–61.28 | 2.78 | 135.52 |
| pairwise-mac-windows | windows → mac | file | 0 | 2 | 0 | 71.03 | 37.32–104.75 | 2.91 | 91.27 |
| pairwise-mac-windows | windows → mac | file | 1024 | 2 | 1024 | 31.57 | 30.88–32.26 | 2.86 | 52.64 |
| pairwise-mac-windows | windows → mac | file | 1048576 | 2 | 1048576 | 175.59 | 170.89–180.29 | 3.20 | 245.47 |
| pairwise-mac-windows | windows → mac | file | 16777216 | 2 | 16777216 | 1997.35 | 1868.29–2126.41 | 3.01 | 2070.44 |
| pairwise-mac-windows | windows → mac | html | 1024 | 2 | 2092 | 8.75 | 6.78–10.72 | 3.04 | 40.10 |
| pairwise-mac-windows | windows → mac | html | 1048576 | 2 | 2097196 | 325.21 | 291.09–359.33 | 3.01 | 399.21 |
| pairwise-mac-windows | windows → mac | html | 65536 | 2 | 131116 | 57.31 | 43.52–71.10 | 3.45 | 108.05 |
| pairwise-mac-windows | windows → mac | png | 1024 | 2 | 3148214 | 486.01 | 472.96–499.06 | 2.82 | 532.83 |
| pairwise-mac-windows | windows → mac | png | 16 | 2 | 854 | 23.10 | 22.43–23.77 | 3.07 | 55.53 |
| pairwise-mac-windows | windows → mac | png | 256 | 2 | 197021 | 61.32 | 60.31–62.34 | 3.10 | 115.91 |
| pairwise-mac-windows | windows → mac | text | 1 | 2 | 1 | 22.77 | 22.17–23.37 | 3.01 | 56.60 |
| pairwise-mac-windows | windows → mac | text | 1024 | 2 | 1024 | 24.43 | 24.16–24.69 | 3.73 | 60.49 |
| pairwise-mac-windows | windows → mac | text | 1048576 | 2 | 1048576 | 411.45 | 329.36–493.53 | 3.04 | 473.73 |
| pairwise-mac-windows | windows → mac | text | 65536 | 2 | 65536 | 47.45 | 44.32–50.58 | 2.78 | 101.26 |
| pairwise-vps-windows | vps → windows | file | 0 | 2 | 0 | 215.34 | 211.73–218.95 | 73.93 | 349.69 |
| pairwise-vps-windows | vps → windows | file | 1024 | 2 | 1024 | 212.94 | 212.70–213.18 | 73.25 | 358.85 |
| pairwise-vps-windows | vps → windows | file | 1048576 | 2 | 1048576 | 2581.57 | 2572.93–2590.22 | 73.12 | 2723.19 |
| pairwise-vps-windows | vps → windows | file | 16777216 | 2 | 16777216 | 16299.65 | 11788.86–20810.44 | 73.07 | 16434.91 |
| pairwise-vps-windows | vps → windows | html | 1024 | 2 | 2092 | 208.25 | 204.73–211.76 | 73.65 | 343.97 |
| pairwise-vps-windows | vps → windows | html | 1048576 | 2 | 2097196 | 16998.08 | 12828.95–21167.20 | 73.34 | 17131.22 |
| pairwise-vps-windows | vps → windows | html | 65536 | 2 | 131116 | 2298.29 | 1606.95–2989.64 | 73.34 | 2449.51 |
| pairwise-vps-windows | vps → windows | png | 1024 | 2 | 3148214 | 3708.84 | 3514.47–3903.20 | 73.45 | 3858.17 |
| pairwise-vps-windows | vps → windows | png | 16 | 2 | 854 | 207.76 | 207.67–207.85 | 73.92 | 349.04 |
| pairwise-vps-windows | vps → windows | png | 256 | 2 | 197021 | 1835.61 | 1740.93–1930.29 | 73.75 | 1991.13 |
| pairwise-vps-windows | vps → windows | text | 1 | 2 | 1 | 210.33 | 207.62–213.03 | 73.04 | 353.89 |
| pairwise-vps-windows | vps → windows | text | 1024 | 2 | 1024 | 218.52 | 204.45–232.59 | 73.32 | 365.41 |
| pairwise-vps-windows | vps → windows | text | 1048576 | 2 | 1048576 | 3012.57 | 2855.92–3169.23 | 73.06 | 3167.38 |
| pairwise-vps-windows | vps → windows | text | 65536 | 2 | 65536 | 1066.30 | 1065.15–1067.45 | 73.71 | 1208.52 |
| pairwise-vps-windows | windows → vps | file | 0 | 2 | 0 | 231.66 | 223.29–240.03 | 73.93 | 367.38 |
| pairwise-vps-windows | windows → vps | file | 1024 | 2 | 1024 | 241.99 | 220.92–263.07 | 73.25 | 338.87 |
| pairwise-vps-windows | windows → vps | file | 1048576 | 2 | 1048576 | 2929.76 | 2556.96–3302.57 | 73.12 | 3061.67 |
| pairwise-vps-windows | windows → vps | file | 16777216 | 2 | 16777216 | 32562.52 | 29634.31–35490.73 | 73.07 | 32750.04 |
| pairwise-vps-windows | windows → vps | html | 1024 | 2 | 2092 | 199.88 | 199.60–200.15 | 73.65 | 298.26 |
| pairwise-vps-windows | windows → vps | html | 1048576 | 2 | 2097196 | 3925.79 | 3285.45–4566.13 | 73.34 | 4094.40 |
| pairwise-vps-windows | windows → vps | html | 65536 | 2 | 131116 | 1584.62 | 1418.57–1750.68 | 73.34 | 1693.11 |
| pairwise-vps-windows | windows → vps | png | 1024 | 2 | 3148214 | 5636.91 | 3846.80–7427.01 | 73.45 | 5785.34 |
| pairwise-vps-windows | windows → vps | png | 16 | 2 | 854 | 217.65 | 215.68–219.62 | 73.92 | 304.44 |
| pairwise-vps-windows | windows → vps | png | 256 | 2 | 197021 | 3484.75 | 2711.72–4257.78 | 73.75 | 3607.66 |
| pairwise-vps-windows | windows → vps | text | 1 | 2 | 1 | 216.88 | 209.90–223.87 | 73.04 | 329.88 |
| pairwise-vps-windows | windows → vps | text | 1024 | 2 | 1024 | 232.70 | 224.26–241.15 | 73.32 | 360.43 |
| pairwise-vps-windows | windows → vps | text | 1048576 | 2 | 1048576 | 7890.23 | 7847.96–7932.50 | 73.06 | 7988.40 |
| pairwise-vps-windows | windows → vps | text | 65536 | 2 | 65536 | 1153.53 | 894.42–1412.64 | 73.71 | 1269.69 |

## Limits

- System clipboard, native rendering, sleep/wake and network switching are not exercised.
- HTML submission uses a test injector through the production local API; other content uses the production CLI.
- Distinct OS machine identities do not attest bare-metal hardware or exclude virtual machines.
- Network paths remain unknown unless observed independently; SSH reachability does not prove a direct synchronization path.
- Negative checks use bounded observation windows. Stops use the observed service generation; Unix SIGTERM or Windows termination are fallbacks. Forced stops are recorded.
- Three samples do not support a reliable tail-latency percentile. Inspect all raw samples and bounds.
