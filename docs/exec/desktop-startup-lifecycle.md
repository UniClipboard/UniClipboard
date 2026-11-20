# Desktop startup and process lifetime acceptance

## Current behavior

The shared configuration stores `desktop.mode`, `desktop.launch_at_login`, and `desktop.start_in_background`. A manual no-argument launch always opens or activates the independent GUI. Explicit `--background` retains tray/service in normal mode and only unicd in lightweight mode. At login, normal mode uses `desktop.start_in_background`: true omits the GUI, false opens it. Lightweight login always keeps only unicd. The preference never suppresses manual activation. Fresh or pending setup opens the GUI before runtime admission in every entry path.

GUI close releases its renderer and subscriptions. Normal close retains tray/service; lightweight close retires GUI, tray and quick-panel processes and leaves only unicd, without a hotkey or adapter. The quick panel has its own OS process, created on first request and hidden between requests while tray integration runs. The tray alone owns the clipboard adapter, global shortcut, notifications, login registration and owned-service handle. Lightweight close transfers an owned service through the strict owner-pipe release command plus EOF, then confirms the same generation, PID and directory report standalone through the existing service API. It never restarts the service. Explicit application Quit closes all Desktop roles and stops only a tray-owned service; a borrowed standalone service survives Quit.

See the [startup architecture](../architecture/headless.md#desktop-startup-settings), [process design](desktop-process-design.md), and [local service contract](../contracts/local-service-contract.md#service-lifecycle).

## Repeatable process acceptance

```sh
make desktop-e2e CASE=desktop-processes
make desktop-e2e CASE=startup-lifecycle
```

`desktop-processes` exercises the current launcher and independent role processes with the production service, role IPC, profile storage and lifecycle code. Its build-tagged fixture replaces native rendering and clipboard access. The case must retain role PIDs, creation times, executable/source hashes, activation results, history/settings checks and cleanup status. The canonical report retains the scenario receipt, SHA256SUMS and replay entry. Private binaries, profiles and raw logs are removed after cleanup.

`startup-lifecycle` retains regression coverage of the existing service startup primitives. Its `owner` fixture exercises replacement, standalone startup and parent-pipe ownership without a GUI. The legacy `--lightweight` primitive is a fixture operation; it does not describe the current Desktop product mode. File-backed login checks validate saved intent, external disable, explicit retry and failure handling. They do not establish native login registration.

Required current-role checks include:

- Start fresh setup in either mode without premature adapter capture.
- Save lightweight mode and manually restart. Confirm the GUI opens. Close it and confirm only the same service generation/PID remains.
- Verify explicit background entry in both modes and login entry with both normal-mode background preferences.
- Close and reopen the GUI repeatedly without retaining the old GUI PID.
- In normal mode, close the GUI and create an independent quick-panel PID through the retained tray. In lightweight mode, confirm no hotkey or adapter survives GUI close.
- Converge concurrent launches on one role owner per profile.
- Preserve an independent standalone service through Desktop Quit.
- Stop a tray-owned service through owner-pipe cleanup.
- Recover missing UI roles and restarted services without duplicate adapter ownership.
- Preserve history/settings and establish a fresh baseline after service reconnection.
- Reject profile/binary mismatches and unknown listeners without stopping their owner.

A process fixture does not establish native tray visibility, shortcut delivery, keyboard focus, clipboard formats or paste insertion. Cross-compilation establishes only compilation. Earlier lifecycle results retain their original source identity and do not extend acceptance to the new process topology.

## Native acceptance checklist

The following steps are acceptance requirements, not a PASS record. Use an isolated profile and synthetic content. Record the exact OS, architecture, executable hash, source identity, and any emulation layer.

1. Start a native package. Complete or cancel first-install setup in both modes. Confirm that capture starts only after explicit entry.
2. Enable lightweight mode and quit the entire application. Reopen manually. Confirm that the GUI opens. Close it and confirm that GUI, tray and panel retire while the same unicd generation/PID becomes standalone. Confirm no hotkey or adapter remains.
3. Use normal mode. Open the main window through the native tray. Confirm a distinct GUI PID. Close it. Confirm that PID exits while tray/service remain. Repeat several times.
4. In normal mode, invoke the global shortcut with an external application focused and no GUI process. Confirm an independent quick-panel PID and the original target. Verify actual paste with synthetic text. Record native permission refusal separately.
5. Dismiss and reopen the panel. Verify shortcut conflicts, duplicate presses and modifier release. Confirm no second global registration or adapter.
6. Start an independent `unicd`, then Desktop. Confirm the same service generation. Quit Desktop and confirm the independent service remains.
7. Restart a tray-owned service and repeat capture/application. Verify fresh-baseline behavior and no synchronization echo.
8. Crash each UI role separately. Confirm the documented recovery path. Crash tray and confirm its owned service retires; relaunch and verify recovery.
9. Invoke explicit Quit from tray and the GUI application command. Confirm all owned role processes, shortcut registrations and adapter sessions retire.
10. On the default profile, enable login startup, log out and log in. Verify both values of the normal-mode login background preference. Verify lightweight login keeps only unicd after setup. Separately verify explicit `--background` in both modes and manual activation with every saved preference. Verify that an external system disable remains effective until explicit retry.
11. Enter later migration. Verify atomic tray pause, shortcut release, panel closure and exclusive import. Confirm an independent service blocks import without being stopped. Cancel and reopen to verify the pending marker.

macOS flat binaries cannot register a login item. The native API cannot preserve user-selected custom profile/socket overrides. Signing, bundle replacement, login execution and other architectures require their own native evidence.

## Verification record

Current results belong to the canonical case receipt generated for the exact tested source. The [updated process receipt](desktop-process-lifecycle.md#windows-coordination) records Windows ARM64 native tray, hotkey, foreground, rendering and close/reopen acceptance. System clipboard, actual paste, login execution and other native platforms remain outside that pass. The earlier service-lifecycle receipt covered primitive process ownership and file-backed login only. A later [D-group receipt](desktop-native-integration.md) records its own platform/source scope; it does not establish this process change.

Run `go run ./tools/docscheck` after architecture updates. Verification markers remain tied to committed source and do not imply native acceptance.
