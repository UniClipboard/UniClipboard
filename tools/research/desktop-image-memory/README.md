# Desktop image memory implementation replay

The immutable application baseline is `173d362036ae4a746878306915c85bcdf7468751`.
The approved implementation is local commit `ea91a8f66938`. The retained patches build
baseline, admission-only, streaming-only and combined variants; preparation rejects
combined production files that differ from that commit. All variants use identical
aggregate instrumentation, mygo version, fixtures and non-race flags.

Prerequisites: the research harness prerequisites in
[the original replay](../windows-desktop-memory/README.md), plus an explicit VM
handoff with no other application/service task. Never interrupt another task.
The application must be built with `desktop_e2e` to isolate clipboard access.
The runner uses the existing console-subsystem automation CLI; a GUI-subsystem
CLI does not provide synchronous PowerShell session output.

One replay command from the repository root:

```sh
python3 tools/research/desktop-image-memory/replay.py all \
  --stage "$HOME/.cache/unic-image-memory-replay" \
  --output .build/desktop-image-memory-replay --vm 'Windows 11'
```

Preparation alone is safe while the VM is busy: replace `all` with `prepare` and
omit `--output`. After handoff, use `run` with that same stage and a fresh output.
`run` verifies every staged file against preparation hashes before touching the VM.
The stage must be fresh for preparation and live below the shared home directory.

The default schedule performs three alternating-order repetitions. ARM64 images
include all four variants. Empty/text ARM64 controls include baseline and combined.
Matched x64-on-ARM64 image controls include baseline and combined only. Use
`--repetitions 1 --datasets images --architectures arm64 --variants baseline combined`
for a smaller replay. Those flags change the declared coverage; do not relabel a
smaller run as the full matrix. A full matrix has30 accepted runs and takes about
one hour, including seeding,45s visible,30s hidden,30s reopened and an image
selection checkpoint. WM_CLOSE hides the GUI at this source; it does not exit it.

Counters distinguish total/private working set and private commit. Go aggregate
heap and texture-area estimates are different evidence. The implementation replay
does not add a framework fork, forced GC, GOMEMLIMIT or renderer eviction.
Readiness timestamps describe UI-observed selection, not physical input latency.
Native crop equality checks only the selected synthetic image region, not the
entire desktop. Raw synthetic captures stay in scratch and are deleted.

The output retains small source/program/VM identities, per-phase counters,
latency/cache aggregates, capture/crop hashes, cleanup and SHA256SUMS. The runner
removes guest profiles, programs, fixtures, logs, captures, temporary source and
host staging after owned processes exit. It restores a VM it resumed; a VM that
was already running remains running. No other VM or process is stopped.

Create `STOP_AFTER_RUN` inside the owned stage to stop after a completed run.
The runner retains the failure/interruption receipt and still cleans its task
resources. Interrupt cleanup stops only its matching sampler before waiting for
owned programs, preventing a seeding/spawn race.

`comparison.json` aggregates run medians and ranges. Group counters pair same-sample
process values. It preserves first10s and full45s visible sampled peaks separately.
The CPU and latency thresholds trigger investigation; a one-run result is not a
completed three-repetition claim.

After the performance matrix, a separate Windows process check reuses the existing
canonical image scenario with task-prebuilt programs. It runs software views and a
real Windows service; it does not establish physical input or native presentation.
It records actual Windows cancellation timings and unchanged input/orientation/
resource assertions. The source adapter only replaces host compilation, `/tmp`
and fixture initialization; it does not relax assertions. Windows ARM64 has no
race detector in the pinned toolchain. Run after the VM handoff, without another
active measurement:

```sh
python3 tools/research/desktop-image-memory/windows-correctness.py \
  --vm 'Windows 11' --stage "$HOME/.cache/unic-image-memory-correctness" \
  --output .build/desktop-image-memory-windows-correctness
```
