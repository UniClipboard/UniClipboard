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
  - app/desktop/internal/quickpanel
  - internal/localcontent
  - internal/clipboardintegration
  - internal/webtransfer
  - internal/tcnet/ephemeral.go
  - engine/transfer.go
  - e2e/headless
  - e2e/cli
  - e2e/webinterop
  - tools/package
verified: f8d5a41544932c2ca2bf4a9a2c230ee5db939353
---
# Headless service and desktop integration

All programs expose `--version --json` before configuration and host initialization. Versioned CLI packaging and npm channels follow the [release architecture](releases.md).

## Current implementation

The product has three runtime programs and two development distributions.
`unic` is a command-line client. `unicd` is the Engine owner and local service.
`unic-desktop` owns mygo, the UI Kit, system clipboard, tray and notifications.
The headless distribution includes `unic` and `unicd`.
The desktop distribution includes those same binary files plus `unic-desktop`.
Both archives expose `uniclip` as an alias of `unic`.
On macOS/Linux, `uniclip` is a relative symbolic link to the sibling `unic` file; tar preserves the link.
On Windows, `uniclip.cmd` invokes the sibling `unic.exe` and preserves arguments and its exit code without requiring symbolic-link privileges.
The packaging manifest records the alias separately from executable hashes. Moving an extracted installation keeps the alias valid.
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

## Command surface and temporary transfer

The CLI is organized by user intent. `unic send` (`s`) and `unic recv` (`r`) move content. `unic history` (`h`) browses it. `unic device` holds pairing and trust. `unic dev` holds development and diagnosis commands. `unic status`, `config`, `plugin` and `migrate` provide management operations. Old command names are removed without aliases.

`send --to all|DEVICE` goes through the normal sync path. `CommitRequest.DeviceIDs` limits the send to named paired devices; unknown, repeated or empty selections are `invalid_argument`. Only peer-session backends honor it.

`send` without `--to` and `recv CODE|LINK` run a temporary transfer with a peer that has no unic device identity, such as the page at `try.uniclipboard.app`. It speaks the page's pull protocol (`internal/webtransfer`: frames, offer, SHA-256 verification, 20 MiB and 50 items per transfer). The sender listens on a fresh random-key tailcat endpoint (`internal/tcnet/ephemeral.go`) that accepts every virtual port and serves one receiver. When rendezvous accepts the ticket, a 6-digit code aliases it for at most five minutes with ticket kind `uc-web-try`; otherwise the sender prints only a link. Bare tailcat addresses cannot initiate a pull. `recv` accepts links or ticket fragments (`#c=…&k=…`); `recv` refuses a `unic-pair` code and `device add` refuses a transfer code.

The receiver admits the offer through `Engine.AdmitTransfer` before reading data, writes files into a private inbox directory and completes into the ordinary received pipeline. History origin is `external` with plugin ID `unic.transfer`. The transfer never creates a device or trust. The sender keeps its prepared transfer payload in memory. Delivery means that the receiver verified payload hashes and sent a protocol ACK. The ACK precedes receiver file placement and pipeline completion; it does not prove history persistence or system clipboard application. Addresses, tokens, codes and links are never logged.

## On-demand service for CLI commands

Each `unic` leaf command declares one requirement: `local`, `service`, `service+clipboard` or `offline`
(`app/service/cmd/unic/requirements.go`). `unic help --json` exports the declarations. Text help lists commands without dependency badges. Terminal output uses styling and progress on stderr; redirected streams, `NO_COLOR` and `TERM=dumb` use plain text.
A service command acquires a service on its first service request. Local argument parsing precedes acquisition; validation that requires service state occurs afterwards. `recv` checks the source shape locally (`webtransfer.SourceProblem`), so a source that is neither a 6-digit code nor a transfer link never starts a service. Resolving a well-formed code or link, and rejecting a pairing code or pairing ticket, still happens in the service.

`unicd` has three lifetime modes. `standalone` and `desktop` generations are borrowed and never stopped by a CLI command.
A `cli-temporary` generation, started by `unic` as `unicd --cli-temporary`, lives while at least one CLI usage lease exists.
A lease is an open HTTP response on the private socket, bound to one generation. Process death closes it.
The last release closes admission and retires the generation in one step, and the releasing command waits for the generation to stop.
A command that meets a retiring generation retries acquisition before it sends any business request.
A launcher holds initial ownership through an anonymous stdin pipe until its first lease is admitted, so a ready service never exits before its starter can use it.
There is no idle timeout, durable counter or configuration setting.

Acquisition shares one profile transition lock (`service-transition.lock`) with desktop startup. It covers probing, launch and lease admission. The service serializes last release and new admission with its own internal lock. Commands do not hold the transition lock while business work runs.
The normal desktop may replace a temporary generation. The affected command reports the interruption and never repeats a write.
Lightweight desktop startup refuses a temporary generation; it is not promoted.

Commands that start a paired-device send observe only their own Engine operation and report `success`, `partial`, `failed`, `superseded`, `timeout`, `interrupted` or `none`. An operation-query error returns a command error without a send report; it does not prove that the local commit failed.
Pairing approval, `device add` without an argument, and `dev logs level --for` hold a temporary generation for a bounded, announced window.
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
| `app/desktop/internal/quickpanel` | Desktop-only target sessions, adapter-write receipts, native paste and global-shortcut ownership |
| `app/service/internal/server` | Service-owned HTTP routes and socket listener |
| `internal/localapi` | Shared protocol DTOs and clients |
| `internal/localconfig`, `internal/localpaths` | Shared local configuration and state paths |
| `app/desktop/internal/lifecycle` | Serialized service replacement, parent pipe and saved login intent |
| `internal/servicelifecycle` | Generation, lifetime mode, usage leases and owner EOF detection |
| `internal/servicelaunch` | Sibling resolution, transition lock, detached process setup and readiness wait shared by launchers |
| `app/service/cmd/unic/session.go` | CLI service acquisition, launch, release and lazy request transport |
| `app/service/cmd/unic/requirements.go` | Leaf command requirement catalog, word aliases and help |
| `app/service/internal/host/transfer.go` | Temporary transfer: offer, pull, ticket resolution, placement into the received pipeline |
| `internal/webtransfer` | The page's transfer protocol, tickets and links; depends only on `internal/rendezvous` for the code shape; no tailcat dependency |
| `internal/tcnet/ephemeral.go` | One-shot tailcat listener and dialer with random keys |
| `e2e/webinterop` | unic against the real page code under bun |
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
Temporary transfer has no GUI entry. This follow-up remains separate from the archived command-surface and host implementation. The page's reply (`push`) is not supported. The production code service for `uc-web-try` tickets is not verified; links work without it.
Closing a desktop login session does not promise service survival; that depends on its service manager.

CLI lifecycle acceptance gaps:

- Process tests run on the host platform only. Other platforms are compile-checked (C36).
- The file resource-release barrier after a send deadline is not asserted.
- Resource thresholds were fixed after the implementation existed. The recorded numbers are measurements, not a pre-set gate.
- A single full-suite run once failed C03 ("temporary service outlived its last user"). Six isolated reruns and a clean-checkout run passed. The cause is unknown; treat a repeat as a lifecycle race.
- A cold paired-device send can miss a restarted peer because tailcat stale paths can outlast the ten-second connection window.
- Text help does not explain command requirements or held session windows. General syntax and ticket-kind errors exit 1, despite the advertised usage exit code 2.

See the [lifecycle execution receipt](../exec/0021-cli-on-demand-service-lifecycle.md).
The local submission cache also hashes only the snapshot, so changing `device_ids` under the same request ID returns the original result instead of a conflict.

The front-matter `verified` marker names committed source checked against this page; it does not extend platform acceptance.
Historical process evidence retains its original source/binary identity. The latest documentation audit does not establish a new process acceptance result.

## Desktop view

The normal desktop uses the approved mygo composition and opens history directly.
Setup and passphrase-unlock flows are removed.
The service remains the Engine owner; the view uses the shared local socket client.
The live binding and acceptance boundary are described in [repository overview](overview.md#live-desktop-composition) and the [execution receipt](../exec/desktop-live-data.md).

## Desktop startup settings

The shared `config.toml` has a `[desktop]` section. Engine does not consume it.
`language = "zh-CN"` selects the default desktop language. Supported alternatives are `en`, `ja`, `pt-BR` and `ru`. Language changes apply through normal configuration refresh without a restart. See [desktop languages](overview.md#desktop-languages).
`mode = "normal"` is the default. `mode = "lightweight"` selects service-only startup.
`launch_at_login = false` and `start_in_background = false` are the defaults.
Mode and background changes apply on the next launch. Normal background startup creates its window with `Hidden: true`.
The tray action and a second normal launch show the window. Closing the window hides it; explicit Quit ends the process and its owned service.
`unic-desktop --mode normal|lightweight` overrides the mode for that launch without changing the file.
Use `--mode normal` to reopen settings after selecting lightweight mode.
`unic-desktop-demo` is the independent layout fixture command and does not manage a service. Production `unic-desktop` has no demo dependency or `--demo` mode.

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

The container-based production acceptance job trusts only its checked-out repository path before building. Go VCS stamping remains enabled. The acceptance manifest continues to record source and executable hashes.

## Explicit directory send

`unic send --to all -f /absolute/path/to/directory` sends one directory selection through paired devices. Multiple file and directory arguments form one batch. `unic history send REF --to DEVICE|all` prepares a source reference or resends a retained immutable snapshot. It does not restore the item to the local clipboard. Directory arguments require `--to`; temporary code/link transfers continue to accept ordinary files only.

Directory source preparation runs in Engine outside clipboard polling. Async local submissions expose preparation stages and explicit cancellation. Client disconnection does not cancel admitted work. A host-owned cancellation context and `DELETE /v1/submissions/{request_id}` cancel the operation without replay. A cancellation after local commit preserves the committed history and cancels remaining transmission where possible. See the [local service contract](../contracts/local-service-contract.md) and [directory contract](../contracts/directory-contract.md).
