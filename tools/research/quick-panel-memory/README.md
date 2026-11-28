# Independent quick-panel memory replay

> **Archived research harness notice**: Superseded for ongoing baseline regression and cross-platform measurement by the formal benchmark suite in [`tools/bench/`](../../bench/cmd/unic-bench/main.go) (`unic-bench` and `make bench-mac` / `make bench-win-vm`). See [Desktop resource benchmark architecture](../../../docs/architecture/desktop-benchmark.md) and keep this directory only for historical replay traceability.

Read [FAILURES.md](FAILURES.md) before edits. This opt-in measurement reuses the
canonical Desktop E2E build, its directory clipboard, existing Windows-native
protocol helper, existing synthetic corpus and research memory counters. It does
not install a package or access personal history or clipboard bytes.

Use only the explicitly handed-over Windows 11 Parallels guest. Record its UUID,
architecture and initial state first. No other application task may use the guest.
The runner requires an already running guest and leaves VM state to its caller.
Restore the initial state after the whole cohort. A stage must be fresh and under
`~/` so the existing `\\Mac\Home` share can transport task files. Do not change
sharing, credentials or permissions.

```sh
python3 tools/research/quick-panel-memory/prepare.py \
  --source ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0 --projection-fix \
  --arch arm64 --stage "$HOME/.cache/unic-qp-control"
python3 tools/research/quick-panel-memory/prepare.py \
  --arch arm64 --stage "$HOME/.cache/unic-qp-candidate"
python3 tools/research/quick-panel-memory/run.py \
  --stage "$HOME/.cache/unic-qp-control" --output .build/qp-control \
  --vm 32affa85-5174-4d5b-ab6b-903a06d7fa9b --datasets images
```

Omit `--source` to freeze current tracked and nonignored source. Every build has
source/program hashes, identical non-race flags, the pinned dependencies and
aggregate-only diagnostics. Weak view references prevent the probe from retaining
retired bindings. The image admission/streaming implementation is already present
in the comparison baseline. The matched control also applies the shared same-query
projection correction. Without it, reopening can show an empty list instead of
real previews. The final memory comparison changes only selected-original input buffering. ARM64 is native on this guest; amd64 is emulated.
Neither is a physical x64 Windows measurement.

Alternate fresh control/candidate runs with `cohort.py --control-stage <stage>
--candidate-stage <stage> --output <fresh-output> --vm <UUID> --repetitions 3`.
The cohort stops on its first failure and retains its journal. Summarize each run
with `summarize.py <run-output> --output <summary.json>`. Validate and compare
complete cohorts with `analyze.py <cohort> ... --output <fresh-analysis>`.
Use three image repetitions per declared
architecture for a final cohort. Use `--selections 3` to add three actual Down-key
selection/original-ready/frame checks and a final hidden phase. Empty/text controls and any reduced coverage
must be reported separately. Sample each real role independently. Byte counters
separate private working set, working set and private commit. Go heap and native
window/handle counts describe different resource boundaries. Graphics residency
is not inferred from private commit. No forced GC or working-set trimming occurs.

The native measurement records rendered frames and private IPC activation. It
is not the canonical native recording case and does not prove real shortcut or
cross-application paste. Run `make desktop-e2e CASE=desktop-native-processes` for
race-instrumented native lifetime/copy/reopen acceptance with continuous recording.
Run `quick-panel-contract` for target/cancellation/keep-open semantics.

The runner retains failures, samples, aggregate diagnostics and synthetic frames
under ignored output. It removes owned guest processes and private profiles.
Remove each owned stage after comparison. Retain only compact aggregate receipts,
source/program identities, frame hashes and SHA256SUMS in project documentation;
never submit programs, databases, source exports or raw logs.

## Root-cause diagnostics (not production optimization)

Use `prepare.py --attribution` for a task-private copy of the pinned mygo module.
The shared module cache and product go.mod remain unchanged. The private copy
records weak Go owners for uploaded D3D11 images and a weak renderer registry.
GPU counters describe base RGBA texture payload, not driver allocation/residency.
The build identity hashes all injected files and the patched renderer.

Use `run.py --no-captures --gui-selections --selections 3 --diagnostic-gc
--diagnostic-release` to exercise three selections in each real GUI/panel, skip
all raster captures, then run two explicitly diagnostic phases. One GC/profile
follows the normal performance phases. A subsequent one-time release removes
only textures whose Go image owners are gone. These interventions are not product
behavior, a memory acceptance result, or permission to add recurring forced GC.
Requested flags and observed receipt flags must match. Never launch another guest
PowerShell during an active native run: a separate console may change foreground
focus. Host-side builds/analysis may proceed concurrently.

Join counters with `attribute.py <run-output> --output <joined.json>`. Add
`--assert-no-dead-textures` for a red-capable real-process resource assertion.
It fails if dead Go image owners still have retained D3D11 textures after the
diagnostic GC. It also requires both panel renderers to be observed. Heap profile
binaries remain ignored; retain only compact allocation summaries and hashes.
A no-capture run establishes memory/resource behavior, not native visual acceptance.
