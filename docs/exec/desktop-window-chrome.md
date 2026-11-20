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
The default is false. Linux/macOS expose the preference in General settings;
Windows always uses custom caption actions. The change preserves native resize
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
