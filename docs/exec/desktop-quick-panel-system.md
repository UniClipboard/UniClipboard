# Desktop quick-panel system execution

Baseline: c386b286e4, branch v2, clean working tree.
Contract: [Desktop quick-panel system](../contracts/desktop-quick-panel-contract.md).

## Execution order

1. Specify failure cases and write process E2E before implementation.
2. Add Desktop-owned session, clipboard write receipt and shortcut registration modules.
3. Add native target adapters and bind the existing panel/settings.
4. Run production-process E2E, regression, compile/build, lint and documentation checks. Keep a small JSON receipt and delete private profiles/binaries.

## Acceptance

Final frozen-source checks passed. No native App, browser, renderer, system hotkey or real clipboard was exercised.

- Quick-panel production-process E2E: 12 scenario groups. `.build/quick-panel-system/receipt.json` binds 316 source hashes, including go.mod/go.sum, and the race-enabled production service binary.
- Existing desktop data-binding process E2E: 16 checks. `.build/quick-panel-system/desktop-regression.json`.
- Existing search process E2E: 14 scenario groups. `.build/quick-panel-system/search-regression.json`.
- Existing startup/lifetime process E2E: 15 checks. `.build/quick-panel-system/startup-regression.json`.
- Existing config/clipboard/host/local API/Engine checks: 231 passed across five packages under the race detector.
- Native macOS build and Windows/Linux amd64 cross-builds passed. `.build/quick-panel-system/builds.json` records final executable hashes; temporary binaries and private profiles were removed.
- Pinned golangci-lint v2.15.0 reports zero new issues for affected Desktop/config packages, including separate Windows/Linux analysis of native adapters.
- Implementation commit: `854df5e68966b8ef3db1cf73c33785355fc54cb0`. The five affected architecture pages were reviewed against this source and their verified markers were updated. `go run ./tools/docscheck -strict` passes without warnings.

All four process receipts were checked against the committed implementation source. Their aggregated identities are recorded in `.build/quick-panel-system/acceptance.json`.
An earlier startup run rejected a source change while acceptance was active; its result was not accepted. Later frozen-source runs replace that receipt. Regressions also caught HTML-only plain-mode handling, immediate keep-open focus recovery and an ignored error in its delayed focus check before their fixes.

## Implemented behavior

The existing quick-panel composition now calls one Desktop-owned target session. Global activation, tray activation and the Settings button share capture-before-show ordering. The default is Command+Ctrl+V on macOS and Ctrl+Alt+V on Windows/Linux. The Settings page persists a fixed shortcut choice through existing config CAS and displays registration conflicts separately from saved intent. Lightweight mode has no graphical shortcut host.

The session applies history through production IPC and the existing adapter. Its tracked board verifies a single completed write and compares payload plus file paths before native input. Enter sends paste, Shift+Enter uses the existing plain Action, Copy never injects input, and Command/Ctrl+Enter reopens after a 150 ms dispatch interval only while the target still owns focus. HTML-only records cannot silently become empty plain text.

Cancellation and newer generations suppress late apply completions. External blur never restores focus. Explicit dismissal can restore the captured target. Native activation, permission, held-key and injection failures never replay paste. The UI reports input dispatch rather than claiming receiver insertion.

macOS uses existing purego/objc bindings. Windows uses x/sys. Linux X11 adds pinned jezek/xgb v1.3.1 for checked protocol errors and XTest. This avoids fatal process-global Xlib error handlers and external command dependencies. Wayland/XWayland sessions keep portal activation and Copy; arbitrary target capture and automatic paste remain unavailable.

## Reproduction

Run from the repository root. Fixtures never launch native GUI, renderer, global keys or the system clipboard. Their private profiles and binaries are removed.

```sh
make desktop-e2e CASE=startup-lifecycle
make desktop-e2e CASE=data-binding
make desktop-e2e CASE=search-contract
make desktop-e2e CASE=quick-panel-contract
```

## Native acceptance checklist (not run)

1. Build/install the actual platform package. On macOS, use a stable application-bundle identity for Accessibility authorization.
2. Open a disposable target document. Invoke the configured global shortcut. Confirm the label and that capture happened before unic activation.
3. Paste text, rich text, image and a managed file. Confirm exact content in the receiving application. Test Shift+Enter with a stored plain representation and its explicit rejection for HTML-only content.
4. Use Command/Ctrl+Enter. Confirm paste reaches the target before the panel reopens. Switch applications during dispatch; confirm the panel does not reclaim focus.
5. Use Escape, external blur, repeated Enter, target termination and delayed clipboard completion. Confirm no stale paste or focus theft.
6. Revoke Accessibility permission or enable secure input on macOS; use an elevated target on Windows. Confirm failure stays visible and Copy remains usable. Verify held modifiers and supported keyboard layouts.
7. Change the global shortcut, create a real conflict, retry, disable, restart and quit. On Wayland, confirm portal approval behavior and Copy-only paste fallback.
8. Check preview nonfocusability, native menus, screen-edge placement, background startup and reactivation on each platform.

## Remaining limits

OS focus and input routing cannot be made one atomic transaction. The process fixture substitutes native target/input calls and cannot establish native permission, focus policy, layout behavior or receiver insertion. The 150 ms reopen interval is a dispatch guard rather than an acknowledgement. Wayland portal registration remains asynchronous in the pinned mygo API. No paste-success analytics was added. The implementation commit is recorded in Git history. No push was performed.
