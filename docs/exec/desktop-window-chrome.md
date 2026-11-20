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

Native Windows results are pending coordinated VM access. No Snap claim is made.
