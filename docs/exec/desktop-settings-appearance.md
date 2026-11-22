# Desktop appearance settings

- Baseline: `fcf5611662039c1db79fc6537c4f79aff2a5b436`.
- Scope: the first complete D-group path, persisted color mode.
- Work agreement: [parallel settings plan](desktop-settings-parity.md).
- Status: persisted color mode implemented; canonical nonvisual production-process acceptance passed. Native appearance acceptance remains not run.

## Failure inventory

1. A profile without an explicit color mode has no effective default.
2. The service accepts an unsupported mode or loses the last valid mode.
3. A stale settings save overwrites an external edit.
4. Saving appearance resets unrelated telemetry, sync or device preferences.
5. An external edit reaches disk but not the active configuration.
6. Restarting the service loses the saved value.
7. Production configuration projection ignores a successful save.
8. GUI, setup and Quick Panel use different color sources.
9. Returning to system mode retains a previously forced scheme.
10. A color change resets editor contents, selection or async state.

The nonvisual process case covers 1–7. Host wiring must use the existing mygo
theme module for 8–9. Cases 8–10 require separately authorized native or view
acceptance before claiming complete visual behavior.

## Existing library capabilities

Pinned mygo is `v0.8.4-0.20261108082913-9233663736ca`.
Its `Theme.SetSource` supports the existing `system`, `light` and `dark` sources.
The host applies the source; native BoardUI keeps its single source-bound
presentation and reads the effective scheme. Public controls do not own saved
preferences.

`ui.Context.SetZoom` is a maintained content-zoom API. It updates layout,
pointer coordinates, accessibility bounds and native-resolution rendering.
It is a suitable capability for a later UI-scale slice. No manual coordinate
scaling or bitmap resampling is required. This slice does not add a scale field.

The pinned module contains an audio package. A playback API alone does not
define product interaction sounds, assets, event ownership or platform behavior.
Sound, palette expansion, custom color keys and Omarchy following remain outside
this first slice. No saved switch claims those features work.

## Acceptance

The canonical `settings-appearance` case runs a private production `unicd`.
It disables telemetry and sync and never creates a renderer, browser, native
window or system clipboard adapter. It verifies defaults, all accepted values,
production projection, invalid values, stale revisions, external edits and
restart persistence. The runner records source identity and cleanup; the case
receipt records the service executable hash and completed checks.

```sh
make desktop-e2e CASE=settings-appearance
```

Keep the first failure. Allow at most two corrective replays after diagnosing
that failure. A repeat must be investigated before delivery. The expected first
failure is absence of the reserved `desktop.color_mode` field in the baseline.

No visual or native acceptance is claimed by this document.

## Pre-implementation evidence

Canonical run `20261122T072011Z-2ba38d5b` failed at the first production-service
assertion: `default color mode: got "", want system`. Its scenario receipt
records `passed=false`, `cleanup_passed=true`, `renderer=false` and
`system_clipboard=false`. The service SHA-256 is
`38201b4806747d1021d510a3946b19da117356fbfa2cdfac508edab941b002ce`.
The original result and scenario receipt remain under the ignored canonical
run directory. This failure establishes the absent baseline configuration field;
it does not diagnose a renderer or native theme failure.

## Implemented path

The service stores `desktop.color_mode` in its existing TOML configuration.
The default is `system`; only `system`, `light` and `dark` are accepted.
The production appearance page uses the existing revision-checked save path.
`projectLiveConfig` projects successful reads and saves into the existing view
state without recreating editor state.

The GUI, tray and Quick Panel hosts set mygo's native theme source before
startup and update it from their existing configuration watchers. Setup shares
the GUI host theme. The host uses the maintained `Theme.SetSource` method;
this method marshals native changes to the main thread and invalidates content
appearance. This source inspection establishes integration, not visual or native
scheme acceptance.

Canonical green run `20261122T072242Z-184419a2` passed the six process check
groups, including production config projection. It used production `unicd`
SHA-256 `b49f574ee56844bb636d5ddff93c589a5fdd7c560e8d23378206d281b9b73512`.
The case and scenario receipts both report PASS and private cleanup PASS.
All five entries in the run's `SHA256SUMS` verified independently. The original
red artifact remains unchanged. No renderer, GUI or system clipboard was used.

Remaining acceptance: actual controls and native scheme changes across the
main window, setup and Quick Panel; OS theme changes while following system;
editor/selection preservation during actual theme changes; other-platform
runtime acceptance. UI scale, palettes, custom colors, sound and Omarchy
following remain subsequent D-group work rather than implemented features.
