# Desktop window chrome

## Failure inventory

- An omitted setting enables the native title bar instead of defaulting off.
- Saving the setting loses other Desktop configuration or fails after restart.
- A saved change claims immediate application while the existing window is unchanged.
- Windows draws both native and custom caption buttons, or draws neither.
- Maximize cannot restore, Close bypasses the existing lifecycle, or a caption button starts dragging.
- Setup and startup-error pages have no usable window controls.
- Quick Panel inherits main-window decoration and loses its compact frameless behavior.

## Acceptance

Run the canonical `window-chrome` view case and the `window-chrome-settings` CLI
process case. The former checks platform window options and custom-caption actions
across light/dark views. The latter checks default, saved values, pending restart,
and persistence across a real service restart. Both produce source-bound receipts.
Software-view assertions do not establish native resize, drag or OS caption behavior.

## Implementation and verification

The host captures `desktop.system_window_frame` when creating the main window.
The default is false. General settings exposes the preference on all three platforms;
Windows uses native controls when enabled and a single shared toolbar otherwise.
The original verification below predates the single-row follow-up. The change preserves native resize
edges and the existing window-close lifecycle. Quick Panel remains unchanged.

- PASS: canonical `window-chrome` view case, including light/dark captions and
  minimize/maximize/restore/close dispatch. Final local report:
  `.build/desktop-e2e/20261120T091720Z-be83a352/index.html`.
- PASS: canonical `i18n` view case. Local report:
  `.build/desktop-e2e/20261120T091347Z-246e52aa/index.html`.
- PASS: real CLI/service `window-chrome-settings` process scenario, default,
  revision-checked save, restart reporting and persistence across service restart.
- PASS: localconfig race checks, scoped vet, pinned golangci-lint 2.15.0,
  and production builds for macOS arm64, Windows amd64 and Linux amd64.
- PASS: docscheck with seven architecture-staleness warnings.
- NOT RUN: native Windows caption actions/drag/resize and Linux/macOS native
  title-bar toggles. Software-view captures do not prove these OS behaviors.

The docscheck bump was attempted and refused uncommitted source paths. The
existing `verified` marker remains unchanged until the implementation is committed.
No application, browser or VM was opened for these checks. No commit, push or
publication was performed. Temporary cross-platform build programs were removed.

Repeat with:

```sh
TMPDIR=/private/tmp make desktop-e2e CASE=window-chrome
TMPDIR=/private/tmp make desktop-e2e CASE=i18n
TMPDIR=/private/tmp UNIC_E2E=1 go test ./e2e/cli -run '^TestCLI$/^window-chrome-settings$' -count=1 -v
```

## PR integration

Merged `origin/v2` at `fdf768d37`, including independent Desktop roles, into the
window-chrome branch. The GUI does not recreate the tray or apply the old hidden
startup policy. Close delegates to the new GUI-only shutdown handler. Upstream
mode/background restart semantics and all translations are preserved.

The merged source `4fbcd2329` passed the canonical `window-chrome` case, the real
CLI/service settings scenario, scoped pinned lint, and Windows amd64 build. The
architecture verification marker now names this committed implementation. Initial
reports above retain their original pre-integration source identities.

The canonical `desktop-processes` E2E passed after integration, including GUI
close/reopen, independent Quick Panel, concurrent activation and role recovery.
Report: `.build/desktop-e2e/20261120T092836Z-5f2fab51/index.html`.
Documentation checking after the marker update passed with six warnings on other
architecture pages.

## Borderless BoardUI follow-up (2026-11-21)

The user requested removal of system window chrome after inspecting the local
native preview. The implementation baseline is `78b9e662575e8a3d815065c5dc703d330ef724d5`.

Failure inventory before implementation:

- Hidden-titlebar mode still leaves native traffic lights or GTK caption buttons.
- A former native-button reservation leaves a redundant top strip.
- Hiding native controls leaves no usable Minimize, Maximize/Restore or Close.
- A fully frameless Windows window loses native top resize edges.
- Preview and production choose different decoration paths.
- Custom Close bypasses the existing GUI-role shutdown handler.
- The explicit Linux/macOS system-frame preference stops working.

Extend the canonical `window-chrome` E2E before changing runtime code. Require
custom controls on all platforms when the system-frame preference is off, retain
native resize edges, and click each caption action in both schemes. Keep the
first failure in a new ignored artifact directory; allow one diagnosed replay.
Build and restart only the owned native preview. Inspect that window, capture
its actual decoration, and retain a source/binary-bound local receipt. Native
Windows/Linux behavior remains NOT_RUN.

Follow-up results:

- The initial canonical view run failed on Linux's unexpected native buttons in
  `.build/boardui-borderless/red`. The source-bound `green` replay passed.
- `production-boundary` passed with both real command builds and the UI/core
  dependency checks. Quick Panel and the explicit native-frame setting are unchanged.
- The owned macOS preview was rebuilt and reopened. Its actual window capture
  `native-after.png` shows application controls instead of traffic lights. Its AX
  tree exposes Minimize, Maximize and Close. Native Maximize changed bounds from
  1100×720 to 1600×805 and changed its action to Restore. Restore returned exactly
  to 1100×720 at (250,52). The preview remains open for user inspection.
- The first window lookup immediately after spawning preceded application
  registration and returned APP_NOT_FOUND. A subsequent lookup by the exact PID
  found the expected frontmost window. There was no application crash or retry launch.
- These focused native observations do not establish physical edge-resizing,
  dragging, native Windows/Linux behavior or user visual approval.

The original wider migration screenshots and hosted test packages predate this
follow-up. They must not be presented as evidence of the new caption policy.

The macOS arm64 production build and Windows/Linux amd64 cross-builds passed.
Pinned lint 2.15.0 reported zero issues. Its first invocation failed while fetching
an automatically selected Go 1.26.11 toolchain (download EOF); the diagnosed
invocation explicitly used the existing Go 1.27.3 toolchain and passed. No source
change bypassed lint. Build hashes are retained in the ignored `builds.json`;
scratch build executables were removed, while the user's running preview remains.

## macOS correction (2026-11-21)

The preceding borderless follow-up misinterpreted the user's request. It is
superseded: macOS must retain real system traffic lights, hide only the native
title-bar presentation and draw its own title bar. Restore Linux's original
native caption policy and keep Windows's existing custom controls. Do not draw
replacement traffic lights.

Failure inventory before this correction: hidden native traffic lights; custom
Windows controls on macOS; native title text/background still visible; traffic
lights overlapping content; a 52-DIP reservation caused by the previous 19-DIP
vertical inset; preview differing from production; changing other OS behavior.

Extend the existing E2E policy checks first. Fix the macOS native control inset
at (14,9) DIP inside a 32-DIP application title bar. Preserve native resize edges,
window lifecycle and Linux/macOS's explicit system-frame preference. Keep the
first red artifact, run one diagnosed green replay, and inspect the owned native
macOS preview before delivering.

Correction results: the initial `window-chrome` policy E2E failed on hidden macOS
buttons (`.build/boardui-macos-titlebar/red`); the corrected replay passed. The
owned native preview now displays the real traffic lights over the compact
self-drawn title bar, with a centered application name. Its AX tree contains
native close/minimize buttons and no application replacement caption actions.
The actual window capture and binary-bound receipt remain in the ignored
`boardui-macos-titlebar` directory. The macOS production build passed. Lint's first
pass caught the runtime import grouping; the format correction passed pinned
lint with zero issues. No other runtime change followed native capture. Native
dragging/resizing, Windows/Linux execution and user visual approval remain
NOT_RUN. The live owned preview stays open for inspection.

The later [v1 unified-top correction](macos-titlebar-v1-reference.md) supersedes
the centered 32-DIP title row described above. Normal History/Settings now own the
window top; that fallback row is only for setup/startup.

## Windows single-row follow-up: failure inventory (before implementation)

- Windows hides the General frame switch or ignores a saved true value.
- The toolbar has a second caption row or shows duplicate native controls.
- At 600/640 logical pixels or scaled rendering, search overlaps history commands or caption buttons.
- Caption buttons lose their accessible names, order or maximize/restore state.
- Settings and plugin details lose controls after navigation.
- Setup, migration, loading and startup-error views lack a draggable fallback with controls.
- Recreating GUI retains a pending notice derived from an older daemon baseline.
- A saved change alters the running window instead of applying at next GUI creation.
- Caption Close stops the tray/service or restores the removed hide behavior.
- Drag hit testing consumes input, menus or button actions.
- A native-frame claim relies only on window-content capture.
- Acceptance reads personal data, changes another VM task, or leaves owned resources.

The first failing full-view run is retained. Follow-up native evidence must include
OS decorations, matching synthetic data and geometry, source/program hashes and cleanup.

## Windows single-row implementation

Base: fetched `origin/v2` and worktree HEAD both
`ccf947c9f9a8e8b759203632e0b33e6f44d6b2a0`. Worktree:
`308f87b7-f239-4f4f-9986-2f49eec7ce14`; branch:
`fix/windows-single-row-chrome`. The parent worktree is untouched.

The UI Kit SplitView owns a trailing toolbar slot. The GUI host supplies controls
from windowchrome. History, settings and plugin details share their existing
48-point toolbar with those controls. Setup/migration/loading/startup failure use
one fallback band. Native mode uses TitleBarDefault and draws no custom controls.
The GUI captures its frame preference once. Saved changes do not recreate the
running window. Its pending notice compares against that GUI's captured value;
the service no longer reports this host-only preference as a daemon restart.

### Local evidence

- RED: `.build/desktop-e2e/20261120T100419Z-f1a44d1b/index.html`.
  Complete views proved toolbar Y=32 and caption controls outside its band.
  The first harness build had an AccessAll naming typo; it was corrected before
  this behavioral run. That initial build failure is retained separately.
- RED: CLI/service expected no host-only restart entry and received
  `restart_required=["desktop.system_window_frame"]`.
- PASS: full-view/fallback replay,
  `.build/desktop-e2e/20261120T100912Z-9627eedd/index.html`.
  Complete history/settings/plugins views; 600/640/1100 logical widths;
  light/dark themes; scale 2; search expansion; shared-toolbar geometry;
  caption callbacks; welcome/import/failure fallback with actual setup renderer.
- PASS: CLI/service default/save/reload/restart persistence scenario.
- PASS: independent process regression,
  `.build/desktop-e2e/20261120T100641Z-c69fd001/index.html`.
- PASS: i18n replay,
  `.build/desktop-e2e/20261120T100711Z-607aaf17/index.html`.
- PASS: scoped vet, localconfig race checks, pinned golangci-lint 2.15.0,
  Windows amd64, Linux amd64 and macOS arm64 desktop builds.
- PASS: docscheck before commit/bump, with seven architecture staleness warnings.

### Native replay and boundary

Run `make desktop-e2e CASE=window-chrome-native` for 640/light and
`make desktop-e2e CASE=window-chrome-native-dark` for 600/dark. These canonical
cases verify UI saves, persisted TOML/API values, the unchanged running window,
two GUI recreations with the same tray/service, pending-state clearing, complete
history screenshots, plugin detail controls, maximize/restore/minimize OS state,
caption Close and tray reopen. Windows additionally retains exact-owned-window
OS captures and frame/client geometry from `windows_chrome.ps1`. Content recordings
remain separately identified as mygo captures.

The isolated catalogue uses application-scoped input. Physical empty-toolbar
movement, native edge resize and double-click maximize require explicit guest
replay of `windows_chrome.ps1 -Action drag-resize`, with the current owned PID and
exact binary from its role receipt. The helper refuses a mismatched executable
or unavailable foreground target. It never reads or writes a clipboard.

Additional canonical fallback cases are `window-chrome-setup` (welcome,
migration confirmation and synthetic passphrase pages) and
`window-chrome-startup-error` (a real service startup failure caused by an owned
directory at the database path). The sized native cases explicitly select the
platform skin; the older sized runner default would otherwise force macOS.

Native Windows ARM64 acceptance is complete. The final receipt below supersedes the earlier handover-blocked checkpoint. No Snap claim is made.

## Historical checkpoint: native handover blocked

Implementation source: `a0a0f5fb48f0117d2ca46447e5b7a6bf3dd5b906`.
At this checkpoint, the implementation and acceptance entry were committed locally.
Delivery was held because the concurrent VM task had not confirmed an exclusive handover.
The two coordination requests were sent at 10:02 and 10:33 UTC on 2026-11-20.
No guest command, application installation, VM start/suspend, personal clipboard
access or permission grant was performed by this task. The VM was initially
observed running; this task did not change its state.

The final committed-source software replay passed at
`.build/desktop-e2e/20261120T105500Z-2daac5c2/index.html`. The report includes
complete history/general checkpoints, setup fallback pages, geometry, assertions,
source-file digests and verified SHA256SUMS. Its private cleanup passed.
An exploratory assertion incorrectly expected the macOS-only setup branding row
on Windows (`20261120T105402Z-7ef91029`). The exact `Skin == MacOS` condition
identified the bad expectation. It was removed without renderer changes. The
following replay passed. The original behavior RED and this invalid-assertion
report are retained separately.

The compact [machine-readable receipt](desktop-windows-single-row.json) records
source identity, exact Windows x64/ARM64 PE hashes, local verdicts and cleanup.
These Windows programs were cross-built with `desktop_e2e` and `buildvcs=false`;
they were not executed. Native mode screenshots, OS button interaction, physical
drag/resize/double-click, current guest architecture and Snap remain NOT_RUN.
No software-view image substitutes for the missing OS-frame images.

Replay the four native cases after the VM handover:

```sh
make desktop-e2e CASE=window-chrome-native
make desktop-e2e CASE=window-chrome-native-dark
make desktop-e2e CASE=window-chrome-setup
make desktop-e2e CASE=window-chrome-startup-error
```

Use the existing owned-window PowerShell helper for explicitly authorized physical
drag/resize/double-click checks in that VM. Keep the same synthetic data, sizes and
light/dark conditions. Restore the handover state after checking and remove only
task-owned guest processes/profiles. No push, PR, merge, release or publication is
authorized or performed.


## Final Windows native receipt (2026-11-20)

The VM owner released exclusive use at 12:22:41 UTC. The received state was
`suspended`. This task resumed UUID `32affa85-5174-4d5b-ab6b-903a06d7fa9b` and
verified Windows 11 Pro 10.0.26200.9711, ARM64, console session 1, DPI 192 and a
3360 x 1624 physical desktop. The guest had no Go, usable Python or FFmpeg tools.
The host therefore cross-built the canonical `TestDesktopScenarios` package and
standard Go `test2json`. This was an execution adaptation, not the default guest
`run.py` path. The real tagged GUI, CLI and service remained unchanged. The
v1fixture compiler path prefix was mapped to the owned guest source directory;
fixture bytes and scenario assertions were unchanged. Windows ARM64 has no Go
race detector. FFmpeg/FFprobe 9.0.2 x64 ran under guest emulation; the application
and driver ran as native ARM64.

Executed source: `af1de54fb` (full identity is in the JSON receipt). Application
programs were built at preserved `b5e93fa8`; intervening changes only affected
acceptance code and documentation. Runtime case/helper hashes were independently
compared with the executed source before cleanup. The whole portable guest source
snapshot is not claimed to be immutable: runtime assets were updated explicitly
between diagnosed attempts. The final report records that distinction.

Final report: `.build/windows-chrome-resume/native-accepted/index.html`.
All five cases passed with real Go JSON PASS events, no skips, playable continuous
recordings and per-case cleanup PASS:

| Case | Coverage | Frames / seconds |
|---|---|---|
| window-chrome-native | 640/light, both modes, saved preference, two GUI recreations, cleared pending, caption actions, plugin details, caption Close and tray reopen | 74 / 26.32 |
| window-chrome-native-dark | 600/dark, the same assertions | 73 / 26.08 |
| window-chrome-native-input | Physical empty-toolbar drag, native edge resize, double-click maximize, caption Close and tray reopen | 18 / 7.72 |
| window-chrome-setup | First install, migration confirmation, synthetic passphrase fallback, maximize/restore/minimize; no service before consent | 22 / 8.64 |
| window-chrome-startup-error | Real owned database-path startup failure, fallback controls and maximize/restore/minimize | 12 / 5.52 |

The primary light pair is under `cases/window-chrome-native/`:
`custom-history-os.png` and `system-history-os.png`. Both use the same synthetic
history and 640 x 720 client viewport. The dark pair is under
`cases/window-chrome-native-dark/`, with a 600 x 720 client viewport. Each pair has
the same DPI and theme. Outer heights differ because native mode adds the system
title bar. OS PNGs use `System.Drawing.CopyFromScreen` over the exact foreground
window's DWM bounds. The helper places that full frame inside the work area
without resizing its client, so the taskbar cannot obscure its bottom edge.
The inspected PNGs contain the complete real application, frame and rounded
corners. The mygo recordings remain explicitly identified as content captures.

### Retained failures and corrections

No repeated run erased an earlier artifact. The local behavior RED remains
separate from these native driver failures. Each correction received at most two
verification attempts; unrelated later failures retained separate results.

- `native-first`: the Windows Ctrl+, input did not match the existing Cmd+,
  application binding. The scenarios now enter Settings through the real sidebar.
  This task did not expand its product scope to keyboard-shortcut changes.
- `native-retry1`: Parallels cancelled a child test after the launcher returned.
  The launcher now waits for the child and records its exit code.
- `native-retry2`: both preference round trips and OS caption states passed,
  then plugin navigation clicked before the new page had rendered. An explicit
  accessible-element wait fixed this race. The first pair also exposed taskbar
  occlusion; the OS helper now uses the work area before capture.
- `native-final`: physical input was refused by the catalogue's original guard;
  it now requires an explicit owned-window guest authorization. Setup exposed
  the host compiler path in v1fixture, fixed by compiler path mapping. Startup
  failure exposed an immediate click after restore, fixed by a stable maximize
  element wait.
- `native-verified`: physical OS checks passed but the close regression lacked
  its initial role checkpoint. Setup operations passed but recording attempted
  minimized content, which has no drawable size. A role checkpoint and one lock
  around minimize, OS verification and restore fixed these driver failures.
- `native-followup`: the earlier dark case passed before the taskbar correction.
  It remains separate from the final complete-frame pair.

The initial CPU WMI inventory and later CIM cleanup lookup timed out. A longer
encoded direct-process-query cleanup script also exceeded 55 seconds; its exact
blocking phase remains unknown. Short queries independently verified architecture
and display, then found no task applications or other PowerShell processes.
A short ownership-checked directory removal passed. Cursor restoration was read
back independently. These retained infrastructure failures involve no product
path. No process termination was needed. No personal profile or clipboard was
read. The first transport failures remain local evidence.

Physical window input is an `owned-window-guest` environment. Ordinary runs and
CI shards exclude it. Replay only after exclusive VM handover:

```sh
make desktop-e2e CASE=window-chrome-native
make desktop-e2e CASE=window-chrome-native-dark
make desktop-e2e CASE=window-chrome-setup
make desktop-e2e CASE=window-chrome-startup-error
python3 app/desktop/e2e/run.py run --case window-chrome-native-input --guest <VM-UUID>
```

The input case keeps the directory clipboard. It checks exact foreground PID and
executable before pointer delivery. Physical clicks on system-titlebar buttons, native tray icon clicks, Windows Snap,
cross-application paste, system clipboard delivery, other Windows DPI settings,
x64 native execution and user visual approval remain NOT_RUN for this task.
The tray IPC reopen and independent GUI/tray/service lifetime assertions passed.

No push, PR, merge or release was performed. Guest and host build resources are
removed after evidence collection. See the compact JSON receipt for the final
cleanup and restored VM state. PNGs, recordings and first failures remain only
in local ignored evidence directories. The committed receipt contains hashes
and verdicts, not databases, programs, source snapshots or large logs.


## PR 2281 CI navigation race

The first PR checks run `44762086384` failed only in Desktop shard 3/3, job
`136376960537`, artifact `14644081151`. The `window-chrome-native` case accepted
the sidebar toggle at 12660 ms, then looked up Settings at 12666 ms and found no
element. The retained failure tree subsequently contains that element. This is
an asynchronous rendered-tree transition, not a missing Settings entry. The
original source merge was `9abfdb91e056da2238dc704962f70254c3c56fb6` for PR head
`ddbbb7ca33a9c2b16dff125289dd94343c3d2fc1`. Original evidence checksums passed.

The light, dark and physical-input scenarios now wait for the Settings item
after opening the sidebar. They also wait for the actual General switch before
the final pending-notice absence assertion. Windows plugin navigation waits for
its sidebar item before clicking it. Existing persistence, frame, lifecycle and
OS assertions remain unchanged. No product code or global driver policy changes.

The original hosted report remains under
`.build/ci-2281-first/artifact/20261120T152038Z-0224b4a4/`. Verification uses the
canonical `window-chrome-native` case and all hosted PR Desktop shards. Keep the
first failure and allow at most two verification attempts after this repair;
investigate a recurrent failure before any replay.


## BoardUI branch integration (2026-11-21)

Merged `origin/v2` at `894790a3a` into `feat/boardui-mygo` at `8aeee53c4`.
The current BoardUI composition retains the macOS integrated 52-DIP top band.
Windows custom controls use the host trailing slot in that same band; native
frame preference and GUI-scoped pending state follow the incoming implementation.
The removed three-skin Kit remains absent. Search can shrink below its ordinary
minimum when host controls need space at 600 DIP.

Retained local canonical reports:

- `20261121T103957Z-230c78a0`: build failure from an unused import introduced while resolving acceptance conflicts.
- `20261121T104015Z-d9e179ad`: complete-view failure at 600 DIP; the search minimum pushed Close beyond the toolbar.
- `20261121T104127Z-13151919`: PASS after removing that host-slot search minimum; includes macOS integrated-top cases and complete caption interaction coverage.
- `20261121T104045Z-e5bfe306`: production-boundary PASS, including both real command builds.

Reports reside in ignored `.build/desktop-e2e/` directories with manifests,
checksums, exercised PNGs and cleanup results. The first failure was preserved;
two diagnosed verification attempts were used. The macOS production build and
initial docscheck passed. Native-window and physical OS interaction were NOT_RUN.
