# Independent quick-panel memory replay

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
