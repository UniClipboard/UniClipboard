# Two-host CLI acceptance

For new runs, use the [node-configured real-machine harness](../real/README.md). This directory preserves the earlier fixed-host runner and its acceptance evidence.

Run from a macOS controller with Python 3, Go, OpenSSH and a Windows 10/11 x64 SSH target. The target must support AF_UNIX. The current runner uses `C:\Users\mark\AppData\Local\Temp` for its unique remote directory. It does not install a service, launch a desktop application, or access either system clipboard.

```sh
python3 e2e/twohost/run.py --ssh win \
  --output .cache/0004-twohost-$(date +%Y%m%d-%H%M%S) --rounds 3
```

The output directory must not exist. The runner builds production `unic` and `unicd` for macOS and Windows from the current source. It uses isolated state, verifies copied binary hashes, compares both pairing fingerprints and explicitly approves both devices. It stops only the processes it started. Private temporary state remains outside the repository for inspection. Do not publish that state: it includes identity keys and trust data.

The runner accepts `--binaries DIR` for local iteration, with `mac/unic`, `mac/unicd`, `windows/unic.exe` and `windows/unicd.exe`. This option records actual binary hashes but does not prove that those binaries were built from the current source. Use the default build mode for the final acceptance run.

Each direction covers text at 1 B, 1 KiB, 64 KiB and 1 MiB; PNG at 16, 256 and 1024 pixels square; and files at 0 B, 1 KiB, 1 MiB, 16 MiB and 100 MiB. PNG pixels and file bytes are deterministic pseudorandom data. Every sample receives a new request ID. The actual PNG byte size is recorded. The default is three samples per matrix cell. Samples run sequentially after pairing and policy checks, on an already connected session. Connection establishment and cold-start latency are outside this matrix.

The manifest records source and binary SHA256, platform details, settings checks, raw timing samples and completion status. Every successful transfer requires a new remote history item and an exported content SHA256 match. Uploading a fixture through SSH occurs before timing begins.

`verified_ms` is measured by the controller's monotonic clock, from immediately before CLI submission through receiver history polling, export and SHA256 verification. `submit_ms` ends when the submitting CLI returns. Both include CLI and control overhead. The runner measures SSH probe duration separately and does not subtract it. These measurements are reproducible user-level verification times, not one-way transport latency. They do not depend on synchronized host clocks.

Coverage includes mutual approval, no pre-approval content, local commits without targets, identical and conflicting request retries, receive-side tagging, sender-side sensitive-content policy, sync disable/enable, text filtering, restart persistence/reconnection, revocation and two log privacy sentinels.

The production CLI does not expose HTML/multiple-representation submission. This runner does not treat an HTML file as rich-text coverage. Native capture/application, clipboard echo, in-flight replacement, cancellation races, slow observers, sleep/wake and a network switch remain separate acceptance work. Small negative-observation windows provide bounded evidence only.

The manifest also records `receipt_estimate_ms`: the sender-local CLI start time to the Engine's receive timestamp, adjusted with a native clock probe. Before each type/size cell, twelve timestamp exchanges run over one persistent SSH connection. `receipt_estimate_uncertainty_ms` is half the shortest round trip plus 1 ms for timestamp quantization. Calibration runs outside the measured interval. The probe has no role in the content path. The manifest also records the final clock offset change. The initial five PowerShell probes remain as control-overhead evidence. This estimate excludes the controller's export round trips but still includes sender CLI startup, IPC, commit and receiving work. Values smaller than the clock uncertainty cannot establish a precise one-way latency. The monotonic `verified_ms` remains the primary measurement.

Render the result table from a completed manifest:

```sh
python3 e2e/twohost/summarize.py PATH/manifest.json --output PATH/report.md
```

The runner uses `unic config apply` for per-backend policy because the generic `config set` dotted-path helper cannot address plugin IDs that contain dots.

## HTML and multiple representations

The additional runner injects `text/plain` plus `text/html` through the production local service API. It uses a small test client instead of adding a product CLI flag. The transport, processors, history and receiver export remain production code. Its private executable directory keeps the injector separate from production `unic`.

Run it after a successful production CLI manifest:

```sh
python3 e2e/twohost/html.py --ssh win \
  --production-manifest PATH/manifest.json \
  --output .cache/0004-html-$(date +%Y%m%d-%H%M%S)
```

The runner rebuilds the production programs and requires their hashes to match the prior manifest. `--binaries DIR` can reuse those exact binaries. It creates fresh isolated nodes and checks both pairing fingerprints before approval. Each direction has three trials at plain-text sizes of 1 KiB, 64 KiB and 1 MiB. HTML adds a valid UTF-8 document wrapper to the same text. The manifest records the combined size. Both MIME types and both exported byte streams must match.

This supplement provides HTML pipeline evidence. It does not establish a production `unic submit` HTML interface or native rich-text rendering.

If the workspace changes during a run, keep its failed manifest and preserve the exact measured binaries. Use a separate checkout whose production-source hashes match that manifest. `finish.py` can resume only the lifecycle/privacy closure after a complete 72-sample matrix; it verifies source and binary hashes before starting the same isolated profiles. Its final manifest records the prior failure and the recovery. It does not turn unverified transfers into successful samples. A fresh full run is the normal reproduction path.

A macOS service rebuilt in a different checkout may not match the original complete binary hash. The HTML supplement can reuse the exact measured programs through `--binaries DIR`; it rejects a different artifact. Retain that private program directory while running the supplement.
