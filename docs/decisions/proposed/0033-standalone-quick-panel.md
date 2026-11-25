# 0033 · Standalone Quick Panel

- Status — Draft.
- Date — 2026-11-25.
- Platforms — Linux (Wayland and X11), Windows, and macOS.
- Related decision — [Headless CLI and Desktop integration](0015-headless-cli-and-desktop-integration.md).
- Landing targets — [Headless architecture](../../architecture/headless.md), [Desktop runtime](../../architecture/desktop-runtime.md), [Quick Panel contract](../../contracts/desktop-quick-panel-contract.md), and [local service contract](../../contracts/local-service-contract.md).

## Context

Quick Panel already runs in its own process. Today the tray process starts or activates it. On Linux and Windows, the panel also sends clipboard writes and checks to the tray. On macOS, the panel sends them to the clipboard adapter in `unicd`, which Desktop enables during startup. The current `--role quick-panel` entry waits for a private activation request. A Wayland session without a tray icon or a global shortcut makes the activation gap visible. The process dependency affects all three platforms. [Quick Panel contract](../../contracts/desktop-quick-panel-contract.md)

## Decision

### Launch and lifecycle

Provide `unic-desktop --quick-panel` as a public Desktop entry. It opens or focuses one panel for the selected profile. Repeated or concurrent launches use the existing profile-scoped role lock and private IPC. The tray menu, main-window button, and global shortcut call the same activation path. The existing shortcut toggle and Favorites filter remain explicit activation actions.

An initialized profile with a running standalone `unicd` supports opening, searching, previewing, and copying with only the panel and `unicd`. The public entry connects to the matching service through the existing local socket. It reports a missing or unavailable service directly. Use the shared setup and migration checks before admitting activation. An explicit command from the panel can open the main window through the normal Desktop launcher.

The panel owns its windows and process lifetime. It remains usable after the GUI or tray exits while the matching standalone service is available. It borrows the service for the duration of use. Closing the panel leaves that service running. Preserve the distribution model of a single Desktop executable and the headless service executable. [Headless architecture](../../architecture/headless.md)

### Clipboard ownership

Use one tracked history-application path. `unicd` supplies history and content leases, arbitrates application, and records the result. The native adapter writes to the system clipboard and verifies the actual write. Keep one observation baseline and one in-flight application per profile. [Engine contract](../../contracts/engine-contract.md)

| Platform | Native clipboard writer |
|---|---|
| macOS | The existing adapter in `unicd`. The panel requests opt-in when it needs clipboard access and waits for readiness. |
| Windows and Linux | The panel, using the existing mygo board and a bounded, explicit application request authorized by `unicd`. A background observer can continue to run through the existing Desktop integration. |

Extend the local adapter contract to admit a panel write with or without an existing background observer. With an observer, serialize the panel write with other applications and associate the verified write with the active observation generation. Without an observer, establish the baseline needed for the explicit write. Bind the request and result to the selected profile, service generation, and application token. Reuse the existing content leases, application result, and read-back rules. Reject stale results and advance the baseline only after a verified write. A write with an uncertain result requires a new user action before another write. Preserve the configured restore-sync behavior and suppress capture and sync echoes of the same write. [Local service contract](../../contracts/local-service-contract.md#adapter-session)

Close the window and release its search, preview, and rendering resources on dismissal. Keep the native clipboard source and any file lease while the clipboard still needs them. On Wayland, a focused panel can use the ordinary selection protocol. If the desktop leaves the data with the source process, the windowless panel continues serving it until ownership ends. This keeps Copy usable after the user dismisses the panel, including on desktops without a clipboard manager. [Engine contract](../../contracts/engine-contract.md) · [Wayland selection protocol](https://wayland.freedesktop.org/docs/book/Protocol.html#selection)

### Copy and automatic paste

Copy works independently of target capture. Capture a potential paste target before panel activation. Keep target identity and native input in Desktop. On macOS, Windows, and X11, apply the existing target, permission, focus, and clipboard checks before dispatching one paste chord. On Wayland, support automatic paste through compositor-managed focus return plus `zwp_virtual_keyboard_v1` (on compositors that advertise it) or the user-consented `org.freedesktop.portal.RemoteDesktop` portal (with persistent `restore_token`), falling back to Copy when neither is available, consent is declined, or the panel was opened from `unic` itself. [Quick Panel contract](../../contracts/desktop-quick-panel-contract.md#platforms-and-privacy)

## Trade-offs

| Approach | Consequence |
|---|---|
| Public command routed through the tray | A command supplies another way to invoke the tray. Tray availability still controls panel activation and, on Windows and Linux, clipboard application. |
| Native clipboard access in `unicd` on every platform | A common writer would require the headless service to participate in the user's graphical session on Windows and Linux. Wayland clipboard selection also depends on the focused client and input serial. |
| Standalone panel with service-coordinated application | Keep desktop input and the platform write in the process that can perform them. Reuse the macOS daemon adapter and adapt the local service contract to coordinate the other platforms. |

Choose the third approach. It supports standalone use and gives clipboard application one place to enforce history policy, ordering, and resource lifetime. Native implementation differences stay behind the existing Desktop and service interfaces.

## Acceptance

Specify the end-to-end cases before changing the runtime. Use the public entry, production roles, and an isolated profile. Run the canonical Desktop suite against the following scenarios.

1. On Linux, Windows, and macOS, start standalone `unicd`, then open Quick Panel through `--quick-panel`. Check the process inventory, search and preview stored history, and copy text, an image, and files. Dismiss the window and verify the real system clipboard from another application. On Wayland, include a session without a tray host, a global shortcut binding, or a clipboard manager. Check that replacing the clipboard releases the old file lease.
2. Repeat activation and exercise simultaneous requests. Verify one panel per profile and one service generation. Check service absence, profile mismatch, and migration admission through the same public command.
3. Run the tray alongside the independent panel. Verify one write per action and no capture or sync echo. Exit the tray while retaining the same standalone service, then repeat Copy. Interrupt a write and verify that the result is reported without replay.
4. Check real target insertion separately on macOS, Windows, Linux Wayland (`zwp_virtual_keyboard_v1` / `RemoteDesktop` portal, plus Copy fallback when unavailable), and Linux X11.

Keep a reproducible report with platform and session identity, source and executable hashes, process inventory, operation order, and independent clipboard read-back. A directory clipboard can verify process and protocol behavior. Native system clipboard and target insertion require their own runs and verdicts. [Desktop E2E contract](../../contracts/desktop-e2e-contract.md)
