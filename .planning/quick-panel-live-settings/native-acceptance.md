# Native acceptance procedure — NOT RUN

Run only on a dedicated, disposable macOS test host. Do not use a personal desktop. This procedure is manual product acceptance; the browser suite does not automate or prove these checks.

## Build and identity

From the repository root, restore the exact HEAD and dirty-source manifest supplied with this task. Stage the sidecars, then build a new acceptance bundle in a unique output directory:

```bash
node scripts/stage-daemon.mjs --target aarch64-apple-darwin
python3 apps/gui-go/packaging/macos/package.py bundle --target aarch64-apple-darwin --variant acceptance --out target/quick-panel-live-settings-native-acceptance
```

Use the acceptance runner's private HOME, unique `gui-go-*` profile, `UC_GUI_GO_ISOLATED=1`, `UC_DISABLE_SYSTEM_CLIPBOARD=1`, and development environment. Do not start the shipping variant. Keep the runner's owned process cleanup and sanitized receipt. Record SHA256 for the resident, UI and helper executable before execution. Record `codesign -dv --verbose=4` for each actual executable and the acceptance bundle. Packaging signs the helper under its own `.quick-panel` identifier and the UI under `.ui`; this does not establish which process TCC attributes as responsible. Record the actual TCC entry and each process permission result.

## Listener

1. Deny Accessibility to the actual helper responsible process. Start the acceptance app. Enable the panel and configure a modifier. Confirm listener initialization fails without a permission prompt. Record PIDs and sanitized diagnostics.
2. Grant that process permission in System Settings. Return to the settings page without opening the panel through its ordinary shortcut. Double-tap the configured modifier. Confirm the native panel opens. Confirm resident/helper PIDs did not change.
3. Return to settings repeatedly. Confirm one double tap causes one toggle. Revoke permission and confirm double taps no longer toggle. Grant again and confirm recovery without restarting the app.
4. Repeat while disabled, while the helper is stopped, and after enabling again. Refresh must not launch a stopped helper or enable a disabled preference. Retain failures; do not replace them with host permission screenshots.

## Position and delayed response

1. Save `follow_cursor`, put the pointer away from screen center, open once, and record the first visible frame/window bounds and the helper placement diagnostics. Close, save `center`, and repeat. Do not accept a second opening as the first-opening result.
2. During an options request, toggle closed. Confirm no late window appears. Reopen immediately and confirm only the latest opening sets its position.
3. During an options request, switch to another application. Confirm the delayed answer does not show or activate the panel. Repeat without Accessibility permission; focus validation must still apply.
4. Refresh settings while already visible. Confirm no reposition or focus activation occurs. Make options unavailable and confirm the panel remains reachable using its last known placement.
5. Disable the panel. Confirm the helper exits while resident/daemon PIDs remain unchanged. Re-enable and confirm the new helper launches. Record normal and forced exits separately.

For every step save the exact action, expected/actual result, timestamp, PIDs, first-frame screenshot/window coordinates and sanitized diagnostics. Save source/build identities, run ID, cleanup receipt and SHA256. Do not publish tokens, connection files, databases, private logs or user content. Mark each unexecuted check NOT RUN.
