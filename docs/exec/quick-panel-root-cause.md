# Independent quick-panel memory: root-cause follow-up

## Scope and identity

The user requested further attribution after the [memory candidate](quick-panel-memory.md)
failed to establish sustained OS memory savings. Product source remains
`d17f8da3fcf6d4f8f188d3336130090097e48d0c`, branch `fix/quick-panel-memory`.
The independent roles and hidden-panel lifecycle remain unchanged. This follow-up
changes research tooling only. It does not accept or publish the earlier candidate.

The same Windows 11 ARM64 guest and synthetic corpus were used: thirty texts and
twelve 2048x1536 PNGs, private initialized profiles, telemetry/sync disabled,
1100x720 GUI, 380x366 panel and image satellite. ARM64 is native; x64 is emulated.
The x64 control uses `ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0` plus the identical
same-query projection correction from the first investigation. Product sources,
program digests, module version, injected source hashes and PE identities are in
[builds.json](evidence/quick-panel-root-cause/builds.json).

## Findings

### Confirmed: orphaned D3D11 image textures outlive their Go owners

The pinned mygo version is `v0.8.4-0.20261108082913-9233663736ca`.
Its D3D11 `Renderer.imageView` stores uploaded textures in `r.images`, keyed by
scene image ID. That map has native handles, not strong Go image references.
`Renderer.show` invokes `sweep` after 120 rendered frames. The image eviction
condition is `r.frame-t.lastFrame > 240`. This counts rendering activity, not
elapsed time or resource lifetime. `Window.Hide` hides the native window and
leaves its renderer alive. An event-driven hidden/static window does not advance
the required frame count.

The isolated build added a weak image owner to each texture and a weak renderer
registry. Neither registry extends image or renderer lifetimes. Before and after
a single diagnostic GC, the panel retained native textures whose weak Go owners
were nil. In the no-capture ARM64 trace, its satellite stopped at frame 53 and
had never swept. The main panel reached frame 180 and had swept at frame 120,
but its old images had not reached the 240-frame threshold.

After diagnostic GC, orphaned base RGBA texture payload was 29,257,728 bytes on
ARM64, 22,732,800 bytes on x64 candidate, and 25,890,816 bytes on x64 control.
A one-time diagnostic removal reduced every observed orphan count to zero.
Active payload remained 3,440,640 / 3,637,248 / 3,637,248 bytes respectively.
This confirms native texture retention independently of Go heap garbage.

GUI has the same defect. After three image selections it retained roughly
10.1 MB / 10.3 MB of textures with dead owners on ARM64 / x64. The earlier
statement that this might be panel-specific is not supported. Panel history and
satellite have separate renderers, which retain their own texture caches.

**These byte counts describe texture payload, not process private commit or
GPU/driver residency.** Releasing the orphaned handles did not reduce the
observed private-commit or private-WS medians in the following fifteen seconds.
Do not claim a 23–29 MB process saving. Driver pooling, allocation ownership and
complete graphics residency were not measured. Texture retention is a confirmed
resource defect, but it does not by itself explain the Task Manager column.

### Confirmed: substantial dead Go allocations remain during idle intervals

Normal operation completed first. The driver then requested exactly one GC in
the private panel and wrote an allocation-stack heap profile. This intervention
is diagnostic only; it is excluded from performance acceptance and is not a
proposal for recurring GC or working-set trimming.

Values below are medians from the last half of each phase; MB is decimal.
Each row is one diagnostic trace, not a three-pair performance estimate.

| Trace | Hidden private commit MB | After diagnostic GC MB | After orphan release MB | Go objects MB, before → after GC |
|---|---:|---:|---:|---:|
| ARM64, no capture | 178.61 | 151.35 | 151.35 | 57.08 → 18.69 |
| x64 candidate, no capture | 217.26 | 177.59 | 177.59 | 58.10 → 18.73 |
| x64 control, no capture | 220.86 | 185.04 | 185.04 | 57.54 → 18.70 |

GC count advanced by one. Released Go heap pages increased by about 27.2 MB,
40.6 MB and 36.8 MB respectively. Cache counts remained twelve images, one
original, eighteen details and zero in-flight reads. The Go allocator can retain
reusable spans, and object collection and page return are separate events.
The normal idle interval had not yet reclaimed all of that transient allocation.
The intervention establishes that these bytes were not all permanently retained
application objects. It does not establish a safe production reclamation policy.

Heap profiles show one known-length selected-original buffer at approximately
9.5 MiB of sampled live allocation, plus encoded thumbnails and normal UI state.
They do not show old full-original buffers accumulating as live retained payload.
The roughly 18.7 MB post-GC Go object totals are essentially the same for x64
control and candidate. This does not support a new live-object retention leak in
the bounded input reader. Profile sizes are sampling estimates; cache counters
and original buffer length have different measurement boundaries.

The earlier x64 candidate's 36 MB hidden regression did not reproduce in this
changed no-capture/GUI-selection protocol. The current candidate was lower than
control at both hidden checkpoints. These single traces do not overturn the
previous failed performance matrix. They show that GC/page-return timing can
change a short-window result by tens of MB, and that resident/commit acceptance
cannot be inferred from TotalAlloc alone. The precise cause of every earlier
run's difference remains unproven.

### Confirmed measurement perturbation: software captures restore bitmap pixels

A capture-enabled repeat retained 3,244,032 CPU pixel bytes after hiding. Both
no-capture architecture traces retained zero. Source inspection connects this to
`Bitmap.ReleasePixels`, `Bitmap.smaller`/`levelPix`, and the capture renderer:
software rendering needs pixel data and can reconstruct released mip levels.
Those pixels can remain when no later native upload drops them.

The capture repeat's commit fell from 191.94 MB to 135.09 MB after diagnostic GC
and page return. This entire difference cannot be assigned to screenshots:
no-capture runs also retained substantial transient Go allocation. Future memory
measurements must precede software capture or use a separate native cropped
capture session. Visual acceptance and resource measurements remain separate.

### No evidence of a universal GUI-versus-panel memory ordering

With this corpus, the GUI also grew after three image selections: private commit
was 169.71 MB on ARM64 and 224.66 MB on x64. Initial visible panel commit was
138.15 / 163.77 MB. These observations do not reproduce an inherent 60 MB GUI
versus 100+ MB panel gap. The panel later also underwent three hide/reopen cycles
and three selections, so its final checkpoint is a different interaction history.
Do not call these a controlled before/after product-performance comparison.

The user's exact history, selected item, viewport and Task Manager column remain
unavailable. Native window/driver/runtime fixed cost is substantial; the earlier
empty panel was approximately 67 MB private commit. The remaining native bytes
have not been attributed to individual allocation call stacks. Subtracting Go
counters from private commit does not identify a GPU or native-heap owner.

## Reproduction and validation

See [the diagnostic replay](../../tools/research/quick-panel-memory/README.md).
The private mygo copy is created inside the source snapshot. It is never written
to the shared module cache and is deleted after the build. Product go.mod and
product Go files remain unchanged. No public contract was modified.

Three complete no-capture traces passed actual-role identity, original readiness,
three GUI selections, three panel selections, hide/reopen, role cleanup and
unchanged clipboard sequence. No PNGs were produced in those runs. They establish
resource behavior, not visual approval. The previous canonical native/race cases
remain the correctness evidence for the unchanged product.

The command below is red-capable and was run against the ARM64 trace:

```sh
python3 tools/research/quick-panel-memory/attribute.py \
  .build/qp-rootcause-arm64 --output .build/qp-rootcause-arm64-assert.json \
  --assert-no-dead-textures
```

It failed: `dead Go image owners still retain native textures after diagnostic GC`.
The failure is intentional evidence of the resource defect; no production fix is
claimed. The following diagnostic release removed the orphan counts in all three
no-capture traces. [Runs and joined resource records](evidence/quick-panel-root-cause/runs.json),
[phase counters](evidence/quick-panel-root-cause/measurements.csv), and
[heap allocation summaries](evidence/quick-panel-root-cause/heap-summaries.txt)
retain the evidence without binary heap profiles or payloads.

The first instrumented capture run failed at the third selection because the
panel had hidden. Owned cleanup passed. A concurrent task-scoped read-only guest
PowerShell probe may have changed foreground focus; this cause is not proven.
No further guest command was launched during the subsequent active native runs.
The fresh capture repeat and all three no-capture traces passed. The first failure
remains [recorded](evidence/quick-panel-root-cause/first-focus-failure.json).

The successful capture repeat's directory was mistakenly named
`qp-attribution-arm64-no-capture`, although its invocation omitted `--no-captures`.
Its receipt correctly records false and thirteen raster files. It is retained as
capture evidence only. The runner now binds requested flags to observed receipts
and rejects a no-capture run that produces rasters. The initial private build also
failed because copied module directories were read-only; only the private copy's
permissions were adjusted. The shared module cache was never changed.

## Engineering implication and cleanup

A fix should target shared renderer resource ownership and an explicit texture
capacity/lifetime boundary. A 240-rendered-frame grace period does not bound idle
retention. Eviction must preserve reconstruction after texture loss and must run
through the existing main-thread/native lifetime rules. Merely adding a weak owner
to the existing frame-based sweep would still leave hidden windows unswept.
A production fix needs an upstream mygo change or a separately reviewed immutable
dependency update; this research does not introduce a local product override.

The Go evidence calls for reducing transient decode/reconstruction work and
measuring long enough to distinguish dead objects, reusable heap spans and actual
page return. It does not justify periodic forced GC. Panel process exit is not
required to demonstrate either confirmed finding and was not implemented.

All eight task-private build/module stages were deleted. Every run removed its
guest programs and private profiles. The VM was restored to its initial suspended
state. [Cleanup](evidence/quick-panel-root-cause/cleanup.json) and
[SHA256SUMS](evidence/quick-panel-root-cause/SHA256SUMS) bind closure. No push, PR,
merge, installation, release or system-permission/share change occurred.
