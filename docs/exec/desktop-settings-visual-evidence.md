# Desktop settings native screenshot evidence

- Date: 2026-11-22.
- Authorization: The user requested screenshot evidence after the parallel implementation.
- Parent: [parallel execution receipt](desktop-settings-parallel-receipt.md).
- Final canonical run: `20261122T075547Z-03d03c5b`, PASS with unchanged source and private cleanup PASS.
- Replay: `make desktop-e2e CASE=settings-visual-evidence`.

The canonical runner opened the real native Desktop composition with a private
profile and production service. Sync, telemetry, login startup and global
shortcuts were disabled. The clipboard adapter remained the isolated directory
fixture. No permission prompt, native file dialog or personal clipboard access
was triggered.

Six original window-content PNGs were inspected and uploaded inline to the
current Lody conversation: diagnostic levels, telemetry/export controls, cache
usage, portable-backup controls, dark appearance and Accessibility status. The
scenario selected dark mode through the real control, verified the saved value
and displayed selection, captured dark content, then restored system mode. It
retained a continuously sampled recording. All fourteen report checksums were
independently verified. Native GUI/tray exit and endpoint removal were observed.

The initial capture run `20261122T075134Z-1fba958f` passed but framed some new
controls at the viewport edge. The expanded capture run
`20261122T075336Z-cd744dbc` failed while waiting for an `option` role. Its failure
PNG shows the open theme menu. Inspection of pinned mygo `ui/access.go` confirms
that native `RoleOption` maps to `RoleListItem`; the script used the wrong role.
One corrective replay using `listItem` passed. Both earlier artifacts remain
unchanged. No production behavior was changed to obtain the replay.

The [small receipt](evidence/settings-visual/receipt.json) and its
[checksums](evidence/settings-visual/SHA256SUMS) bind the delivered images, actual
binary identities, assertions and original report. Images and recording remain
in the ignored canonical output directory; they are not committed.

This establishes main-window settings capture and the tested dark/system
selection path on macOS. It does not establish native setup/Quick Panel theme
coverage, other-platform runtime behavior, real permission grants, dialog export
and import, OS chrome, updater installation or cross-application paste. Screenshot
delivery is recorded; user visual approval has not been recorded.
