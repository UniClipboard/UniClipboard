# Independent quick-panel memory

## Source and scope

Worktree: `92fff632-8c13-466c-9df4-aa0259ab3116`.
Branch: `fix/quick-panel-memory`.
Implementation candidate: `9caec3a031f5ff195fdd7c6dce6464b4e21e476d` (local; delivery HOLD).
Fetched origin/v2 baseline: `ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0`.

GitHub metadata independently confirms that [Build CLI run 44713097523](https://github.com/UniClipboard/UniClipboard/actions/runs/44713097523)
and artifact `14618246611` (`desktop-test-windows-amd64`) use that SHA. Its
artifact digest is `sha256:77d23cb6b377d85600a3c3b4437da012806195a148fa2a08f4222183d8be8b3b`.
The artifact was not installed or treated as a diagnostic program. Native task
builds have their own frozen-source and executable hashes.

The previous shared image admission and streaming-thumbnail changes already
reach quick-panel images through `newLiveQuick`, `liveState.loadDetail`,
`loadSearchEntry` and `readPreviewImage`. Their process-wide admission limit is
per OS process. The old GUI/service memory matrix did not open an independent
quick panel and is not independent-panel evidence.

The supplied Task Manager column and exact user dataset are unavailable.
Matched synthetic results must not be presented as the exact GUI 60 MB versus
panel 100+ MB observation. All four roles are measured independently.

## Implemented change

Selected originals still used geometrically growing whole-input buffers, although
the local response already supplies its bounded length. The final change reserves
that encoded input once with the existing standard bytes.Buffer. A fixed64KiB
io.CopyBuffer limits individual read requests; a writer-only wrapper prevents
Buffer.ReadFrom from turning reserved capacity into a multi-megabyte request.
Unknown, zero and over-limit lengths retain the bounded fallback. The actual
32MiB encoded limit,16-million-pixel check, EXIF, exact reconstruction bytes,
admission ownership and cancellation remain authoritative. Neighbor thumbnails
keep their existing streaming path. No library, configuration or forced GC is
added. GUI and independent panel both reach this shared production code.

The stronger reopen test also exposed an existing projection defect. applySearch
cleared panel rows, but a same-query service refresh correctly reused its
unchanged history revision. No new result serial arrived to rebuild those rows.
The panel now invalidates its own projection serial when clearing its rows. The
service keeps its revision/cache optimization. The matched memory control has
that same correction, so it compares real reopened content against real content.
Only selected-original input handling differs within each final source pair.

The independent tray/GUI/panel/service architecture, native window lifetimes,
caption layout, adapter/shortcut ownership and command controller remain intact.
No second GUI, adapter, service or subscription is introduced. Memory size alone
is not evidence of a leak. NewDesktop supplies a reusable live binding to panel;
it does not create a main GUI window in that role.

## Test-first evidence and rejected controls

The [failure inventory](../../tools/research/quick-panel-memory/FAILURES.md)
precedes product edits. All regressions use canonical real-service/native cases;
no unit test was added after implementation.

A whole-window/live-binding retirement control was rejected: its initial hidden
saving did not persist consistently through cycles and original readiness gained
roughly300-700ms. A satellite-only control reduced native window resources but
failed to establish lower total memory after repeated reconstruction. Those
changes were restored to the exact v2 source. Their passes do not count as final
memory implementation acceptance. Retained first failures include native runs
20261120T101113Z-0dae6c3d and20261120T102526Z-772939f9.

The new native reopen test then reproduced the projection defect in
20261120T102704Z-a04d346b: Copy succeeded; search survived; the reopened list was
empty. The final native regression checks two rendered satellite frames, both
exact directory-board copies, retained search, hide/same PID, GUI exit/recreation,
tray/service continuity and cleanup. Its continuous recording is application
native evidence, not physical input.

Selected-original allocation failed in20261120T104807Z-02a83f33:
18,332,272 bytes exceeded the15,733,612-byte budget for a3,148,214-byte encoded
PNG with a4,194,304-byte raster. Reserving the input first passed in
20261120T104926Z-084bb4a0 at10,568,520 bytes. These are TotalAlloc deltas with
other views cancelled, without forced GC. They are not process working-set
reductions. Unknown, low, high and extreme length hints preserved decoding.

The reservation-only native control showed a visible-memory benefit but higher
startup working-set peaks and CPU. A new real-service read-request assertion
failed in20261120T110845Z-4006fa00: the largest request was3,153,920 bytes.
The final64KiB copy path passed the full image case in
20261120T110957Z-ad20151b:10,634,560 allocated bytes,65,536-byte largest request,
386 body reads. Original bytes/dimensions, EXIF, malformed/oversized inputs,
queued/active cancellation, priority, shutdown and service restart also passed.
The lower request size is established; Windows overhead must be judged through
the separate native cohort, not inferred from this counter alone.

Final-source native-process and quick-panel-contract cases passed in
20261120T111318Z-7b84432a and20261120T111320Z-f9f0b86c. Pinned lint2.15.0 reports
zero issues. Later final performance/native receipts bind the exact accepted
source files and program hashes separately from these host correctness runs.

## Measurement protocol

Use the [replay](../../tools/research/quick-panel-memory/README.md).
The existing Parallels guest is `32affa85-5174-4d5b-ab6b-903a06d7fa9b`, Windows
11 ARM64 build 26200, initially suspended. The busy-session observation showed
only this task. Earlier native work had handed over the suspended guest.
ARM64 programs execute natively; amd64 programs use Windows x64 emulation.
Neither establishes physical x64 Windows performance or emulator-only overhead.

Every cohort freezes source and uses identical non-race flags, pinned dependencies,
private initialized profiles, Windows skin/light theme, telemetry/sync off and
synthetic content. Images use thirty text entries and twelve seeded 2048x1536 PNGs.
Empty/text controls use zero/thirty entries. The main GUI viewport is 1100x720
logical units; the history panel is 380x366 and image satellite 400x310.
No personal profile, credential or clipboard content is read or written.

The driver resolves actual GUI, quick-panel, tray and service PIDs, executable
hashes, process creation times and runtime architecture. Role endpoints share
neither PIDs nor automation window IDs. Same-sample group counters cover only
those live roles. Total WS sums may double-count shared pages. MB is decimal;
MiB is 1048576 bytes. Private commit, private WS and WS remain separate.

Visible measurements precede app-scoped captures. The driver also measures GUI
exit, explicit panel hide and three hide/reopen cycles. Stable statistics use the
last half of each declared phase, after the three-second grace period for hidden
phases. One-Hz sampled peaks are lower bounds on instantaneous peaks. Go metrics
and bitmap/cache aggregates use the same guest UTC clock; native counters remain
separate from Go allocation. GDI/USER/handle counts and address-space regions do
not establish driver residency or native-heap ownership.

Readiness is private activation to observed search controls and model-original
readiness. It is not physical-key-to-pixel latency. Native frames separately
verify eventual rendering. The cold first opening and warm reopen observations
are separate. CPU means process CPU seconds, not wall time. A twenty-percent CPU
or hundred-millisecond readiness increase requires investigation before acceptance.

## Results and delivery boundary

**HOLD for the sustained process-memory goal.** This local candidate proves fewer
selected-original allocations. It does not prove lower resident/committed panel
memory. Do not publish it as a solution to the reported 100+ MB panel. The x64
post-selection hidden regression remains material and unresolved. No more retries
were used to discard that result. A subsequent lifecycle change needs a separate
contract decision and fresh test-first acceptance.

Six alternating image runs per architecture completed (three matched pairs).
One empty/text pair each completed on ARM64. All sixteen runs passed actual-role,
content, source identity, native crop-equivalence and owned cleanup checks.
Medians below are medians of run medians. MB is decimal. Full ranges and counters
are in the [aggregate comparison](evidence/quick-panel-memory/comparison.json) and
[per-run measurements](evidence/quick-panel-memory/measurements.csv).

| Architecture / phase | Panel private commit MB, before → after | Panel private WS MB, before → after | All live roles private commit MB, before → after |
|---|---:|---:|---:|
| arm64 / panel-with-gui | 135.76 → 133.15 | 85.49 → 82.58 | 452.99 → 447.89 |
| arm64 / panel-gui-exited | 131.93 → 133.03 | 81.49 → 82.47 | 283.10 → 284.61 |
| arm64 / hidden | 121.97 → 126.78 | 71.50 → 76.28 | 272.54 → 276.61 |
| arm64 / selection-3 | 148.20 → 148.73 | 97.86 → 96.14 | 301.94 → 300.49 |
| arm64 / selection-final-hidden | 142.95 → 145.33 | 92.65 → 94.98 | 296.46 → 297.11 |
| amd64 / panel-with-gui | 162.07 → 163.46 | 106.66 → 108.38 | 574.57 → 571.53 |
| amd64 / panel-gui-exited | 163.52 → 166.84 | 107.99 → 111.06 | 374.94 → 380.94 |
| amd64 / hidden | 147.02 → 150.35 | 91.47 → 95.33 | 359.59 → 364.07 |
| amd64 / selection-3 | 198.50 → 200.80 | 143.15 → 145.50 | 406.03 → 411.25 |
| amd64 / selection-final-hidden | 164.42 → 200.61 | 109.32 → 145.49 | 375.10 → 411.26 |

The all-role sum excludes GUI after it exits. It does not assign another role's
memory to the panel. WS is also recorded separately; summed WS can count shared
pages twice. No native/GPU byte total was measured. GDI/USER/handles are resource
counts, not GPU-memory attribution.

| Architecture | Cold panel peak WS MB, before → after | Panel CPU seconds through final hidden, before → after | Selection-to-model-original median ms, before → after |
|---|---:|---:|---:|
| arm64 | 152.92 → 146.96 | 6.52 → 7.06 | 308 → 394 |
| amd64 | 169.91 → 170.81 | 11.53 → 12.31 | 444 → 387 |

ARM64 CPU rose 8.4%; its selection model readiness rose 86 ms. x64 CPU rose 6.8%.
These increases stayed below the prespecified investigation thresholds, but do
not cancel the sustained-memory failure. They are model/IPC measurements, not
physical shortcut-to-pixel or paste latency. See the aggregate timing ranges;
short five-second phases can have no five-second Go sample and report null.

The ARM64 empty-data panel commit was 67.50 → 67.05 MB. Text-data commit was
74.83 → 74.57 MB. They have one pair each and establish controls, not a statistical
improvement. Both original-input buffering variants already use the earlier
shared image optimization. The native images were identical in all paired center
crop checks. The final selected-original allocation probe improved from
18,332,272 to 10,634,560 bytes (42.0% fewer); this is a separate claim.

Live-binding counters remain bounded at twelve image owners, one original,
eighteen details, zero in-flight reads and nineteen panel goroutines at final
hidden. Their counts do not grow across the measured cycles. The x64 candidate's
final hidden Go heap allocation median was 46.87 MB versus 33.76 MB in the control.
Heap spans and released pages also differ. This is insufficient to identify the
36.19 MB private-commit increase as either a Go leak or a graphics leak. The
remaining process bytes must not be labeled GPU residency by subtraction.
mygo's hidden windows retain their renderer and the panel retains its live cache;
`Window.Hide` only hides the native window. The empty/text controls establish a
substantial baseline unrelated to large original input.

The host is ARM64. The verified loaded PE images distinguish native ARM64 from
x64 emulation. IsWow64Process2 returned UNKNOWN for x64 here, which does not
make its x64 PE an ARM64 image. Microsoft documents that [x64-on-Arm has no WOW64
layer](https://learn.microsoft.com/en-us/windows/arm/apps-on-arm-x86-emulation).
Keep the [raw API meaning](https://learn.microsoft.com/en-us/windows/win32/api/wow64apiset/nf-wow64apiset-iswow64process2)
and the source-bound PE/executable identity separate. Do not attribute the observed
cost solely to emulation or extrapolate to a physical x64 host.

## Additional validation and cleanup

- PASS: canonical image-previews, desktop-native-processes and quick-panel-contract
  cases listed above; host race builds, native windows, exact directory-board Copy,
  retained search and repeated previews. Windows ARM64 Go race is unsupported.
- PASS: Windows native lightweight and full matrix. Full03 used one real global
  shortcut with GUI absent, real tray clicks, exact HWND/PID checks, Escape focus
  restoration, independent/owned services and Quit/close/reopen lifecycle.
  [Full receipt](evidence/quick-panel-memory/run-full-03.json).
- FAIL retained: Full02 decoded the Go helper's UTF-8 output with Windows' console
  encoding and corrupted Chinese JSON. Explicit UTF-8 stream decoding in the
  existing driver fixed the harness. Full03 used a fresh private root and passed.
  [First failure](evidence/quick-panel-memory/run-full-02.json).
  Inspect setup also encountered execution-policy invocation and a missing helper
  source file before input. Only process-local Bypass was used; no policy changed.
- PASS: pinned lint 2.15.0; go mod verify; uninstrumented Desktop production builds
  on macOS arm64 and Windows arm64/amd64. Diagnostics use non-race programs;
  correctness uses race where supported.
- NOT RUN: physical x64 Windows, actual cross-application clipboard/paste insertion,
  user visual approval, user's exact dataset/Task Manager column, and the referenced
  package installation. Native automation acknowledgement is not paste insertion.
- PASS: each run removed owned guest profiles/processes. Final cleanup removed
  twenty explicitly named task stages and restored the exact guest to its initial
  suspended state. [Cleanup receipt](evidence/quick-panel-memory/cleanup.json).

[Source/program/role identities](evidence/quick-panel-memory/identities.json),
[render hashes](evidence/quick-panel-memory/frame-hashes.json) and
[checksums](evidence/quick-panel-memory/SHA256SUMS) bind the small retained evidence.
Native frames/recordings and first-failure raw artifacts stay in ignored `.build`;
programs, databases, source exports and raw logs are not submitted. No push, PR,
merge, package installation or publication occurred.

The proposed next step is to retire an idle hidden panel process after operations
settle and let the tray recreate it. The shortcut/lifecycle paragraph of the [quick-panel
contract](../contracts/desktop-quick-panel-contract.md) currently requires it to
remain hidden between requests. A decision was requested before changing that
contract. The contract and current role/window lifetimes remain unchanged here.
