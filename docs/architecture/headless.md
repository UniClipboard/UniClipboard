---
paths:
  - app/service
  - app/desktop/cmd/unic-desktop
  - internal/localapi
  - internal/localconfig
  - internal/localpaths
  - internal/servicelifecycle
  - internal/servicelaunch
  - app/desktop/internal/lifecycle
  - internal/localcontent
  - internal/clipboardintegration
  - e2e/headless
  - tools/package
verified: dceb702ad0930a396ee01466f3dd1da49631dba9
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
The adapter session expires after ten seconds without renewal; the client sends heartbeats every two seconds.
A local launch key suppresses duplicate startup analytics across ready retries and reconnects. It is not an adapter credential and never enters telemetry.
Closing the main window leaves the adapter running. Explicit desktop quit stops the adapter.
In normal mode the desktop owns the service lifetime. Desktop exit also stops that service.
In lightweight mode only the standalone service remains; it receives and stores content without a desktop.

## On-demand service for CLI commands

Each `unic` leaf command declares one requirement: `local`, `service`, `service+clipboard` or `offline`
(`app/service/cmd/unic/requirements.go`). Help prints badges from the declarations.
A service command acquires a service on its first service request. Syntax errors never start a service.

`unicd` has three lifetime modes. `standalone` and `desktop` generations are borrowed and never stopped by a CLI command.
A `cli-temporary` generation, started by `unic` as `unicd --cli-temporary`, lives while at least one CLI usage lease exists.
A lease is an open HTTP response on the private socket, bound to one generation. Process death closes it.
The last release closes admission and retires the generation in one step, and the releasing command waits for the generation to stop.
A command that meets a retiring generation retries acquisition before it sends any business request.
A launcher holds initial ownership through an anonymous stdin pipe until its first lease is admitted, so a ready service never exits before its starter can use it.
There is no idle timeout, durable counter or configuration setting.

Creation and retirement share one profile lock (`service-transition.lock`) with desktop startup. Commands never hold it while they run.
The normal desktop may replace a temporary generation. The affected command reports the interruption and never repeats a write.
Lightweight desktop startup refuses a temporary generation; it is not promoted.

Commands that start a send observe only their own Engine operation and report `success`, `partial`, `failed`, `superseded`, `timeout`, `interrupted` or `none`.
Pairing approval, `code`, and `logs level --for` hold a temporary generation for a bounded, announced window.
Exact DTOs, budgets and exit codes are in the [local service contract](../contracts/local-service-contract.md#service-lifecycle).

## Offline CLI operations

`unic dev logs export [--output FILE]` reads only retained local log segments and writes
a private tar.gz with a checksum manifest. It does not use the service socket,
create an Engine, or require a desktop adapter. It works while unicd is stopped.
Existing destinations and log symlinks are rejected. Live export is an on-disk
snapshot and does not flush service queues. The archive never includes configuration,
credentials, databases or history. See section 12 of the
[observability contract](../contracts/observability-contract.md#12-offline-local-log-export).

## Invariants

- One state directory has one service, one Engine identity and one history database.
- A normal desktop launch replaces an existing service for that directory before attaching the adapter.
- Normal desktop exit, termination or abrupt process death closes its service owner pipe.
- A lightweight launch leaves only a standalone service and reuses an existing standalone generation.
- CLI and desktop access the service through shared local DTOs and clients.
- The CLI does not launch a GUI or silently replace system clipboard reads with history reads.
- A CLI command stops only a temporary generation whose last lease it released. It never stops a standalone or desktop-owned service.
- The launcher never unlinks a socket or kills a process it did not create and verify.
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
| `app/desktop/internal/lifecycle` | Serialized service replacement, parent pipe and saved login intent |
| `internal/servicelifecycle` | Generation, lifetime mode, usage leases and owner EOF detection |
| `internal/servicelaunch` | Sibling resolution, transition lock, detached process setup and readiness wait shared by launchers |
| `app/service/cmd/unic/session.go` | CLI service acquisition, launch, release and lazy request transport |
| `app/service/cmd/unic/requirements.go` | Leaf command requirement catalog and help |
| `internal/localcontent` | Local snapshot import and file-reference conversion |
| `internal/clipboardintegration` | UI-independent observation/application state machine and IPC client |
| `e2e/headless` | Production-binary, multi-process acceptance and manifest |
| `tools/package` | Shared headless binaries and optional desktop archives |

## Specifications

See the [local service contract](../contracts/local-service-contract.md),
[Engine contract](../contracts/engine-contract.md), and [history contract](../contracts/history-contract.md).

## Gaps

Native macOS, Windows and Linux clipboard acceptance remains separate from process fixtures.
Signed platform installers, standalone service-manager registration and store publication are not supplied by the archive builder.
The normal desktop registers login startup through mygo. macOS requires an application bundle.
The flat binary archives are not macOS application bundles.
A temporary service gives no continuous reception between CLI invocations; run `unicd` or the desktop for that.
Closing a desktop login session does not promise service survival; that depends on its service manager.

The front-matter `verified` marker names committed source checked against this page; it does not extend platform acceptance.
Current process checks are bound to complete source-hash manifests, as recorded in the execution receipt.

## Desktop startup settings

The shared `config.toml` has a `[desktop]` section. Engine does not consume it.
`mode = "normal"` is the default. `mode = "lightweight"` selects service-only startup.
`launch_at_login = false` and `start_in_background = false` are the defaults.
Mode and background changes apply on the next launch. Normal background startup creates its window with `Hidden: true`.
The tray action and a second normal launch show the window. Closing the window hides it; explicit Quit ends the process and its owned service.
`unic-desktop --mode normal|lightweight` overrides the mode for that launch without changing the file.
Use `--mode normal` to reopen settings after selecting lightweight mode.
`--demo` remains an isolated layout fixture and does not manage a service.

The launcher resolves `unicd` beside its own executable. Both programs must be installed together.
A profile-scoped startup file lock serializes replacement and readiness checks.
Before replacement, the launcher validates the sibling executable and checks the service directory through the private socket.
It stops only the observed random service generation and waits for socket removal after Engine closes.
An unknown listener, mismatched directory or failed replacement aborts startup.
The new service must report the launched PID and ownership mode before startup succeeds.
If an earlier service is still releasing the directory after its listener closed, the launcher retries
only the directory-in-use exit code within a 30-second startup deadline. Other startup failures return immediately.
The normal service watches stdin, which is an exclusive anonymous parent pipe.
Normal exit closes the pipe and waits for bounded graceful shutdown; a forced desktop kill closes it in the kernel.
The service cancels its runtime on EOF. Lightweight services do not watch stdin.
A normal desktop exits when its owned service terminates. It does not silently adopt a different service generation.
Lightweight startup refuses an existing normal-owned service until the normal desktop exits.

A running normal desktop watches configuration changes and applies changed login intent through
`mygo.App.SetOpenAtLogin`. `mygo.App.OpenAtLogin` supplies actual system status separately.
The profile-local `desktop-login.json` receipt records successfully applied intent.
Unchanged intent never reverses an external system disable; the user can explicitly retry in Settings.
A failed operation does not update the receipt and the GUI displays an application error.
CLI configuration state describes saved intent; native login application errors and actual status are displayed by the desktop.
When no normal desktop is running, a login-setting change waits for the next normal desktop session.
The built-in native login API cannot preserve custom `UNIC_HOME` or `UNIC_SOCK` overrides, so enabling registration with either override is rejected.
The login entry starts the same desktop executable, which reads the saved mode and leaves only `unicd` in lightweight mode.
See the [startup acceptance receipt](../exec/desktop-startup-lifecycle.md).
