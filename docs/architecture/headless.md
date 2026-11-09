---
paths:
  - app/service
  - app/desktop/cmd/unic-desktop
  - internal/localapi
  - internal/localconfig
  - internal/localpaths
  - internal/localcontent
  - internal/clipboardintegration
  - e2e/headless
  - tools/package
verified: eeda052c613e53a49909d19d8ad22bfa88ec42fc
---
# Headless service and desktop integration

## Current implementation

The product has three programs and two distributions.
`unic` is a command-line client. `unicd` is the Engine owner and local service.
`unic-desktop` owns mygo, the UI Kit, system clipboard, tray and notifications.
The headless distribution includes `unic` and `unicd`.
The desktop distribution includes the same two binary files plus `unic-desktop`.
Neither headless program imports the desktop application or a UI toolkit, directly or transitively.

```text
unic ------------+-- local IPC -- unicd -- Engine / history / plugins / network
unic-desktop ----+
      +-- mygo / UI Kit / system clipboard / tray / notifications
```

The service accepts explicit text, PNG and file-reference submissions without a clipboard observation epoch.
Incoming content remains readable through history or the bounded transient Received list.
Clipboard ability does not filter headless content reception.
An Action creates a new item and applies it only when explicitly requested.
History restore continues to respect `sync.on_restore`.

One exclusive adapter generation connects the desktop to the service.
The desktop establishes a fresh baseline and runs the observation/application state machine through IPC.
A missing adapter is normal. Observe and Apply are `unavailable` until an adapter declares its abilities.
Generation expiry releases owned leases and disables clipboard command dispatch.
Closing the main window leaves the adapter running. Explicit desktop quit stops the adapter.
The service continues receiving and storing content after desktop exit.

## Invariants

- One state directory has one service, one Engine identity and one history database.
- CLI and desktop access the service through shared local DTOs and clients.
- The CLI does not launch a GUI or silently replace system clipboard reads with history reads.
- The desktop owns system clipboard access. The service owns submission, sync and history policy.
- Reconnect establishes a baseline and does not apply disconnected backlog.
- A lost write result is uncertain and is never automatically retried.
- Snapshot uploads, session leases and pending commands have fixed limits.
- A service restart invalidates leases and sessions. A prior system clipboard file lease does not survive restart.

## Code map

| Path | Responsibility |
|---|---|
| `app/service/cmd/unic` | CLI, streams and explicit export |
| `app/service/cmd/unicd` | Headless lifecycle and socket server |
| `app/service/internal/host` | Engine assembly, session ownership, local request handling |
| `app/desktop/cmd/unic-desktop` | GUI, tray, notifications and adapter startup |
| `app/desktop/internal/board` | Native mygo clipboard access |
| `app/service/internal/server` | Service-owned HTTP routes and socket listener |
| `internal/localapi` | Shared protocol DTOs and clients |
| `internal/localconfig`, `internal/localpaths` | Shared local configuration and state paths |
| `internal/localcontent` | Local snapshot import and file-reference conversion |
| `internal/clipboardintegration` | UI-independent observation/application state machine and IPC client |
| `e2e/headless` | Production-binary, multi-process acceptance and manifest |
| `tools/package` | Shared headless binaries and optional desktop archives |

## Specifications

See the [local service contract](../contracts/local-service-contract.md),
[Engine contract](../contracts/engine-contract.md), and [history contract](../contracts/history-contract.md).

## Gaps

Native macOS, Windows and Linux clipboard acceptance remains separate from process fixtures.
Signed platform installers, login/service-manager registration and store publication are not supplied by the archive builder.
Closing a desktop login session does not promise service survival; that depends on its service manager.

The front-matter `verified` marker remains the baseline until these workspace changes are committed.
Current process checks are bound to complete source-hash manifests, as recorded in the execution receipt.
