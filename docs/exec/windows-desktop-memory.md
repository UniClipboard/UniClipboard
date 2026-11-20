# Windows Desktop memory research

Date: 2026-11-20. Scope: local research and reversible diagnostic controls.
No production optimization, installer execution, push, PR or merge is part of this work.

## Evidence boundary

The user reported approximately 200 MB while Windows Desktop was running. The
Task Manager column, process grouping, system architecture, DPI, workload and a
profile were unavailable. No personal history was accessed.

The exact test package is Actions run [44665561906](https://github.com/UniClipboard/UniClipboard/actions/runs/44665561906),
artifact 14596984031, `desktop-test-windows-amd64`. Its actual source is
`173d362036ae4a746878306915c85bcdf7468751`. This was also the freshly fetched
`origin/v2` baseline. The independent branch is `research/windows-desktop-memory`.
See [artifact identity](windows-desktop-memory/artifact-identity.json) for the
worktree, source, archive, installer and program hashes. All six package checksum
entries passed verification. The unsigned installer was extracted, never installed.

An **exact-package synthetic-history control was established** after correcting
service readiness. The [native exact receipt](windows-desktop-memory/native-exact-receipt.json)
records the original Desktop executable SHA256, actual Desktop/child-service PIDs,
and an OS client-area capture reviewed for the twelve-image history and selected
2048x1536 preview. It measured 195.51 MB total Desktop working set,124.58 MB
Desktop private working set, and 85.94 MB service private working set. The two
private working sets sum to 210.52 MB, or **200.77 MiB**. This is a plausible
explanation for a grouped reading near 200, not proof of the user's column/group.

The exact control holds the guest clipboard busy without reading or changing it.
This changes normal capture/paste behavior and can delay main-thread clipboard
retries by up to one second. Initial attempts with failed startup or no valid
history assertion are excluded. The repeated matrix below uses rebuilt same-source
binaries with the existing `desktop_e2e` private testboard, native automation and
aggregate counters. Those binaries are not the downloaded program. Keep both
forms of evidence separate; see [source identity](windows-desktop-memory/source-identity.json).

## Environment and method

The existing Parallels Windows 11 VM is
`32affa85-5174-4d5b-ab6b-903a06d7fa9b`: Windows build 26200, ARM64, four virtual
CPUs, 6144 MiB configured RAM, console session 1, 4096-byte pages and DPI 192.
The initial VM state was suspended. The prior D/G task sessions were archived and
idle, and no Desktop/service process was running before this task resumed it.
The x64 payload is a PE x86-64 image running on ARM64 Windows. The native ARM64
control uses the same application source. `IsWow64Process2` returned an unknown
process-machine value for x64 on this system; PE headers and build metadata are
the architecture identity, not that ambiguous API value.

The protocol was written before measurement in [protocol.md](windows-desktop-memory/protocol.md).
The principal matrix uses three fresh-process repetitions of empty history,
60 text entries, and those entries plus twelve seeded 2048 x 1536 RGB PNGs.
Architecture order alternates by repetition. Window content is 1100 x 720 logical
units, Windows skin, light theme and English UI. Telemetry is disabled in the task
profile. Automatic sync is disabled; explicit synthetic submissions use no paired
devices. Original image sizes and [fixture hashes](windows-desktop-memory/fixture-manifest.json)
are fixed. The service restarts after seeding. No new GC call, GC tuning or memory
limit is applied. The existing 3-second hidden-window release remains active.

Sample once per second. Startup means sampled values in the first 10 seconds;
settled visible samples are at 30-45 seconds. Hidden and reopened samples are
15-30 seconds after real close-to-tray and second-instance activation respectively.
A finite sampled peak is a lower bound on the true instantaneous peak.

`Process.WorkingSet64` records total resident memory. `QueryWorkingSet` records
resident pages with the shared bit clear as private working set. Private bytes
records process private commit. These are different quantities; the usual Task
Manager Memory column is a private-working-set measure, not Go heap or total
working set. See Microsoft's [counter definition](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex)
and [Task Manager explanation](https://blogs.windows.com/msedgedev/2021/01/13/investigate-microsoft-edge-memory-usage/).
Summing total working sets across processes can count shared pages twice.

`VirtualQueryEx` separately records committed MEM_PRIVATE, MEM_MAPPED and MEM_IMAGE
regions and reserved address space. These are not resident native-heap, database
or GPU totals; copy-on-write pages retain their mapped/image classification.
See [VirtualQueryEx](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualqueryex).
Native heap versus graphics/driver residency remains unknown unless specifically
identified by a diagnostic control. The unexplained remainder is never assigned
to mygo or GPU memory. Windows uses mygo's D3D11 renderer, not the macOS Metal path
and not Skia in this pinned dependency.

## Principal measurements

Final repeated-run tables and compact counters are recorded in
[measurements.json](windows-desktop-memory/measurements.json) and
[measurements.csv](windows-desktop-memory/measurements.csv). MB is decimal.

| Architecture | Dataset | Desktop total WS | Desktop private WS | Desktop private commit | Service private commit |
|---|---|---:|---:|---:|---:|
| x64 (emulated) | empty | 102.01 (102.00-111.74) | 33.01 (32.59-42.68) | 87.38 (86.65-98.06) | 78.76 (78.02-94.99) |
| x64 (emulated) | text | 118.71 (118.39-120.04) | 48.77 (48.34-49.47) | 102.65 (102.30-104.33) | 78.97 (78.58-80.50) |
| x64 (emulated) | images | 203.71 (202.38-203.83) | 132.59 (132.21-133.69) | 187.63 (186.20-187.71) | 168.49 (147.60-169.17) |
| ARM64 | empty | 89.27 (88.91-89.41) | 19.18 (18.71-19.26) | 69.10 (68.88-69.52) | 61.88 (61.63-61.96) |
| ARM64 | text | 104.80 (104.73-104.95) | 34.14 (34.10-34.37) | 84.66 (84.62-84.86) | 62.80 (62.12-63.17) |
| ARM64 | images | 189.69 (189.11-189.99) | 118.71 (118.28-119.18) | 169.23 (168.71-169.23) | 151.59 (151.31-151.83) |

Values are the median of three run medians, with the range of run medians in parentheses.

The largest first-10-second sampled Desktop peak across the three image runs was
252.63 MB total WS / 199.59 MB private commit on x64 and 209.07 / 189.29 MB on
ARM64. Empty-history Desktop sampled peaks were 138.87 / 97.16 MB and
126.10 / 68.70 MB respectively. Service image-run sampled commit peaks were
168.24 MB x64 and 151.74 MB ARM64. These sampled peaks are not instantaneous
allocation maxima. All per-run startup peaks are retained separately from settled
values in the JSON.

For the image dataset, Desktop plus its child service total **249.49 MB private
working set on x64** and **221.95 MB on ARM64** (median of three group sums).
Combined private commit is 355.36 MB and 320.55 MB respectively. Thus a process-group
Memory reading near 200 MB can have a different meaning from an individual Desktop
working set. The user's actual grouping remains unknown.

| Architecture | Image visible private commit | Hidden private commit | Reopened private commit |
|---|---:|---:|---:|
| x64 (emulated) | 187.63 (186.20-187.71) | 117.26 (116.48-121.16) | 118.21 (117.55-122.07) |
| ARM64 | 169.23 (168.71-169.23) | 99.66 (99.20-102.43) | 100.57 (100.16-103.26) |

The first x64 image run measured 203.71 MB settled total working set, 132.59 MB
private working set and 187.63 MB private commit. Hidden values fell to
137.74/66.46/121.16 MB; reopened values remained 138.63/67.33/122.07 MB.
This reproduces a numerical 200 MB total-working-set scenario with synthetic
images in a diagnostic build. It does not reproduce the user's unknown Task
Manager column or establish the exact-package observation.

In that run, Go heap objects were 46.05-46.66 MB at 30/40 seconds. Heap address
space was approximately 108.2 MB, with only 18.19 MB released. After the existing
hide release, live heap objects were 19.35-19.66 MB and released heap was
83.49-83.65 MB. The approximately 65 MB increase in released Go heap matches
most of the 66.47 MB decrease in private commit. This is recoverable decode and
collection/scavenging cost. It is not evidence of a leak.

The application cache held twelve shared image owners, one original tier and
eleven thumbnail tiers. All twelve had released their CPU pixels. It retained
50 metadata rows and 21 wanted details. Text-only details held approximately
5040 bytes. Thus neither an unbounded text cache nor retained bitmap CPU pixels
explains that image scenario. Unreachable decode temporaries, encoded
reconstruction bytes and Go heap reservation/release have separate lifetimes.

Native ARM64 first-image values were 189.11/118.28/168.71 MB. Architecture controls
include differences in generated code, runtime, native libraries and emulation;
they do not isolate an emulator-only allocation total. An early ARM64 calibration
had a higher total working set than later native runs while private commit was
similar. Treat resident values as ranges and keep the repeated matrix separate
from that calibration. ARM64 packaging is relevant only to ARM64 Windows users.

## Lifecycle, selection and framework controls

The ARM64 admission surrogate changes only `detailReadLimit` from 4 to 1.
The original image loader remains unchanged. Service private commit fell from
151.59 MB in the repeated baseline to 82.28 MB in this single control. Group
private working set fell by approximately 42 MB; group private commit fell by
approximately 75 MB. Desktop resident memory increased. This does not establish
a universal working-set reduction. The service Go heap address-space allocation
fell from approximately 108 MB to 48 MB. The exact source chain is Desktop
`HistoryContent` -> service handler -> Engine retained content -> blob read.
The service materializes the encoded representation before returning headers;
Desktop currently acquires its image decode gate after opening that response.
Thus serialized body decoding alone does not bound service-side materialization.

The streaming-neighbor control keeps selected originals unchanged and streams
encoded thumbnail input through the existing decoder. Desktop private commit
was 147.12 MB versus 168.50-169.23 MB in the adjacent original controls, about
22 MB lower. Its total working set was 220.67 MB versus approximately 189 MB.
It therefore has a demonstrated commit benefit, **not a demonstrated resident
working-set benefit**. Desktop CPU to the initial 45-second checkpoint was
3.141 seconds versus 2.750 seconds in the adjacent original control. These are
single controls; neither precise CPU overhead nor response-latency percentiles
were established.

The D3D11 paired probe uses the same instrumented executable with only immediate
unused-texture sweeping enabled or disabled. After eight image selections, the
original retained 53 image textures with 29.96 MB estimated RGBA area; the control
retained 12 with 3.69 MB. The original sweep ages objects by presented frame count.
An idle event-driven window does not advance that age. This is bounded by frame
age, but not by a wall-clock deadline or byte budget. After scroll and Quick Panel
creation, original image texture area was 33.80 MB and remained at that level in
final idle. These are object-area estimates, not VRAM or resident-driver counters.
The main double-buffered swapchain area estimate is 25.34 MB at 2200 x 1440;
three created renderers together estimate 33.76 MB of swapchain pixels and about
3.34 MB of font-atlas pixels. These estimates must not be added to working set.

After selections, aggressive sweeping reduced Desktop private commit by about
37 MB and private working set by about 36 MB. After scrolling the commit gap was
about 10 MB. CPU consumed by the selection/scroll prefix increased by roughly
1-1.4 seconds. Four native target-only captures have identical selected-image
crop pixels, including the capture after ten reopen cycles. See
[gpu-resources.json](windows-desktop-memory/gpu-resources.json). The immediate
sweep is an attribution lower bound, not a production policy. Original cached
CPU raster bytes remained zero at those checkpoints. Metadata/detail/image owner
counts remained bounded; the selected original had one application owner.

The original-behavior ten-cycle run kept GDI handles at 67. Reopened private
commit was 150.15 MB at cycle 2 and 149.73 MB at cycle 10; final idle was
150.06 MB. Process handles were 508 through cycles and 504 at final idle.
There is no continuing per-cycle growth in this run. The extra Quick Panel view
and framework resources are one-time retained costs in this scenario. Complete
native heap allocations, driver residency and long-duration leak freedom remain
unknown. Quick Panel visibility in the first paired sequence differed between
runs, so its phase is not a valid paired GPU comparison.

The single-executable auxiliary controls separate application data from window,
Kit and text setup. Each has one 45-second process run; the table uses 30-45-second
medians. All window modes confirmed the GPU backend. The process-only mode does
not create a window. See [window-controls.json](windows-desktop-memory/window-controls.json).

| ARM64 auxiliary mode | Total WS (MB) | Private WS (MB) | Private commit (MB) |
|---|---:|---:|---:|
| Process-only | 60.00 | 38.67 | 51.31 |
| Blank window, light | 69.92 | 9.42 | 59.20 |
| Empty Kit root, light | 69.76 | 9.31 | 58.81 |
| Empty Kit root, dark | 69.72 | 9.22 | 58.72 |
| 50 text/CJK rows, light | 73.37 | 11.83 | 61.28 |
| 50 text/CJK rows, dark | 73.38 | 11.85 | 61.35 |

Text layout/font setup adds about 2.5 MB private commit over an empty Kit root
in this control. Theme deltas are small. These are combined probe costs, not an
exact allocation census of fonts. Private resident pages are not monotonic across
these modes; subtracting resident values does not isolate framework or native-heap
ownership. The full Desktop empty-profile matrix and service counters remain the
relevant application baselines. No claim assigns the unclassified remainder to
a font library, mygo, Skia, GPU or emulation.

The growth screen uses ten hide/reopen cycles followed by 60 seconds idle.
It preserves the existing hide-triggered collection policy. Finite stable
measurements do not prove absence of all long-duration leaks. Selection, grid,
scroll and quick-panel results are separate from the fixed initial-list matrix.

## Ranked proposals

These are candidates for discussion. Temporary controls are not production changes.
Measured deltas overlap and must not be added together.

| Rank | Concrete change and code location | Evidence and affected memory | Cost, risk and acceptance |
|---|---|---|---|
| 1 | Acquire a shared image-request slot **before** `HistoryContent`, in `app/desktop/internal/desktopui/live.go` (`loadSearchEntry`), coordinated with `live_details.go` admission and `live_images.go` decoding. Preserve text concurrency, selected priority and cross-view cancellation. | The 4-to-1 surrogate reduced service private commit by about 69 MB and group private WS by about 42 MB. It targets simultaneously materialized encoded responses and service Go-heap peaks. | Medium risk: queueing can delay selected images. Initial Desktop CPU was 2.797 s versus 2.750 s in the adjacent control; this does not measure latency. Require real Desktop/service E2E with two views, cancelled requests and complete previews; repeat three native runs and measure selection latency. The existing image E2E passed the surrogate; see its receipt. |
| 2, ARM64 only | Deliver the existing ARM64 target to ARM64 Windows users; inspect `app/desktop` release target selection. | Same-source matrix reduced median group private commit by about 35 MB. This includes runtime/code/native-library differences as well as emulation. It is not an emulator-only allocation estimate. | Low application risk, separate distribution acceptance needed. Native image-run Desktop CPU was 2.72-3.00 s versus 3.56-4.39 s for x64. Validate setup/replacement and architecture selection. No saving is claimed for physical x64 Windows. |
| 3 | Stream neighbor-thumbnail encoded input in `live_images.go`; retain full selected-original semantics. | About 22 MB less Desktop private commit in the single control, but **no total-WS benefit established**. Targets temporary encoded-input copies and Go-heap span demand. | Medium risk: header replay, byte bounds, EXIF, corruption and cancellation need acceptance. CPU increased in the single control. Existing real-service image-preview E2E passed, including original bytes and EXIF. Repeat native performance runs and malformed/oversized/cancelled stream scenarios before adoption. |
| 4 | Add an upstream mygo D3D11 texture byte budget and wall-clock idle eviction with a grace period, keeping current-frame resources pinned. Location: pinned mygo `internal/gpu/d3d11/d3d11.go` sweep/present seam documented in the retained patch. | Obsolete image texture area differed by about 26 MB after selections. Process private commit differed by about 37 MB then, and about 10 MB after scrolling. Driver residency remains unknown. | Highest risk: partial redraws, resource reconstruction and upload latency. Aggressive eviction increased CPU by about 1-1.4 s. Do not ship per-frame immediate eviction. Require repeated scroll/select/hide/reopen, pixel equivalence, multi-window rendering and CPU/latency budgets on native x64 and ARM64 Windows. |

The admission candidate should precede a deeper Engine blob-streaming redesign.
Its narrower boundary can prevent concurrent allocations without changing content
ownership. A future lazy Engine representation could reduce per-request buffering,
but that benefit overlaps admission and is not quantified here.

The already merged event-driven rendering, viewport detail window, 32 MiB
thumbnail budget, serialized image decode, bitmap CPU-pixel release, 500-row
window and hidden-window Go release are retained. They are not new proposals.
No memory target below 100 MB is promised. A 16-million-pixel permitted original
alone has a 64 MB RGBA raster before codec scratch, reconstruction data and GPU
resources. Framework/OS costs and x64 emulation remain separate boundaries.

## Replay, validation and cleanup

Run the complete paired baseline through the [research harness](../../tools/research/windows-desktop-memory/README.md):

```sh
python3 tools/research/windows-desktop-memory/replay.py \
  --vm 'Windows 11' --repetitions 3 \
  --output .build/windows-memory-replay
```

For the native interaction and growth screen, select `--datasets images
--architectures arm64 --repetitions 1 --explore --cycles 10`. For the temporary
streaming control, add `--stream-neighbors` to a separate baseline-equivalent
invocation. The portable runner exports the immutable source, builds diagnostics,
initializes profiles explicitly, measures actual processes and removes temporary
source, executables, data and raw logs. No public listener is opened.

The temporary streaming patch passed canonical `make desktop-e2e
CASE=image-previews` in an independent detached worktree at the real base SHA.
The [compact receipt](windows-desktop-memory/stream-e2e-receipt.json) records
source/binary hashes, the race detector, PNG selection/original bytes, quick-panel
resolution, JPEG EXIF, later-frame CPU release, serial body reading and private
cleanup. This is process acceptance; Windows performance and native presentation
are separate evidence. No unit tests were added after implementation.

The retained admission surrogate also passed the canonical real-service
image-preview E2E; see [admission-e2e-receipt.json](windows-desktop-memory/admission-e2e-receipt.json).
No production package was installed. No production code, module selection or
runtime configuration was changed in this worktree. Diagnostic source exports
and the detached control worktree are task scratch only. The report distinguishes
private commit from resident pages throughout. Startup/interaction latency
percentiles and dedicated graphics-memory counters were not established; CPU
observations are cumulative process time, not user-response latency.

Final cleanup and replay validation are recorded in
[cleanup.json](windows-desktop-memory/cleanup.json) and
[replay-smoke-receipt.json](windows-desktop-memory/replay-smoke-receipt.json).
The end-to-end portable smoke command passed with actual Desktop PID 4356 and
child-service PID 1476; all replay output hashes were verified. Its runner removed
the guest and host staging directories and preserved its initial running VM state.
The enclosing research task then restored the original suspended state.
The document checker passes with seven pre-existing architecture freshness
warnings. No unrelated architecture page was marked verified by this research.

Only the English report, small aggregate receipts/tables, hashes and replayable
research sources/patches are retained. No installer, binary, database, personal
profile, source snapshot, screenshot or large raw log is submitted. To align this
research with the original observation, the remaining inputs are the actual Task
Manager column/process group, Windows architecture/DPI, and the triggering
history/view scenario.
