---
paths:
  - app/service
  - app/desktop/cmd/unic-desktop
  - internal/localapi
  - internal/localconfig
  - internal/localpaths
  - internal/releaseinfo
  - internal/servicelifecycle
  - internal/servicelaunch
  - app/desktop/internal/lifecycle
  - app/desktop/internal/desktopprocess
  - app/desktop/internal/setupflow
  - app/desktop/internal/setupui
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
verified: 28d51b0f0d4cc5ba650e45ed908b749380e78d40
---
# Headless service and desktop integration

All programs expose `--version --json` before configuration and host initialization. Versioned CLI packaging and npm channels follow the [release architecture](releases.md).

## Current implementation

The product has three runtime programs and two development distributions.
`unic` is a command-line client. `unicd` is the Engine owner and local service.
`unic-desktop` provides three independent OS processes from one executable: tray, GUI and quick panel. On Linux each role sets its process/thread name via `PR_SET_NAME` (`unic-tray`, `unic-gui`, `unic-panel`) without changing `/proc/<pid>/exe`. The tray owns menu, shortcut and notification integration. The GUI and quick panel own their respective views. `unicd` remains a fourth process without a UI toolkit.
The headless distribution includes `unic` and `unicd`.
The desktop distribution includes those same binary files plus `unic-desktop`.
Both archives expose `uniclip` as an alias of `unic`.
Production Desktop staging copies an accepted exact CLI pair into an application-private directory. Its format constructors and remaining release gates are described in [releases](releases.md#desktop-staging-and-package-definitions).
On macOS/Linux, `uniclip` is a relative symbolic link to the sibling `unic` file; tar preserves the link.
On Windows, `uniclip.cmd` invokes the sibling `unic.exe` and preserves arguments and its exit code without requiring symbolic-link privileges.
The packaging manifest records the alias separately from executable hashes. Moving an extracted installation keeps the alias valid.
Neither headless program imports the desktop application or a UI toolkit, directly or transitively. macOS Desktop packages must lock a CLI artifact containing the local desktop clipboard API described below.

```text
unic ------------+-- local IPC -- unicd -- Engine / history / plugins / network
unic-desktop --role tray -- adapter IPC --+
      +-- clipboard / shortcut / notifications
unic-desktop --role gui -------- local IPC -- unicd
unic-desktop --role quick-panel --+
      +-- target checks / focus / paste; clipboard writes delegate to the active adapter
```

The service accepts explicit text, PNG and file-reference submissions without a clipboard observation epoch.
Directory snapshot export runs in the CLI and reconstructs independently owned roots under `rN/<original-name>`. It validates manifest bounds and member hashes, preserves empty directories and rejects existing root containers. Failed exports roll back only their newly created containers. See the [directory export contract](../contracts/directory-contract.md#cli-snapshot-export).
Incoming content remains readable through history or the bounded transient Received list.
The service also exposes stored history annotations, their assets and explicit enrichment requests through the [local service contract](../contracts/local-service-contract.md#history-annotations-and-enrichment).
Clipboard ability does not filter headless content reception.
An Action creates a new item and applies it only when explicitly requested.
History restore continues to respect `sync.on_restore`.

One exclusive adapter generation connects the desktop to the service.
The desktop establishes a fresh baseline and runs the observation/application state machine through IPC.
A missing adapter is normal. Observe and Apply are `unavailable` until an adapter declares its abilities.
Generation expiry releases owned leases and disables clipboard command dispatch.
The adapter session expires after ten seconds without renewal; the client sends heartbeats every two seconds.
A local launch key suppresses duplicate startup analytics across ready retries and reconnects. It is not an adapter credential and never enters telemetry.
A manual no-argument Desktop launch always opens or activates the GUI. In normal mode, closing it terminates only the GUI process and keeps tray integration running. In lightweight mode, closing it also retires tray and quick-panel processes; only unicd remains, without an adapter or hotkey. Explicit application Quit stops UI roles and only a tray-owned service. A borrowed standalone service survives Quit.

### Network change and device refresh

`unicd` watches host interface changes and clock jumps associated with sleep and wake. The macOS tray also sends a native wake notification to the service. The service coalesces these signals before asking Engine to retry disconnected peers and probe existing peer sessions. This work never replays clipboard content.

`unic device list --refresh` requests a bounded connection check before printing the device list. Desktop can request the same check for all devices or one selected device. See the [local service contract](../contracts/local-service-contract.md#device-connection-refresh) for both routes.

## Command surface and temporary transfer

The CLI is organized by user intent. `unic send` (`s`) and `unic recv` (`r`) move content. `unic history` (`h`) browses it. `unic device` holds pairing and trust. `unic dev` holds development and diagnosis commands. `unic status`, `config`, `plugin` and `migrate` provide management operations. Old command names are removed without aliases.

`send --to all|DEVICE` goes through the normal sync path. `CommitRequest.DeviceIDs` limits the send to named paired devices; unknown, repeated or empty selections are `invalid_argument`. Only peer-session backends honor it.

`send` without `--to` and `recv CODE|LINK` run a temporary transfer with a peer that has no unic device identity, such as the page at `try.uniclipboard.app`. It speaks the page's pull protocol (`internal/webtransfer`: frames, offer, SHA-256 verification, 20 MiB and 50 items per transfer). The sender listens on a fresh random-key tailcat endpoint (`internal/tcnet/ephemeral.go`) that accepts every virtual port and serves one receiver. When rendezvous accepts the ticket, a 6-digit code aliases it for at most five minutes with ticket kind `uc-web-try`; otherwise the sender prints only a link. Bare tailcat addresses cannot initiate a pull. `recv` accepts links or ticket fragments (`#c=…&k=…`); `recv` refuses a `unic-pair` code and `device add` refuses a transfer code.

The receiver admits the offer through `Engine.AdmitTransfer` before reading data, writes files into a private inbox directory and completes into the ordinary received pipeline. History origin is `external` with plugin ID `unic.transfer`. The transfer never creates a device or trust. The sender keeps its prepared transfer payload in memory. Delivery means that the receiver verified payload hashes and sent a protocol ACK. The ACK precedes receiver file placement and pipeline completion; it does not prove history persistence or system clipboard application. Addresses, tokens, codes and links are never logged.

The plugin manager installs public GitHub Go and Lua plugins from a pinned commit. It prefers a verified release asset and permits a source build only with `--build`. Use `unic plugin list` for installed and active versions, and `unic plugin settings` for revision-checked configuration through the local service. See the [installation contract](../contracts/plugin-contract.md#8-从-github-仓库安装).

The marketplace uses the service installation API to activate eligible Go
plugin updates in the current service process. The CLI install command updates
the installation on disk for activation at service startup.

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
Desktop startup refuses a CLI-temporary generation and leaves its clients running. It never promotes or replaces that generation. Retry after the command retires its temporary service.

Commands that start a paired-device send observe only their own Engine operation and report `success`, `partial`, `failed`, `superseded`, `timeout`, `interrupted` or `none`. An operation-query error returns a command error without a send report; it does not prove that the local commit failed.
Pairing approval, `device add` without an argument, and `dev logs level --for` hold a temporary generation for a bounded, announced window.
Exact DTOs, budgets and exit codes are in the [local service contract](../contracts/local-service-contract.md#service-lifecycle).

## First CLI use

Every CLI data directory requires explicit migration initialization. Business
commands refuse with exit 6 (`migration_required`) before service acquisition.
Run `unic migrate --help`, then complete a verified import. When no known v1
source exists, use `unic migrate --initialize-empty --json` explicitly. Pending
imports cannot use that path. Help/version/migration remain accessible. Receipt
publication and service acquisition share the profile transition lock; acquisition
rechecks initialization after waiting. Partial, failure, cancellation and dry-run
never grant permission. Explicit Desktop imports share this gate; welcome/skip
does not initialize the CLI. Source identity prevents another source from completing
a pending import. See the [CLI initialization contract](../contracts/local-service-contract.md#explicit-cli-migration-initialization).

Offline import fills unset v2 display name, theme, anonymous usage-event
preference and tag presentation settings. See the
[v1 preference contract](../contracts/history-contract.md#v1-preference-preservation).

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
- A tray adopts an existing standalone service without stopping or owning it.
- A tray starts a desktop-owned service only when no service owns the profile. Tray exit or abrupt death closes that service owner pipe.
- Manual launch opens the GUI in either mode. Normal GUI close retains tray and service; lightweight GUI close retires all Desktop roles and retains only the service.
- Explicit background entry retains tray/service in normal mode and only the service in lightweight mode, after setup admission. Normal login startup uses its saved background preference; lightweight login keeps only the service.
- GUI and quick-panel processes never acquire a second clipboard adapter or register a global shortcut.
- CLI and desktop access the service through shared local DTOs and clients.
- The CLI does not launch a GUI or silently replace system clipboard reads with history reads.
- A CLI command stops only a temporary generation whose last lease it released. It never stops a standalone or desktop-owned service.
- The launcher never unlinks a socket or kills a process it did not create and verify.
- On macOS, Desktop explicitly enables the native clipboard adapter in `unicd` over the owner-only local socket. The adapter loads AppKit on demand and accesses the pasteboard only for the active console user. Tray retains the menu, shortcut and target capture. A bundled macOS service may also take over pairing notifications after Desktop opts in. It keeps its subscription through lightweight handoff. A Desktop borrowing an unbundled service sends notifications from the tray. On other platforms, the tray hosts the adapter. The service owns submission, sync and history policy on every platform.
- Reconnect establishes a baseline and does not apply disconnected backlog.
- A lost write result is uncertain and is never automatically retried.
- Snapshot uploads, session leases and pending commands have fixed limits.
- A service restart invalidates leases and sessions. A prior system clipboard file lease does not survive restart.

## Code map

| Path | Responsibility |
|---|---|
| `app/service/cmd/unic` | CLI, streams and explicit export |
| `app/service/cmd/unicd` | Service lifecycle, socket server and opt-in macOS clipboard adapter |
| `app/service/internal/host` | Engine assembly, session ownership, local request handling |
| `app/desktop/cmd/unic-desktop` | Launcher and independent tray, GUI and quick-panel roles |
| `app/desktop/internal/desktopprocess` | Profile-scoped role locks, private activation IPC and OS peer executable verification |
| `app/desktop/internal/board` | Desktop mygo clipboard adapter |
| `internal/nativeboard` | macOS pasteboard access without the desktop UI runtime and shared source-application icon processing |
| `app/desktop/internal/quickpanel` | Desktop-only target sessions, adapter-write receipts, native paste and global-shortcut ownership |
| `app/service/internal/server` | Service-owned HTTP routes and socket listener |
| `internal/localapi` | Shared protocol DTOs and clients |
| `internal/localconfig`, `internal/localpaths` | Shared local configuration and state paths |
| `app/desktop/internal/lifecycle` | Serialized service acquisition, parent pipe and saved login intent |
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
The development archive builder does not supply signed platform installers, standalone service-manager registration or store publication. Separate Desktop format construction does not establish those release gates.
The tray registers login startup through mygo. macOS requires an application bundle.
The flat development binary archives are not macOS application bundles. The production Desktop stager constructs a macOS bundle, but signing/notarization acceptance and a selected public CLI lock remain open.
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

The normal desktop uses the approved mygo composition. Fresh profiles complete first-install setup or skip optional v1 migration before the normal service and clipboard adapter start. Established profiles and completed setup open history directly. There is no recurring passphrase-unlock flow. See [first-install behavior](overview.md#live-desktop-composition).
The service remains the Engine owner; the view uses the shared local socket client.
The live binding and acceptance boundary are described in [repository overview](overview.md#live-desktop-composition) and the [execution receipt](../exec/desktop-live-data.md).

## Desktop startup settings

The shared `config.toml` has a `[desktop]` section. Engine does not consume it.
`language = "zh-CN"` selects the default desktop language. Supported alternatives are `en`, `ja`, `pt-BR` and `ru`. Language changes apply through normal configuration refresh without a restart. See [desktop languages](overview.md#desktop-languages).
`mode = "normal"` is the default. A manual no-argument launch always opens or activates the GUI in either mode. `unic-desktop --background` selects the background entry. Normal mode retains tray, the windowless quick-panel role and unicd. Lightweight mode retains only unicd. At login, normal mode uses `desktop.start_in_background`. True omits the GUI, while false opens it. Lightweight login startup always keeps only unicd. Fresh or pending setup always opens the GUI first and cannot be bypassed by mode or background entry. `desktop.start_in_background` applies only to normal-mode login startup and never suppresses manual activation. Explicit `--background` always overrides it for that launch.

The tray's Open main window command starts or activates the GUI. Normal GUI close releases its renderer and subscriptions while keeping tray integration. Lightweight GUI close releases the GUI, tray and quick-panel processes and leaves only the service. The normal tray prepares the quick-panel process asynchronously. A panel request starts it on demand if needed. Dismissing the panel releases its view and windows. It owns no adapter or global shortcut. Explicit application Quit closes all Desktop roles, unregisters the shortcut and stops only a tray-owned service. A macOS service already enabled by Desktop keeps the adapter for its own lifetime after ownership transfer or when the tray releases a borrowed service. This includes lightweight login and lightweight GUI close. Ordinary standalone and CLI-temporary launches start without the adapter. Desktop may opt in an existing standalone service without replacing it. A CLI-temporary service cannot opt in.

`unic-desktop --mode normal|lightweight` selects a background startup policy without changing the saved mode. `--background` selects background entry explicitly. The no-role manual entry requests a visible GUI regardless of saved mode or background intent. Internal `--role tray|gui|quick-panel` entries use the same executable. `unic-desktop-demo` remains an independent fixture and manages no service.

The launcher resolves `unicd` beside its executable. A profile transition lock serializes service acquisition. The tray validates the sibling and service directory before acquisition. It adopts an existing standalone generation and returns no owned-service handle. A live desktop-owned or CLI-temporary generation is refused. With no service, the tray starts `unicd --desktop-owned`, verifies PID, directory and ownership mode, and retains the exclusive stdin lifetime pipe. Only directory-in-use exits from a retiring service receive a bounded retry. Unknown listeners and mismatched profiles are never stopped or unlinked.

Lightweight GUI close transfers a tray-owned service to standalone ownership without restarting it. The tray writes the strict release command and closes its exclusive owner pipe. The service requires the exact command followed by EOF before changing ownership; ordinary EOF still cancels a desktop-owned service. The tray uses existing `GET /v1/service` to confirm the same generation, PID and directory now report `standalone`. It then drops its owned-service handle. An uncertain acknowledgement is resolved by querying that generation, never by stopping or restarting the service. A previously borrowed standalone service needs no transfer.

Role ownership uses live kernel locks and private IPC rather than PID files. Unix endpoints are owner-only sockets in a private per-user runtime directory. Windows endpoints are named pipes restricted to the current user SID. Requests bind the canonical profile, role, executable path and SHA-256. The transport verifies the peer PID and executable through the OS, reusing the cached executable SHA-256 only while the resolved path keeps its initial OS file identity (device/volume, inode/file index, size and timestamps) and re-hashing on any change. A different profile, installation or executable build cannot activate an existing role. Runtime state stays profile-scoped; Engine remains the only history owner.

A GUI or quick-panel crash leaves tray integration active. The next request creates the missing UI role. A tray crash releases its native registrations and closes its owned-service pipe. Existing UI clients reconnect after a later desktop launch recreates the tray. Service recovery renews the adapter session and establishes a fresh clipboard baseline; it never applies disconnected backlog. See the [process design](../exec/desktop-process-design.md) for ownership and failure policy. Native focus, tray clicks and receiving-application paste require separate platform evidence.

Migration pauses tray integration and closes the quick panel before offline import. Tray admission serializes pause with panel activation and persists pending setup before import. It stops only its owned service; an independent service blocks migration. Completion requires explicit entry and a ready service. Cancellation keeps the pending setup marker.

The tray watches changed login intent through `mygo.App.SetOpenAtLogin`; `mygo.App.OpenAtLogin` supplies actual OS status separately. Its product name and login identity remain stable across roles. Internal profile propagation does not turn the default profile into a custom login configuration. User-selected custom profiles remain unsupported by the native login API. macOS requires an application bundle. The profile-local `desktop-login.json` records successfully applied intent. Unchanged intent preserves an external system disable; explicit retry can reapply it. A failure retains the prior receipt and remains visible in the GUI. Changes made without a tray wait for the next tray session. The login entry uses the same launcher and reads the saved mode. Normal login uses the saved `desktop.start_in_background` preference; lightweight login always remains service-only. Manual shortcuts and macOS reopen activation always request the GUI.
See the [startup acceptance receipt](../exec/desktop-startup-lifecycle.md). Native login execution remains a separate acceptance gate.

The container-based production acceptance job trusts only its checked-out repository path before building. Go VCS stamping remains enabled. The acceptance manifest continues to record source and executable hashes.

## Explicit directory send

`unic send --to all -f /absolute/path/to/directory` sends one directory selection through paired devices. Multiple file and directory arguments form one batch. `unic history send REF --to DEVICE|all` prepares a source reference or resends a retained immutable snapshot. It does not restore the item to the local clipboard. Directory arguments require `--to`; temporary code/link transfers continue to accept ordinary files only.

Directory source preparation runs in Engine outside clipboard polling. Async local submissions expose preparation stages and explicit cancellation. Client disconnection does not cancel admitted work. A host-owned cancellation context and `DELETE /v1/submissions/{request_id}` cancel the operation without replay. A cancellation after local commit preserves the committed history and cancels remaining transmission where possible. See the [local service contract](../contracts/local-service-contract.md) and [directory contract](../contracts/directory-contract.md).

## Installation paths and development isolation

`UNIC_HOME` explicitly selects the data directory and takes precedence over portable/default paths.
Without it, a regular `portable.dat` file beside the resolved executable selects its sibling `data/` directory. A marker symlink or directory is rejected.
All bundled programs use the same portable resolver. `UNIC_SOCK` remains an explicit socket override; otherwise the socket remains inside the selected data directory and retains native Unix-socket path limits.
Released executables default to the user configuration directory's `unic/`. Version-`dev` executables default to `unic-development/`.
The raw Desktop development entry uses `UniClipboard Development` for single-instance identity; production staging selects `UniClipboard`.
These paths are separate from v1 data. Program replacement never imports v1 history or credentials automatically.

## Configured external sync instances

`unic plugin instance add IMPLEMENTATION INSTANCE` creates a disabled instance of an installed Go sync backend. `unic plugin settings show|save INSTANCE` addresses its separate Schema settings. Enable with `unic plugin enable INSTANCE`, then restart the service. TOML records the implementation reference under `plugins.instances."INSTANCE".implementation`; policies stay under that instance's `.sync`. No plugin is installed or executed merely because a ticket names it.

`unic device backends` lists implementation identity, runtime identity, transport kind, pairing support and local runtime state. `unic device addr --backend INSTANCE` and `unic device add [CODE|TICKET] --backend INSTANCE` choose local instances explicitly. The receiving instance may have another ID but must use the ticket's implementation. No compatible selection means an explicit error, never Tailcat fallback.

`unic device list --json` includes sanitized binding IDs, revisions and sync state. `unic device path pause|resume|remove BINDING_ID REVISION` operates on one path; device removal still revokes every path. Zero configured backends preserve local history and Actions. The [workflow receipt](../exec/sync-plugin-workflow.md) records real CLI/service/child-process acceptance and the boundary to unsupported production transports.

### External-channel registration

`unic device channel-add INSTANCE NAME` explicitly grants one external backend
instance a named local channel. The service owns channel identity and the
revision-checked pause, resume, and removal commands. An installed process can
check current channel authorization with the plugin RPC before serving a
published representation. See the [local service contract](../contracts/local-service-contract.md#local-external-channel-registration).
