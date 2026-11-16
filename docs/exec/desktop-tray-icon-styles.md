# Desktop tray icon style parity

Status: implemented; nonvisual acceptance passed. Native visual acceptance is not authorized.

## Behavior contract

- `desktop.tray_icon` accepts `glyph` (default) and `cat`, matching v1 style identities. Settings > General > Tray icon selects the value through the existing revision-checked configuration API. CLI/TOML use the same field and validation.
- Apply successful changes immediately. Retain the saved style across Desktop restart. Reject invalid values without replacing the last valid settings. No restart is required.
- Preserve the v1 glyph geometry (two offset rounded cards, stroke width 2), cat source alpha and macOS geometry (44-pixel canvas; 36-pixel glyph grid or 34-pixel cat image). Render with existing mygo SVG support and x/image scaling.
- Use mygo template contrast on macOS and Windows. Windows uses the system small-icon metric. Linux uses 22 pixels and the v1 KDE/gsettings foreground-color rule; repaint when the theme changes.
- Style changes must only set the tray image. Language changes may rebuild translated menus. Repeated unchanged settings must not publish an image. A failed image update must retain the shown style and permit retry.
- Keep one static icon. Do not add status badges or animations. No tray exists in lightweight mode.

## Failure inventory specified before implementation

1. An omitted field selects cat instead of glyph.
2. The settings selector changes only local view state or accepts input while a save is pending.
3. The saved value disappears after reopening or is marked as restart-required.
4. A file edit or CLI change never reaches the live image owner.
5. Unknown values replace the last valid settings or silently save a different value.
6. Repeated configuration refreshes publish the same image or reset the current style after a theme event.
7. A style-only change closes open menus by rebuilding them.
8. A native update failure marks an image as shown, preventing retry.
9. Rendering loses transparent interiors, macOS padding, or cat alpha.
10. The icon becomes invisible after a taskbar/panel theme change.
11. Theme polling survives Desktop shutdown or accesses an unrelated profile.
12. Acceptance opens a native window or mislabels a software/observer boundary as OS tray evidence.

## Acceptance

Use `make desktop-e2e CASE=tray-icon-settings`. The registered software-view scenario drives the production settings composition, real unicd/CLI processes, TOML persistence and a production configuration watcher. It observes the actual PNGs delivered by the shared tray image controller at its native API boundary. It retains view checkpoints and a JSON receipt through the canonical runner. Verify default, both switch directions, immediate delivery, file/CLI changes, invalid values, restart persistence, duplicate suppression and image-update retry.

Native tray appearance, open-menu preservation, DPI changes and actual Windows/Linux theme notifications remain NOT_RUN until separate platform acceptance. No browser, App or VM is launched for this task.

## Observed results

- `make desktop-e2e CASE=tray-icon-settings`: PASS. Final report: `.build/desktop-e2e/20261116T044532Z-bca3e1ea/index.html`. Its scenario receipt records both generated PNG SHA-256 values and all published images. Software settings and image-boundary PNG checkpoints are included. Cleanup passed.
- Canonical `i18n` case: PASS across five languages, three skins and both themes. Final report: `.build/desktop-e2e/20261116T044520Z-3af8d14c/index.html`.
- Production compilation: PASS for darwin-arm64, windows-amd64, windows-arm64, linux-amd64 and linux-arm64. Source fingerprints and binary digests are in `.cache/tray-style-build/report.json`; temporary executables were removed. Compilation does not establish native execution.
- The selector uses the shipped v1 labels Default and Classic cat. The Linux KDE palette resolver is reused from v1 source `c6483d73c7a0234be68571fdb5fa3d38a17c4871`. No separate SVG parser or package dependency was added.

The first acceptance run failed because the not-yet-implemented trayicon package was absent. Subsequent driver corrections separated consecutive menu clicks and quoted CLI string arguments as TOML. The final source passed both affected acceptance cases.

Native tray visibility, actual theme-event delivery, open-menu preservation, DPI changes and OS icon-cache refresh remain NOT_RUN. No native App, browser or VM was launched. The separate CI-repair request remains deferred.
