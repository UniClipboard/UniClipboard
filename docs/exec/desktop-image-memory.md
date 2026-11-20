# Desktop image memory implementation

## Scope and identity

The user approved production image-request admission and streaming thumbnail input.
The starting product source is origin/v2
173d362036ae4a746878306915c85bcdf7468751. The completed
[Windows research](windows-desktop-memory.md) is preserved in local commit
e8d764af5. The implementation branch is fix/desktop-image-memory.
The independent lightweight-process branch is not included. WM_CLOSE hides this
version's GUI; it does not exit it. No push, PR, merge or release is authorized.

The [protocol](desktop-image-memory/protocol.md) records failure modes, test-first
acceptance and performance investigation thresholds before product edits.

## Behavior

One shared image request queue admits content before the service can materialize
a response. Selected originals precede queued neighbors. Cancellation removes a
queued waiter; a race with slot transfer returns that slot. Response close occurs
before releasing admission on every content/error path. A view with four pending
neighbor jobs cancels a neighbor so the new selection can take the next job.
Text and directory reads keep their separate existing bounds. Single-file previews
share image admission because their MIME/name arrives with response headers.

Thumbnail decoding streams bounded encoded input and replays only inspected image
headers. The existing 32 MiB encoded and16-million-pixel bounds remain. The header
uses that same byte bound; the extra overflow byte is enabled only after header
inspection. Draining the bounded remainder catches codecs that stop before an
oversized tail. Cancellation checks cover reads and phase boundaries. The codec
and resize functions remain bounded calls rather than preemptible operations.
Original selection, EXIF orientation, reconstruction bytes, shown-pixel release,
preview completeness and the 32 MiB thumbnail reservation remain unchanged.

## Test-first evidence

Canonical `make desktop-e2e CASE=image-previews` reproduced both defects twice
before product changes. Runs20261120T055308Z-798a9f23 and
20261120T055405Z-7a3b94f9 observed four simultaneously open image responses and
a largest thumbnail input read of1,277,952 bytes while serial body consumption
still passed. Selected priority and queued cancellation also failed.

Admission-only run20261120T055647Z-5d129a98 passed request admission, selected
priority, queued/active cancellation, text independence and actual service
stop/restart recovery. Its remaining failure was the intentionally red streaming
assertion. The combined run20261120T055807Z-b00292a7 passed with the race detector:
one open response, zero remaining responses, five requests opened after excluding
the canceled waiter, and largest streamed read30,047 bytes. PNG, JPEG EXIF, selected
original bytes/dimensions, Quick Panel and later-frame CPU release passed.
Malformed/truncated/oversized-header/huge-raster/trailing-over-limit/read-error
inputs and cancellation were rejected; the next valid request succeeded.

A further test-first refinement covered a selected item whose thumbnail was
already cached while four neighbor jobs occupied the view. Run
20261120T061011Z-45c10e90 failed because detailBusy was false during an original
upgrade. The scheduler now tests the required resolution tier independently of
that display flag. Run20261120T061154Z-345d068e passed the full race-instrumented
case, including the selected request taking the next response slot. Prepared
binaries from before this refinement were discarded without running in Windows.

No unit test was added after implementation. The acceptance additions drive the
production view/content path and a real separate service through the canonical
runner. Native Windows cost, native pixels and software-view correctness are
separate evidence.

## Bounded-input CPU refinement

The first native cohort at e540ad3aa was stopped after its first combined ARM64
run exceeded the prespecified CPU investigation threshold:5.016 group CPU seconds
by 45s versus3.609 in the matched baseline. Streaming-only used7.062 seconds.
All five completed exploratory runs, native pixel hashes and interruption/cleanup
recovery are preserved in [unbuffered-native](desktop-image-memory/unbuffered-native/measurements.json).
They are not a completed3-repetition final performance matrix.

A test-first real-service PNG resource check observed482 small body-read requests.
The first total-call assertion was too strict because IPC can return short reads;
its failed result is retained. A64 KiB standard bufio.Reader over the same shared
limiter separates codec request sizes from socket reads. Header replay, decode and
tail drain all consume that same buffer, preserving byte accounting. The corrected
resource assertion counts requests smaller than64 KiB rather than assuming every
read fills its buffer. The full canonical race case passed in
20261120T063709Z-39a67bd9, retaining total reads and largest request separately.
No whole-input thumbnail buffer or new configuration is introduced.

## Native measurements

All 30 planned non-race runs completed: three repetitions of each declared case.
The product source is **ea91a8f6693860dd3af2859222ca1f3c4b4ffe03**.
The comparison baseline is173d362036ae4a746878306915c85bcdf7468751.
[Build identities](desktop-image-memory/native/build-identity.json) bind all
production file hashes, diagnostic sources, flags, dependencies and program hashes.
[PE metadata](desktop-image-memory/native/program-architecture.json) independently
confirms0xaa64 ARM64 and0x8664 x64. Every same-architecture variant uses identical
service and CLI executables. The baseline executable also matches the exploratory
cohort's baseline; CPU variation between those cohorts is not a code change.

Windows 11 build 26200 ran on the existing ARM64 Parallels VM
32affa85-5174-4d5b-ab6b-903a06d7fa9b,4 logical processors,6144 MiB configured RAM,
6435831808 bytes reported physical RAM,4096-byte pages and interactive session 1.
DPI 192 produces a2200x1440 physical client from1100x720 logical units. Windows
skin, light theme, telemetry off and automatic sync off were identical.
The private database contains60 text entries, plus twelve2048x1536 PNGs for the
image case. The initial50-row catalogue includes all twelve images;21 detail
entries and one original tier remain cached. The retained fixture manifest binds
the approximately 9.958 MB encoded size of **each** PNG. No personal history or
clipboard bytes enter these fixtures.

Each run sampled1 Hz for 45s visible,30s hidden and 30s reopened. Stable windows
were30-45s and15-30s respectively. A third image selection then required a full
original and a 10s checkpoint. Native crop hashes match for all36 image captures
(initial and changed selection, within each architecture). Both captures are
2200x1440 at DPI 192. A reviewed native frame established actual synthetic image
content; the selected2048x1536 original and crop remain equivalent across variants.

### Image memory

Values are medians of three run medians, with ranges in parentheses. MB is decimal.
Group counters pair same-sample GUI/service values; group medians need not equal
the sum of separately reported process medians. Total working-set sums can count
shared pages twice and are never the headline saving. Private commit and private
working set are distinct.

| Architecture | Variant | Visible group private WS, MB | Visible group private commit, MB | Hidden group private commit, MB |
|---|---|---:|---:|---:|
| ARM64 native | baseline | 221.75 (221.22-221.90) | 319.73 (319.70-320.50) | 251.15 (250.34-252.71) |
| ARM64 native | admission | 146.37 (145.11-147.49) | 245.21 (243.55-245.37) | 176.03 (162.06-179.77) |
| ARM64 native | streaming | 200.63 (183.81-201.87) | 299.21 (281.97-300.76) | 251.60 (232.69-251.88) |
| ARM64 native | combined | 130.34 (123.14-130.91) | 228.44 (221.67-229.40) | 180.01 (175.21-180.05) |
| x64 emulated | baseline | 247.92 (242.86-250.83) | 353.68 (351.26-357.13) | 284.20 (283.52-287.83) |
| x64 emulated | combined | 151.23 (149.46-170.39) | 262.72 (257.34-277.75) | 208.97 (197.83-227.86) |

The independently measured combined reduction is 91.41 MB private WS /91.29 MB
private commit on ARM64 (41.2% /28.6%), and96.69 /90.96 MB on x64 emulation
(39.0% /25.7%). Do not add the admission-only and streaming-only deltas.
The admission effect is concentrated in the service; the streaming effect is
concentrated in Desktop. The exact process counters and Go heap/span/released
aggregates are in [measurements.json](desktop-image-memory/native/measurements.json)
and [CSV](desktop-image-memory/native/measurements.csv).

| Architecture / variant | GUI private commit, MB | Service private commit, MB | Reopened group private commit, MB |
|---|---:|---:|---:|
| arm64 / baseline | 169.08 (167.92-169.42) | 151.42 (150.31-151.78) | 252.39 (251.60-253.51) |
| arm64 / combined | 148.55 (146.44-150.70) | 78.70 (75.22-79.90) | 164.51 (163.52-167.50) |
| amd64 / baseline | 189.25 (185.83-201.38) | 167.82 (149.86-167.83) | 285.29 (284.33-288.73) |
| amd64 / combined | 166.17 (165.52-181.08) | 96.67 (91.82-96.70) | 199.54 (196.56-212.50) |

The group sampled full 45s private-commit peak median fell from320.13 to 241.55 MB
on ARM64 and357.82 to 273.51 MB on x64 emulation. The JSON preserves each run's
first 10s sampled peak separately. These sampled peaks are lower bounds on
instantaneous peaks. They are not Go heap measurements. Native heap ownership,
complete driver residency and dedicated graphics memory remain unknown.

### CPU and latency

| Architecture / variant | Group CPU seconds by visible checkpoint | UI-observed original selection, ms | Process start to first original, ms |
|---|---:|---:|---:|
| arm64 / baseline | 4.39 (3.25-6.22) | 178.00 (141.00-218.00) | 1497.26 (455.87-2084.81) |
| arm64 / admission | 3.62 (3.22-4.02) | 161.00 (129.00-195.00) | 621.53 (420.36-1762.07) |
| arm64 / streaming | 3.17 (3.09-6.47) | 209.00 (156.00-214.00) | 961.78 (468.74-2071.18) |
| arm64 / combined | 3.03 (2.94-4.66) | 233.00 (154.00-265.00) | 804.25 (436.97-1430.00) |
| amd64 / baseline | 4.80 (4.80-5.86) | 187.00 (167.00-260.00) | 829.65 (825.34-1508.06) |
| amd64 / combined | 5.00 (4.23-5.69) | 202.00 (201.00-387.00) | 867.32 (615.34-1339.98) |

Combined ARM64 CPU median decreased30.96%; x64-emulated CPU increased4.23%.
Selection medians increased 55 ms and 15 ms respectively. No final comparison
exceeded the prespecified20% CPU or 100 ms selection investigation thresholds.
The command-to-observed-ready timer is retained separately and includes automation
startup/polling. UI readiness is a model/bitmap event before presentation; it is
not physical-input-to-pixel latency. Native captures establish eventual visible
pixel equivalence. Startup and CPU ranges show environment/GC variation; these
are three-run observations, not universal performance guarantees.

### Empty/text controls and lifecycle boundary

| ARM64 dataset / variant | Group private commit, MB | Group CPU seconds |
|---|---:|---:|
| empty / baseline | 131.62 (129.15-131.83) | 1.25 (1.19-1.50) |
| empty / combined | 131.14 (130.80-132.46) | 1.41 (1.36-1.52) |
| text / baseline | 147.40 (147.15-148.93) | 1.33 (1.12-1.73) |
| text / combined | 147.52 (147.29-147.55) | 1.48 (1.23-1.77) |

The empty/text private-commit ranges overlap. No material constant memory increase
is established. Their CPU medians increased 0.156s each (12.5% empty,11.8% text), within the
investigation threshold.

At this immutable source, `unic-desktop.exe` owns GUI, tray and clipboard adapter;
`unicd.exe` owns Engine/history/sync. Actual PIDs, start times, executable paths and
parent links are retained per run. WM_CLOSE hides the existing GUI HWND and keeps
both PIDs alive; second-instance activation reopens that same window. This is
**not GUI process exit**. Quick Panel is a window in that GUI process and was not
opened in this memory matrix; its content behavior is covered by real-service
acceptance. Independent tray/GUI/Quick Panel role memory is **NOT_RUN** because
the separate process branch was not merged or mixed into this worktree.

The x64 program runs under ARM64 Windows emulation. Its result is not a physical
x64 Windows result and does not isolate emulator-only memory. The user's original
Task Manager column, grouping and device workload remain unavailable; these
matched synthetic results must not be labeled a reproduction of that exact view.


## Validation and cleanup

The final canonical image-previews, incremental-browsing and data-binding cases
passed with the race detector. Their product file hashes exactly match the native
combined build and immutable implementation. Pinned golangci-lint2.15.0 reports
zero issues. Production Windows ARM64 and x64 builds passed; program hashes and
flags are retained in [build-checks.json](desktop-image-memory/build-checks.json).
No production installer was run.

The [Windows process receipt](desktop-image-memory/windows-correctness/receipt.json)
passed the same real-service image assertions with software views. Queued and
active cancellation each measured 0 ms at millisecond resolution; response ownership
peaked at one, the cancelled waiter opened no response, and zero responses remained.
The next selected cached thumbnail won its original-upgrade slot. Original bytes,
EXIF, malformed/oversized/truncated streams, shown-pixel release, HTTP errors and
actual service stop/restart recovery passed. Windows streamed PNG reads numbered51,
with one small request and a65536-byte maximum. This is actual Windows IPC/codec
acceptance, not native physical-input acceptance. Windows ARM64 race is unsupported;
correctness race evidence comes from the canonical host runs.

Initial Windows-helper attempts failed before assertions: one fixture-source seam
matched both first start and restart; another Parallels command transport did not
complete. No test process or initialized fixture directories were observed during
the stalled command. The task transport was terminated, its resources were cleaned
and the VM was resuspended. The successful replay uses a short command invoking a
task script file and normalized Windows paths. These harness failures and cleanup
are retained separately; they are not product test passes.

Both native and Windows-process runners removed their task programs, profiles,
images, source exports and raw captures. The VM was verified suspended and released
to the other native task at07:57 UTC; no further VM operation is part of closure.
Clipboard-content reads/writes never occur. Windows correctness sequence numbers
were30 before and after. See [validation and cleanup](desktop-image-memory/validation-and-cleanup.json).
No signed package, publication, push, PR or merge was performed.

Replay the full source-bound performance matrix using the
[implementation harness](../../tools/research/desktop-image-memory/README.md):

```sh
python3 tools/research/desktop-image-memory/replay.py all \
  --stage "$HOME/.cache/unic-image-memory-replay" \
  --output .build/desktop-image-memory-replay --vm 'Windows 11'
```

Acquire an exclusive VM handoff before running. It takes about one hour. The
separate Windows correctness command and prerequisites are in that README.
The final SHA256SUMS binds report, small JSON/CSV receipts and replay sources.
No binary, database, screenshot, source export or large raw log is submitted.
