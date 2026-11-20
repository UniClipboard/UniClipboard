# Windows Desktop memory experiment protocol

Pinned before the first measurement. Research only; no production optimization is applied.

## Isolation failure inventory

- An exact production Desktop reads the system clipboard even with a private profile. Hold the guest clipboard open without reading, emptying or writing it while that binary runs. Abort if the lock cannot be acquired. Always release it in `finally`. Clipboard-adapter readiness is therefore excluded from this measurement.
- A clipboard lock does not disable the shared-clipboard integration or change host clipboard bytes. Never send Copy, Paste or Apply commands. No initial clipboard payload enters the private service.
- A task process might inherit personal paths. Override UNIC_HOME, APPDATA, LOCALAPPDATA and USERPROFILE inside the experiment process only. Initialize each CLI profile explicitly with `migrate --initialize-empty`. Never import v1 data.
- An existing same-identity application could receive a second-instance activation. Enumerate Desktop/service processes first and abort on a non-task instance. Extract the installer payload without running the installer or changing registered installation identity.
- Native session 0 cannot establish interactive rendering. Run through `prlctl exec --current-user`, verify session 1, and verify visible target HWND, DPI and client bounds.
- A hidden native HWND can bypass application hide logic. Close with WM_CLOSE and reopen through the application's second-instance command; record these separately from ShowWindow controls.
- Memory counters have different meanings. Record resident total/private working set, private commit, reserved virtual space, GDI/USER counts and handles separately. VirtualQueryEx MEM_PRIVATE is not synonymous with Go heap or native heap. Never attribute the unexplained remainder to graphics.
- Sampling can wake or disturb a process. Keep frequency fixed, retain monotonic elapsed times, and exclude sampler memory from target metrics.
- Force-terminating an unrelated service is forbidden. Match executable path and process-start identity before task cleanup. Restore the VM to its initial suspended state.

## Prespecified schedule

- Same pinned binary, 1100 x 720 logical target, same guest DPI/theme and synthetic corpus.
- One-second sampling. Startup peak: first 10 s. Visible settled: 30-45 s. Hidden: 15-30 s after close-to-tray. Reopened: 15-30 s after second-instance activation.
- Three repetitions for principal empty/text/image comparisons where feasible. Ten hide/reopen cycles and 60 s final idle screen for growth; bounded-duration results do not prove absence of leaks.
- MB denotes 1,000,000 bytes. Retain raw byte counters in JSON/CSV.
- First attempt an exact-package baseline. If isolation invalidates it, retain that failure and label same-source diagnostic controls separately before ranking hypotheses. Instrumented or rebuilt binaries are never labeled as the exact downloaded artifact.

## Additional single-variable controls (declared before implementation)

### Stream neighbor image input

Prediction: bypassing `io.ReadAll` for thumbnail-only reads reduces temporary Go
allocations and visible private commit while keeping the selected-original path,
thumbnail output and native preview unchanged. The original baseline is retained.

Failure inventory: encoded-size limits can be bypassed by decoder early exit;
DecodeConfig can consume headers required for EXIF/orientation; corrupt/truncated
inputs can change error handling; PNG/JPEG metadata can be large; cancellation can
leave a reader/decoder slot retained; selected full-resolution reconstruction and
stored bytes can change; avoiding the raw buffer must not increase concurrent
image decodes. Replay the consumed header, keep the existing limited reader and
serialized gate, drain within that bound, and preserve the original selected path.
This is a temporary diagnostic patch, not an adopted implementation.

### D3D11 texture retention

Prediction: after selecting several distinct original images, the renderer can
retain obsolete textures until its frame-age sweep. Aggregate texture counts and
BGRA pixel-area estimates must rise while application-owned CPU pixels remain
bounded. A temporary immediate-unused-image sweep must reduce that retention
without changing native preview pixels.

Failure inventory: an unchanged visible image may not participate in a partial
redraw; evicting its texture requires correct reconstruction on a later redraw;
font atlases, canvases and current-frame textures must remain intact; reupload can
increase CPU and latency; frame-count aging and wall-clock aging have different
semantics; a texture-area estimate does not measure driver residency or VRAM.
Keep both original and instrumented baselines. Do not claim the aggressive control
is production-ready. No heartbeat, forced GC or functional reduction is allowed.

### Native interaction schedule

Before running the controls: measure grid for30s, list selected image for30s,
eight distinct visible-row selections with3s each, then30s idle. Scroll twelve
steps down and twelve up with1s observation after each step; idle for30s. Open the
real quick panel for30s, dismiss with Escape, and observe30s. The original-behavior
GPU trace additionally runs the ten5s-hidden/5s-reopened cycles and60s final idle.
The aggressive-sweep control runs the same interaction sequence without extra
hide cycles. All clipboard apply/copy/paste actions remain excluded.

The exact-program cross-check uses corrected service readiness and a target-only
native capture after the initial45s sampling window. Keep the clipboard busy
without reading/writing it. Exclude readings if the capture does not establish
loaded synthetic history. Busy-clipboard polling is a known condition difference,
not a claim of normal capture/paste acceptance.

### Content-request admission control

Before implementation: the service materializes retained history representations
before returning HTTP headers. Desktop's image decode gate is acquired after
`HistoryContent` has already opened the response. Prediction: reducing the
existing detail-read admission from4to1 lowers service-owned encoded-buffer and
Go-heap peaks, even though decoded-body concurrency was already one.

Use a one-line temporary `detailReadLimit=1` control with the original image
loader. This is an attribution surrogate, not the proposed production policy;
a production change should gate image requests before opening the response while
preserving text concurrency and selected-item priority.

Failure inventory: slower text prefetch; selected-image starvation behind pending
neighbors; cancellation retaining a service lease or slot; two independent live
windows exceeding a global image bound; different queue order affecting latency;
and a lower peak caused by incomplete previews. Require completed native previews
and the existing real-service image E2E. Do not present serializing every detail
as the final optimization.

### Native probe correction

The first GPU interaction attempt is PARTIAL. Opening Quick Panel from the More
menu produced created-but-hidden native windows, so the visible-window assertion
failed. Its earlier selection/scroll counters are retained as diagnostic context,
not completed lifecycle acceptance. The corrected route uses the existing Settings
button and records visibility explicitly. It returns to History before hide cycles.

The first probe used mygo CapturePage, which re-renders native content and restored
3.19 MB of bitmap CPU pixels. The corrected pair uses OS CopyFromScreen of the
verified foreground client rectangle. This reads only the synthetic target window,
does not re-render its content, and keeps screenshots out of the submitted artifacts.

### Supplementary probe outcome

A Quick Panel-only follow-up attempted a GUI-subsystem build of the automation
CLI to remove console-focus interference. PowerShell did not synchronously consume
its session output: subsequent commands reported a missing session and a closed
stdout pipe. The follow-up failed before Quick Panel acceptance and is excluded.
The portable runner retains the original console automation CLI. This failure
does not identify a product Quick Panel defect. The completed paired run records
actual visibility; only its matching selection/scroll prefix supports paired
claims. Six auxiliary window modes were scheduled for 45 seconds each, with
30-45-second settled sampling, using one v2 probe executable and no added GC.
