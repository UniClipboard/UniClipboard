# Independent desktop processes

Status: implementation design; process and native acceptance are separate gates.
Baseline: `173d362036ae4a746878306915c85bcdf7468751`.

## Ownership

Use the existing `unic-desktop` executable with explicit `--role tray`, `--role gui` and `--role quick-panel` entry points. Each role is a real OS process. Keep `unicd` UI-free as the fourth process. A no-role launch starts or contacts the tray, then requests the saved startup behavior.

| Process | Lifetime | Exclusive responsibility |
|---|---|---|
| Tray | Desktop session | Tray, global shortcut, target capture, clipboard adapter, notifications, login intent, owned-service handle |
| GUI | Open until window close | Setup, migration and main history/settings rendering |
| Quick panel | Created on first request; hidden between requests | Panel rendering, target validation, focus restoration and synthetic paste |
| unicd | Owned pipe or independently started service | Engine, history, settings, network and adapter protocol |

The tray follows the established main-process/native-integration pattern described in [Electron's process model](https://www.electronjs.org/docs/latest/tutorial/process-model). This does not add Electron or another runtime.

## Launch and close

Fresh or pending setup opens the GUI in every startup mode. No clipboard adapter starts before explicit setup completion. Established normal profiles open the GUI unless background startup is selected. Lightweight profiles keep the tray and service, without creating a GUI. A later tray Open always starts or activates the GUI regardless of saved mode.

Closing the GUI exits that process and releases its renderer, caches and service subscriptions. Closing or dismissing the quick panel hides that independent process; it registers no global shortcut and owns no adapter. Explicit Quit in the tray closes both UI roles, releases the shortcut and adapter, then stops only the service started and owned by the tray. An independent `unicd` remains running. Running `unicd` alone never starts graphical integration.

## IPC and isolation

One live role lock and one private role endpoint exist per canonical profile. Bind requests to the canonical profile and executable identity. Verify live endpoint ownership rather than trusting a PID file. Reuse existing local transports and file locks. Reject a mismatched profile, binary or unexpected listener. All content remains in Engine; no second history database exists.

The GUI delegates native integration requests to tray. The quick panel receives an ephemeral foreground target captured by tray before process launch or window activation. The target stays in memory and private RPC only. It delegates history application and clipboard-match checks to the tray's tracked adapter. Keep the existing generation-bound focus, permission and paste checks; no failed or uncertain paste is retried.

## Failures and recovery

A GUI crash does not stop tray or service. A quick-panel crash leaves the shortcut and adapter running; the next request creates the panel again. Repeated launch and shortcut requests serialize through role ownership. A tray crash releases its shortcut and adapter session through OS process teardown. Its owned service closes through the parent pipe. Existing UI roles remain read-only service clients and reconnect after the next desktop launch recreates tray. A service restart renews the adapter session and establishes a baseline; disconnected backlog never writes to the clipboard.

Migration first pauses tray integration and closes the quick panel. Stop and await only a tray-owned service. An independent service blocks migration without stopping it. Persist pending setup before import; completion requires a ready service and explicit entry. Cancellation preserves the pending marker.

## Acceptance

Write failure scenarios and real-process regression before runtime changes. Reuse canonical Desktop E2E, isolate all profiles and synthetic clipboard bytes, and record role PIDs, creation times, binary/source hashes, IPC results, persistent history/settings and cleanup. Native tray clicks and receiving-application paste remain separate evidence. Native Windows acceptance has priority; cross-compilation proves only compilation.
