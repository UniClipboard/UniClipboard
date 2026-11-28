# Windows Desktop memory research harness

> **Archived research harness notice**: Superseded for ongoing baseline regression and cross-platform measurement by the formal benchmark suite in [`tools/bench/`](../../bench/cmd/unic-bench/main.go) (`unic-bench` and `make bench-mac` / `make bench-win-vm`). See [Desktop resource benchmark architecture](../../../docs/architecture/desktop-benchmark.md) and keep this directory only for historical replay traceability.

This is an opt-in research harness. It never applies a production optimization.
It measures real Windows Desktop/service processes with synthetic data, using the
repository's existing `desktop_e2e` clipboard isolation. Diagnostic builds are
not the downloaded installer payload.

Prerequisites: macOS with the existing Parallels Windows ARM64 VM, `prlctl`, `go`,
Python 3.9+ and Pillow 12.1.1 with zlib 1.2.12 for matching fixture hashes. The existing VM must be running or suspended, have an
active console user, and expose `\\Mac\Home`. The harness does not change sharing
settings, personal profiles, clipboard bytes, OS permissions or passwords.
It refuses pre-existing Desktop/service processes. It restores a VM it resumed.
PowerShell execution-policy override applies only to the task process.

Run one complete paired dataset replay from the repository root:

```sh
python3 tools/research/windows-desktop-memory/replay.py \
  --vm 'Windows 11' --repetitions 1 \
  --output .build/windows-memory-replay
```

Use `--repetitions 3` for the full alternating architecture matrix. A smaller
smoke replay uses `--datasets empty --architectures arm64 --repetitions 1`.
Each principal case samples 45 s visible, 30 s hidden and 30 s reopened.
Service initialization and synthetic seeding add time before those samples.
The supplied source SHA remains pinned; `--source` explicitly selects another
commit and stops if the diagnostic seam has changed.

The command exports source into a temporary directory, adds the reviewable
`.go.txt` diagnostic sources, builds both architectures, generates seeded PNGs,
initializes private CLI profiles explicitly, and runs native-window measurements.
It emits source/binary identities, phase summaries, a cleanup receipt and
`SHA256SUMS`. It removes its source snapshot, binaries, profiles, raw logs and
synthetic images. No listening port is opened. The native automation endpoint
uses a task-local Unix socket; this is not a claim of a user-only ACL.

`measure.ps1` can also run inside an already verified guest. Its `-Diagnostic`
switch requires the existing `desktop_e2e` build. The exact-package mode holds the clipboard busy without reading or changing it.
It requires review of its target-only OS capture before accepting the history
measurement. The recorded exact-package control passed this review after the
service-ready correction; earlier failed attempts remain excluded.

Use `--datasets images --architectures arm64 --repetitions 1 --explore --cycles 10`
for the native list/grid/selection/scroll/quick-panel and hide/reopen growth screen.
`--stream-neighbors` applies only the retained temporary streaming patch inside
the source snapshot. Compare that run with a separate baseline invocation.

`measure-window.ps1` measures process-only, blank-window and text-window controls
built from `window_probe.go.txt`. The probe also separates Kit-light, Kit-dark,
and CJK text-light/text-dark modes. These auxiliary runs are not application
baselines. Add `--detail-admission-control` for the temporary 4-to-1 detail
admission control. Add `--quick-only` to isolate the Settings/Quick Panel route. The portable entry is `replay.py`.

All counters retain byte units. The summary keeps resident working set, private
working set and private commit separate. `VirtualQueryEx` regions describe
committed address-space types; they are not native-heap or GPU allocation totals.
The 200 MB detector reports each metric separately because the user's actual
Task Manager column and process grouping were unavailable.

`gpu_probe.go.txt` and `gpu-present-probe.patch` apply only to a private copy of
mygo at `9233663736ca`. The task builds that copy through a temporary module
override. `UNIC_MEMORY_GPU_SWEEP=1` enables the diagnostic immediate-unused-image
sweep in the same instrumented binary. This is a lower-bound control, not a
production cache policy. Texture-area estimates do not measure driver residency.

To rebuild the renderer attribution control, use the same pinned source export
and diagnostic injection as `replay.py`. Copy the resolved mygo module directory
(`go list -m -f '{{.Dir}}' github.com/egoist/mygo`) into task-private scratch, make
that copy writable, and apply `gpu-present-probe.patch` there with `patch -p1`.
Place `gpu_probe.go.txt` at `internal/gpu/d3d11/memory_research.go` in that copy.
Use `go mod edit -replace github.com/egoist/mygo=<scratch-copy>` only in the
exported application source. Never edit the repository or shared module cache.
Build ARM64 Desktop once with the diagnostic flags in `source-identity.json`.
Run `measure.ps1 -Dataset images -Diagnostic -Explore` twice with that identical
binary: first `UNIC_MEMORY_GPU_SWEEP=0`, then `UNIC_MEMORY_GPU_SWEEP=1`, with fresh
run directories. Add `-Cycles 10` to the original-behavior run. Preserve real
visibility checks; the Quick Panel phases are comparable only when both are
visible. Summarize the raw result directories with `summarize-gpu.py` and retain
only aggregate counters and native crop hashes. Remove the private module copy
and override with the rest of the task scratch. The override is a diagnostic
build detail, not a proposed dependency change.
