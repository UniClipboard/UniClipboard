# Local service and desktop adapter contract

## Scope

`unic` and `unic-desktop` communicate with the single `unicd` that owns a private state directory.
The protocol uses HTTP over the existing owner-only Unix socket, including AF_UNIX on supported Windows hosts.
There is no TCP control listener. Socket access grants same-user control; an adapter session is a concurrency mechanism.
Protocol version remains 1.

## Headless operations

| Method and path | Request | Result |
|---|---|---|
| `POST /v1/submissions` | `{request_id,snapshot?,history_id?,device_ids?,async?}` | Engine `SubmissionResult` |
| `GET /v1/submissions/{request_id}` | None | Current Engine submission result |
| `GET /v1/capabilities` | None | Engine `Capabilities` |
| `GET /v1/received` | None | Engine `ReceivedContent[]` |
| `GET /v1/received/{receipt_id}/content?part=N` | Zero-based part, default 0 | Content stream |
| `DELETE /v1/received/{receipt_id}` | None | Dismiss transient record |
| `POST /v1/history/{id}/actions/{key}` | `{request_id,apply,cancel_on_context?}` | `{item,submission,applied,apply_error?}` |

A snapshot has `Parts` with `Format` and base64 `Data`, and `Files` with absolute `Path`, `Name` and `Size`.
File paths refer to same-user local files. Engine imports stable references, checks changes on reads, and owns no source file.
A request body is limited to 96 MiB. Representation and file limits also follow the Engine content contract.
No operation interprets newest history as the current system clipboard.

The caller supplies a nonempty submission `request_id` of at most 128 bytes.
The service admits at most 1024 explicit request records and expires completed records after 30 minutes.
The service compares the submitted snapshot digest before importing it again.
A retry with an identical snapshot returns the first result. A different snapshot conflicts. The host cache hashes the snapshot, history ID and ordered device selection. Changing any of them conflicts. The async wait preference does not change the request identity.
Client disconnection does not cancel admitted processing. Service shutdown cancels the waiting host operation.
The CLI prints a generated key to stderr before sending. Scripts that require retry control supply `--request-id`.
The request cache and Engine operation handles do not survive a service restart.
A committed history item and its identity remain durable under history policy.

`queued` means network dispatch admission. It does not prove remote receipt or system application.
`no_targets`, `no_backend`, `disabled` and policy dispositions remain successful local commits.
Received records default to a 30-minute TTL and 32 entries. They use Engine TTL and capacity limits, do not survive restart, and may have no persisted history.
Reading content retains and releases a lease, does not apply it, and does not dismiss the record.
A missing or expired receipt returns `not_found`; an invalid part returns a client error.

An Action defaults to no system application.
`apply=true` checks adapter ability before creating a new item.
A successful Action with a later application failure returns HTTP 200 with its item, submission and `apply_error`.
The service never reports that committed Action as an execution failure.
An explicit Action RequestID deduplicates execution; an explicit apply retry may issue a new local application request.
History restore retains the existing `sync.on_restore` contract. Action application does not trigger restore sync.

## Local event delivery

`GET /v1/events` is a best-effort SSE notification stream. Each subscriber has a 32-event queue; overflow drops notifications without blocking the service. Events contain no replay cursor or monotonically increasing sequence. The stream provides no replay or `Last-Event-ID` recovery; its 20-second comment heartbeat only keeps the connection active.

Clients must reconcile authoritative state after interrupted subscriptions and must tolerate missed notifications while a connection remains open. Desktop uses event-triggered reads plus five-second background reconciliation. These background reads do not show foreground loading or replace existing content with a loading placeholder. Pure event-only refresh requires a separate reliable recovery contract before periodic reconciliation can be removed.

## Adapter session

`POST /v1/adapter` accepts `{session?,method,value?}` and returns `{value?,error?}`.
The error is an Engine error with a stable code; platform exception text is excluded.

1. `register` returns a random session generation. A live owner causes `in_use`.
2. `capabilities` replaces the host clipboard capability declaration.
3. `import` converts a snapshot into a session-owned Engine lease.
4. `observe` implements the Engine baseline/change protocol. A new epoch begins at sequence 1.
5. `ready` snapshots and returns all existing receipt IDs, then enables desktop command dispatch after a successful baseline. The adapter excludes those IDs before starting automatic application.
6. `heartbeat` renews the ten-second session deadline. The desktop sends it every two seconds.
7. `close` releases the generation and its resources. A one-second service sweep expires abandoned generations.

The same session also supports `retain`, `release`, `received`, `dismiss`, `reconcile`, `submit`,
`apply`, `apply-history`, `report`, `restore-sync`, `path`, and `reference`.
These methods use the corresponding Engine DTOs and retain the Engine's single application slot.
`GET /v1/adapter/content?lease=...&part=...` requires the generation in `X-Unic-Adapter` and streams a leased part.
Paths and content are available only through explicit requests; state events carry neither.

The service tracks at most 128 leases and 16 pending desktop commands per generation.
All local content endpoints share a four-stream pool and an eight-second stream/write deadline.
Adapter uploads allow at most eight concurrent requests before decoding, with a 15-second HTTP body read deadline.
Adapter RPC operations have an eight-second deadline.
`next` waits up to eight seconds for a command and returns an empty command on timeout.
`result` completes the command by ID. Missing or completed command IDs conflict.
An expired queued command is not dispatched. A dispatched write whose response is lost has an uncertain outcome.
The caller must not infer success or automatically replay that write.

Clipboard reads, explicit copy, history application and restore execute in the active clipboard adapter. macOS hosts the adapter in `unicd` after explicit Desktop opt-in. Other platforms host it in the tray.
The adapter serializes these operations with observation and automatic application.
A write succeeds only after actual read-back and an accepted Engine application result.
Copy writes, establishes a new baseline and submits once, even for unchanged content.
Automatic capture uses monotonically increasing observation sequences.
Applied content uses the Engine application ticket and read-back baseline to prevent echo.
Only file application leases remain held until the clipboard changes.

Session expiry withdraws Observe/Apply capabilities, fails pending commands, closes an outstanding application,
and releases session-owned leases. Old generations and their late results return `resource_expired` or `conflict`.
A service restart invalidates all sessions, handles and application tokens.
The service does not retain a system clipboard lease across restart.
On reconnect the adapter reads a fresh baseline, does not broadcast that content, and ignores the receipt IDs returned by the service readiness snapshot. A failed receipt snapshot cannot establish readiness.
The adapter never replays disconnected backlog into the system clipboard.
Normal GUI close terminates the GUI process and leaves clipboard capture and application running. Lightweight GUI close also terminates tray and quick panel, unregisters the shortcut, and retains the same service generation after confirmed ownership release. Explicit application Quit stops only an owned service. An enabled macOS service retains its adapter until service exit, including after ownership release and after a borrowing Desktop exits. Other platforms release their tray-owned adapter at tray exit. Manual activation opens the GUI again.

On macOS, the local Desktop client uses `POST /v1/desktop/clipboard/enable` to opt in after setup. An existing standalone service can opt in without changing its generation. A CLI-temporary service rejects opt-in. The repeated request uses the same adapter. The first successful response follows the ordinary adapter baseline and readiness barrier. Ordinary service startup is headless and needs no graphical session. Clipboard access requires the active console user. The setting is in memory and resets when the service stops.

Quick Panel uses the owner-only service socket for tracked write and match on macOS. The panel calls `POST /v1/desktop/clipboard/apply` with a history ID and a plain-text flag. The response carries a write mark or a stable failure category. `POST /v1/desktop/clipboard/match` checks the mark immediately before the Desktop sends native paste input. Neither operation records clipboard contents or paste targets.

On macOS, `POST /v1/desktop/notifications/enable` returns a boolean identifying whether the daemon accepted notification responsibility. A bundled service for the active console user subscribes once per daemon generation. It applies the current Desktop language and submits only new pairing events to the native notification center. System authorization is requested in response to the first pairing notification. An unbundled, retiring or CLI-temporary service declines responsibility. A running tray may provide notifications for an unbundled service. Neither the endpoint nor the daemon logs notification text.

## macOS Desktop updates

`POST /v1/desktop/update/check` accepts the current Desktop version and its public archive verification key. It selects the release from the configured stable or alpha feed, using the service host target and current Desktop language. It returns a manifest and an opaque selection token. The endpoint uses the fixed release origin and takes no archive URL from the caller. A CLI-temporary service or an inactive console session cannot check or fetch a Desktop update.

`GET /v1/desktop/update/archive/{token}` streams the selected archive only after verifying the advertised size, digest, signature and embedded numbered release identity. The daemon admits one check or archive operation at a time. A later successful check replaces the previous selection. Desktop completes and verifies the local transfer with its embedded public key before allowing shutdown or installation. Download errors do not retire Desktop processes.

## Export and CLI behavior

`unic send [text]` reads stdin when text is omitted. `-f` submits local file references; `--png PATH` imports PNG.
`unic dev submission REQUEST_ID` queries a result. `unic recv list|show|dismiss` exposes transient receives.
`unic history action ID KEY [--apply] [--request-id KEY]` separates execution from application.
`unic dev copy`, `paste`, `history use`, and Action `--apply` require desktop abilities.
No adapter yields `unavailable`; unsupported capabilities yield `unsupported`; denied access yields `permission_denied`.
CLI success exits 0. Operation failures and general syntax errors exit 1. Invoking `unic` with no command exits 2.
Submission and Action JSON expose stable codes and independent commit/network/application results.

`unic history export ID --output DIR` executes in the CLI process.
It streams every part into an `os.Root` rooted at DIR, uses exclusive creation, and rejects unsafe names.
Directory snapshots use the [directory export contract](directory-contract.md#cli-snapshot-export), including distinct root containers and rollback. Directory references require an explicit snapshot first.
For other content, file parts retain their declared names; representations use `part-N.txt`, `.html`, `.png`, or `.bin`.
Duplicate names, existing files, symlinks at output file names, and path escape fail instead of overwriting.
A failed export removes only files created by that export.
Exported copies belong to the user and are independent of Engine cleanup.

## Service lifecycle

`GET /v1/service` returns `{id,pid,directory,mode,started_at_ms,leases,retiring}` on the owner-only local socket.
`id` is a random 128-bit generation identifier. `directory` is the service state-directory identity.
`mode` is `standalone`, `desktop` or `cli-temporary`. `leases` counts live CLI usages. `retiring` is true once admission is closed.
`POST /v1/service/stop` accepts `{id}` and rejects a mismatched generation with HTTP 409.
A successful response flushes `{ok:true}` before requesting service cancellation.
It acknowledges shutdown admission, not completion. Socket removal follows Engine closure and lock release.
The caller waits for socket removal before starting a replacement.
No lifecycle endpoint is exposed over TCP. Socket permission remains the same-user authority boundary.
A launcher does not stop a service whose reported directory differs from its selected profile.

### Lifetime modes

- `unicd` (standalone) ignores stdin. CLI commands borrow it and never stop it.
- `unicd --desktop-owned` reserves stdin for an anonymous tray-lifetime pipe. Empty EOF, a read failure, a wrong marker, a truncated marker or extra bytes retire and cancel the service, including after abrupt parent death. Only the exact `unic-desktop-release-standalone-v1\n` message followed by EOF releases the exclusive owner. The lifecycle mutex changes the same non-retiring generation from `desktop` to `standalone`; Engine, PID, generation, directory and history remain unchanged. A retiring generation cannot be released. CLI commands borrow this generation and never stop it.
- `unicd --cli-temporary` reserves stdin for the launcher's initial ownership pipe and lives while at least one lease exists.
  Any other argument list exits with code 2.

Both Desktop modes use the same tray service-acquisition policy. An existing same-profile standalone generation is borrowed without replacement or an owned-service handle. With no service, the tray starts a desktop-owned generation and retains its lifetime pipe. A live desktop-owned generation and a CLI-temporary generation are refused without stopping them. A retiring generation receives a bounded acquisition retry. GUI and quick-panel processes never own this service pipe. Explicit Desktop Quit never stops a borrowed standalone generation.
The tray can release its owned generation when lightweight GUI closure ends Desktop integration. `Process.Detach(ctx)` writes the fixed marker once and closes its writer. It confirms release only through `GET /v1/service` reporting the original generation, PID and directory as non-retiring `standalone`. A repeated detach queries that original identity; it never writes a second marker. A timeout or lost response remains uncertain. It never authorizes a service stop, kill, replacement or retry against another generation. Cleanup after confirmed detach is a no-op for the released service; cleanup after an unconfirmed attempt refuses to stop or kill it. Independent service acquisition has no owner handle and never uses this protocol.

A direct `unicd` start meets the exclusive profile lock. It does not take over active temporary clients.

### Exclusive owner release failure specification

Specified before implementation: release must retain the same generation, PID,
state directory and durable history. A normal EOF or owner crash must still stop
a desktop-owned service. A truncated or unrecognized release marker must stop it.
A valid release must not revive a retiring generation. Only the exclusive inherited
pipe can release ownership. A repeated or uncertain detach must only query the
original generation; it must never kill, restart or stop a replacement generation.
After confirmed release, normal owner cleanup must leave the standalone service
running. Borrowed standalone services have no owner handle and are never stopped.

### Usage leases

A lease admits one CLI invocation to exactly one generation. The open HTTP response is the lease.

| Method and path | Request | Result |
|---|---|---|
| `POST /v1/service/leases` | `{id}` (generation) | 200, then a held stream whose first line is `{"lease":"<local id>"}` |
| `DELETE /v1/service/leases/{lease}` | `{id}` (generation) | `{"last":bool}`; `last` is true when this release retired a temporary generation |

Closing the stream releases the lease, including when the client process is killed.
There is no durable counter, lease file or idle timer. A lease identifier is local control material and never enters logs, telemetry or status output.
A mismatched generation returns 409 with reason `generation_changed`. A generation that closed admission returns 409 with reason `retiring`.
The service admits at most 64 leases (`servicelifecycle.MaxLeases`). More requests return 429 with reason `resource_exhausted`. Capacity returns after release.
Leases exist in every mode. Only a `cli-temporary` generation retires.

A temporary generation starts owned by its launcher. The service does not exit while that claim exists.
The first admitted lease replaces the initial claim. The launcher then closes its ownership pipe.
If the pipe reaches EOF before any lease is admitted, the initial claim ends and the service retires. This covers a launcher that dies before readiness.
The last release of a temporary generation closes admission and answers `last:true` before the service cancels itself. Admission and last release are decided under one lock:
a client either obtains a lease in the live generation or receives `retiring` before it submits business work.
After `retiring` or `generation_changed` a client retries acquisition only. It never repeats a business request.

### Launcher rules

1. A launcher performs local argument parsing first. The `recv` source shape (6-digit code or transfer link) is also checked locally. State-dependent validation, and resolving a code, occur in the service and can fail after acquisition. A service is acquired by the first service request, not earlier.
2. It takes the profile transition lock (`service-transition.lock`, shared with desktop startup), probes `GET /v1/service`, and holds the lock only for probe, optional launch and lease admission.
3. An authoritative absence (no listener) permits launch. EOF, unexpected EOF, connection reset and broken pipe during control-plane probing or lease admission are retried within the acquisition budget before any business request. This does not authorize replay of business requests. A timeout, a permission error, an unrecognized listener and a different `directory` are errors. The launcher never unlinks a socket or kills a process in those cases.
4. It resolves `unicd` beside its own executable. It never searches PATH. The child receives `UNIC_HOME` and `UNIC_SOCK` explicitly, its own detached session, and the initial ownership pipe as stdin. Child output goes to `cli-daemon.log` in the state directory (truncated at each launch).
5. Readiness requires the launched PID, the profile `directory` and `mode: cli-temporary`. Socket existence is not readiness. Exit code 75 is retried every 100 ms inside the startup budget.
6. After the first lease, the launcher closes the ownership pipe. The generation then lives by leases only.
7. On release the client sends `DELETE` and waits for the generation to stop only when the answer is `last:true`.
   A launcher that created the generation, and only that launcher, may kill that process after the graceful budget. A borrower reports the failure instead.

Frozen budgets (client side): probe 1 s; acquisition including retirement wait 45 s; one launched `unicd` becoming ready 30 s;
last-user graceful stop 12 s plus 5 s after a forced kill of the launcher's own child; one non-streaming request 60 s, starting after acquisition.
The event stream, `dev watch`, invitation and override sessions are not subject to the 60 s request deadline.
Cleanup uses its own context even when the command context is cancelled.

### Operation observation

`GET /v1/operations/{id}` returns the Engine `Operation` for an operation created by this generation. Operations are in memory and do not survive restart.
A command that initiates a send waits only for the operation it created. `SubmissionResult.SyncDisposition=queued` is not completion.
The client polls every 200 ms until the operation is done, 60 s pass, or the command is cancelled. It never resubmits.
Reported result values are `success`, `partial`, `failed`, `superseded`, `timeout`, `interrupted` and `none` (no operation: disabled, no backend, no target). An operation-query error produces an internal `unknown` result, but the handlers return the error before emitting the send report; the CLI exits 1. The local commit may already exist.
Local commit and network outcome stay separate. `send --json` prints `{submission, send:{outcome, operation?}}`.
Exit code 4 means committed but not fully completed: `partial`, `failed`, `timeout` or `interrupted`.
`success`, `superseded` and `none` exit 0. After a timeout under a temporary service the message states that the service is stopping and that referenced files stay in use until the command exits.
Under a persistent service it names `unic dev submission REQUEST_ID`.

Before a command sends, the client gives paired devices a connection opportunity:
while any paired device is offline and fewer than 10 s have passed since the service generation started, it polls status every 200 ms.
The same rule applies to every service, so completion does not depend on who started it. Offline and pending devices cannot hold a command beyond the window.

### Session commands

| Command | Under a temporary service | Under a persistent service |
|---|---|---|
| `dev watch` | Holds a lease for the stream. A stream that ends without cancellation exits 4 (`stopped or was replaced`). | Same. |
| `device add ADDRESS`, `device add CODE` with approval pending | Waits up to 2 minutes for the peer to connect. Connected exits 0. Pending, removed, interrupted or stopped exits 4. | Returns after the pending request is recorded. |
| `device add` (no argument) | Holds the lease until the invitation expires, is used, is cancelled or is replaced by a different code. | Returns after printing. |
| `device addr` | Prints the address and notes on stderr that it works only while a service runs. | Returns after printing. |
| `dev logs level LEVEL --for D` | Holds the lease until that override expires or is reset or replaced (its `expires_at_ms` changes). | Returns after acceptance. |

An older command's exit never withdraws a newer invitation or override: a command compares the code or the expiry it created.
The held duration is announced on stderr. Stdout stays the command result.

### Temporary transfer routes

| Route | Meaning |
|---|---|
| `POST /v1/transfers` `{snapshot}` | Start an offer. Returns `{id, state:"waiting", link, code?, expires_at_ms, items, bytes}`. Only this response carries the link and code. Limits: 20 MiB, 50 items; larger content is 413 `limit_exceeded`. |
| `GET /v1/transfers/{id}` | `state` is `waiting`, `sending`, `delivered`, `failed` (with `reason`), `expired` or `cancelled`. A finished transfer stays queryable for 5 minutes. |
| `DELETE /v1/transfers/{id}` | Cancel. |
| `POST /v1/receive` `{source}` | Pull from a 6-digit code or a link. A 90 s budget covers dial/pull after ticket resolution; subsequent file placement is outside that context; the CLI exempts it from the 60 s request deadline. Returns `{items:[{kind,name,mime,size,text?}]}`. Failures carry reasons `invalid`, `expired`, `rate-limited`, `unavailable`, `pairing_code`, `busy`, `too-large`, `integrity`, `peer-cancelled`, `timeout`, `connection-lost`. |

The sender reports `delivered` only after a successful hash-verification ACK. The receiver sends that ACK before host file placement and `TransferInbox.Complete`; delivery does not prove history persistence or clipboard application. Receive completion remains subject to pipeline policy.

`POST /v1/submissions` accepts `device_ids` to limit a send to paired devices. `send` without `--to` holds its command (and a temporary service) until the offer is delivered, fails, expires or the user interrupts; an offer that did not deliver exits 4.

### Exit codes

0 done; 1 failed (also general syntax and wrong-ticket-kind errors); 2 no command (migration has its own usage handling); 3 migration partial; 4 committed but not fully completed; 5 command done but stopping its temporary service failed.

A directory-in-use startup failure exits `unicd` with code 75.
Other daemon startup failures exit 1. Invalid daemon arguments exit 2.

### Command requirements

Every leaf command declares one requirement in `app/service/cmd/unic/requirements.go`: `local`, `service`, `service+clipboard` or `offline`.
Aliases resolve to the same leaf. `history action` becomes `service+clipboard` with `--apply`.
`unic help --json` prints the catalog. Text help lists commands without requirement badges or lifecycle-window descriptions. This is a known help gap; the declarations still control dispatch.
A `local` command receives a client that fails on any request. `offline` (`migrate`) never starts or stops a service; its target-profile lock decides.
`service+clipboard` commands acquire the service, then the existing adapter errors apply (`unavailable`, `unsupported`, `permission_denied`). The CLI never starts a GUI.
Locally detected syntax, arity and flag errors are reported before any service starts. Values that need service state, such as a configuration key, are checked after acquisition. `recv` source shape is validated locally; resolving a code and rejecting a pairing code happen after acquisition.


## Product analytics ownership

The service owns one PostHog analytics generation and random identifier per profile.
Registration may carry a 16-character alphanumeric `launch_id` for a pending Desktop process startup. The client omits it after the first successful ready acknowledgement, including later reconnects. The service keeps only the last ready launch key to suppress duplicate ready calls and retries after an uncertain acknowledgement. Registration without a launch key is valid and does not announce a new Desktop startup. The launch key is local-only and never enters telemetry.
The adapter never receives the analytics identifier or ingestion token.
Action, send, pairing and explicit history-application events come from authoritative Engine completions, not activity notifications.
`GET /v1/diagnostics/telemetry` includes analytics health and pending/loss counters, with no identity or credential values.
See the [observability contract](observability-contract.md#10-posthog-product-events).

## Offline log archive

`unic dev logs export [--output FILE]` reads the selected profile's retained local logs
without IPC. It works when unicd is stopped. Default output is a UTC-timestamped
`unic-logs-*.tar.gz` in the current directory. Success prints the absolute path.
The command never replaces an existing output, follows a log symlink, or includes
profile configuration, credentials, database or clipboard history. Archive membership,
size limits, checksums and live-snapshot semantics follow section 12 of the
[observability contract](observability-contract.md#12-offline-local-log-export).

## Explicit history search reads

The shared client exposes three POST routes through the service-owned Engine:

| Route | Request | Response |
| --- | --- | --- |
| `/v1/history/search` | `HistorySearchRequest` | Service history metadata plus bounded preview text/highlights, next cursor, collection revision and evaluation time |
| `/v1/history/search/candidates` | `HistorySearchCandidatesRequest` | A bounded tag/source/extension catalogue page and next cursor |
| `/v1/history/search/counts` | `HistorySearchCountsRequest` | Exact counts for at most 32 complete queries, one collection revision and one evaluation time |

Bodies are limited to 1 MiB and reject unknown fields or trailing JSON.
Each route admits at most four concurrent requests. The Store owns one separate read-only connection to the same WAL database; Engine deadlines include waiting for that connection.
Request text is never put in URLs, errors, logs, events or telemetry.
Search pages contain only explicit bounded previews, not image bytes, host references or absolute paths.
New search revisions serialize as decimal strings. Counts are JSON numbers in request order.
Exact semantics and errors are in the [history search contract](history-search-contract.md).

### Desktop shortcut preferences

The revision-checked configuration accepts `desktop.shortcuts`, a mapping
from supported action IDs to arrays of one- or two-step bindings. Empty arrays
disable an action. Omitted entries select defaults. The existing
`desktop.quick_panel_shortcut` preference remains the preset fallback for panel
activation. `desktop.quick_panel_double_tap` accepts an empty string (off),
`alt`, `control`, or `meta`. Validation rejects ambiguous bindings and retains
the previous revision on rejected saves. The service persists preferences.
The Desktop tray owns system-wide registration, retry and effective status.
No target identity or OS input command enters service DTOs. See the
[Desktop quick-panel contract](desktop-quick-panel-contract.md).

## Desktop language configuration

The existing configuration response and TOML save API include `desktop.language`.
Accepted values are `zh-CN`, `en`, `ja`, `pt-BR`, `ru`, and the empty string (Simplified Chinese).
Missing configuration uses `zh-CN`. Unknown nonempty codes reject the whole configuration and preserve the previous valid state.
The field is host-only and does not change Engine settings, protocol version, device identity or sync payloads.
Saving uses the existing revision check. Desktop consumes the saved state through normal refresh and does not request a restart for this field.

## Directory extension

Directory selection references, preparation, manifests, file-policy grouping, bounds, and acceptance follow the [directory content contract](directory-contract.md). The 0024 execution receipt distinguishes specified behavior from verified implementation.

### Directory submission and cancellation

Exactly one of `snapshot` and `history_id` supplies content. A history source reference is prepared afresh; an immutable snapshot retains its stored bytes. `async=true` returns admission without waiting for preparation. Poll the existing submission endpoint for `phase` and `stage` (`scanning`, `preparing`, `processing`, `sending`, `complete`, or `failed`). Network completion still comes from the operation endpoint. Stages do not expose paths or filenames.

`DELETE /v1/submissions/{request_id}` explicitly cancels a known admitted request, including the interval before Engine admission. Unknown IDs return not_found. Repeated cancellation is safe while the request is retained. Committed history is not rolled back. HTTP disconnection alone keeps the admitted request running. The host uses an independent per-request cancellation context; Engine `CommitRequest.CancelOnContext` opts into that lifetime without changing the default SDK caller-timeout behavior.

The adapter supports explicit leased `tree-paths` and `tree-references` calls. Only the host decodes custom root references. `CopyID` in a local snapshot is an observation marker and never enters peer transport. Tree application rechecks the current clipboard after preparation and before the platform write. Exact tree DTOs and bounds follow the [directory contract](directory-contract.md).

## Source application metadata

Clipboard content and history preserve optional source application identity and cached PNG icons.
See the [source application contract](source-application-contract.md) for capture attribution, resource bounds, and transport semantics.

## Desktop main-window frame preference

`config.desktop.system_window_frame` is a boolean with default `false`. The
revision-checked configuration API, CLI and TOML reload persist the same field.
It is a GUI creation preference, not a service restart requirement. The service
never adds this key to `restart_required`. Desktop captures the saved value when
creating its main window and compares subsequent saves with that window baseline.
Close and reopen GUI to apply a change without restarting tray or service.
Windows, Linux and macOS expose the same preference. A true value uses the normal
native title bar and buttons. Windows false uses custom controls within the
existing application toolbar, with a minimal fallback only on pages without that
toolbar. This field never adds a graphical dependency to the service.

## Desktop tray icon preference

The configuration API exposes `config.desktop.tray_icon` as `glyph` or `cat`. An omitted TOML key uses `glyph`. Revision-checked saves, `unic config set` and direct file reloads use the same enum validation. Invalid values do not replace the previous valid configuration. The service stores the preference and does not render a tray. The tray watches saved configuration while Desktop integration is active and updates its image without requiring restart. Lightweight GUI close removes the tray. `desktop.tray_icon` is not a restart-required key.

## Desktop color mode preference

`config.desktop.color_mode` accepts `system`, `light` or `dark`. An omitted TOML
key uses `system`; other values, including the empty string, reject the complete
configuration. The existing revision-checked save API, CLI and file watcher use
the same validation. Invalid saves preserve the previous valid state. The field
is host-owned, does not change Engine settings and is not restart-required.
Desktop applies the saved value to mygo's shared theme source in each native
role, including startup/setup before the normal history view exists. The
service stores the value without importing or initializing a graphical host.

## Explicit CLI migration initialization

The CLI requires explicit initialization for each resolved data directory before any
business command. The host receipt is `cli-migration.json`; an existing database,
Desktop navigation receipt, environment variable, dry run, or successful service probe
never substitutes for this receipt. A verified explicit Desktop import publishes the same migration receipt. A Desktop welcome/skip action does not. This is a CLI admission rule, not an Engine
or socket API credential. Direct daemon and Desktop lifecycle policies stay separate.

`help`, group help, version and `migrate` remain available. A blocked business
command exits **6** before contacting or starting a service, sync, or clipboard
adapter. JSON mode writes an error to stderr with `reason: migration_required`,
`next_command: unic migrate --help`, and the explicit no-source initialization
command. Plain text prints the same actionable instructions. No interactive
answer or default consent is inferred.

- `unic migrate --source DIR --json` unlocks, imports, reopens and verifies through
  the existing v1 importer. Only an error-free `complete` report publishes CLI
  completion. A partial report exits 3 and keeps the CLI blocked. Failure and
  cancellation exit 1. Dry-run never changes initialization state.
- `unic migrate --initialize-empty --json` is the explicit no-source completion
  path. It checks default v1 discovery and the selected `UC_PROFILE` location,
  accepts an absent path or an empty leftover directory, refuses a nonempty or
  unreadable source, and refuses a pending import. A
  custom source must be imported with `--source`. The command does not infer
  that a custom source elsewhere is absent. It accepts only `--target` and
  `--json` alongside the explicit initialization option.
- Missing/invalid source paths fail before creating pending import state. This
  allows a user who has no v1 data to correct a typo and initialize explicitly.
  Once a valid source import begins, only completion of that source can clear
  the pending state. Before unlock it is bound to the key-slot fingerprint;
  after unlock it is bound to the importer's stable source identity. Moving a
  source preserves its identity. An unrelated source cannot replace it.
- State is scoped to the exact target. Normal commands and migration use the
  same portable/development/`UNIC_HOME` resolver. Path selection is read-only
  until source/target overlap, including symlink overlap, has been rejected.
- Migration holds the profile transition lock, publishes pending intent before
  credentials, probes exclusive offline ownership, and never stops a borrowed
  service. Waiting for the lock and passphrase input can be cancelled. Service
  acquisition checks completion again under that lock. Import and verification
  release the target before atomic completion publication. A failed publication
  leaves pending intent and requires a safe idempotent retry.
- Corrupt or unknown state fails closed. Help stays accessible. A fresh explicit
  verified import can repair a corrupt receipt. No skip or environment bypass
  exists. Atomic publication uses a private synchronized temporary file and rename.

The initialization receipt contains no content or credentials. CLI and Desktop
imports share its pending/source/completion transitions under the same lock. It
is separate from importer idempotence and Desktop navigation. Imported timestamps retain
v1 values. Normal configured age/count cleanup applies after startup; importing
does not silently disable a user's policy. Acceptance with historical synthetic
timestamps explicitly disables automatic cleanup and verifies stable identities
across service restarts. Review cleanup policy before importing old real history.

## Plugin configuration

The service provides `GET /v1/plugins`, `GET /v1/plugins/{id}/settings`, and `PUT /v1/plugins/{id}/settings`. The [settings contract](plugin-settings-contract.md#4-runtime-configuration) defines DTOs, revisions, write-only fields, and activation.

`PluginInfo` also carries the optional manifest display text: `name`, `summary` and `i18n` (`{language: {name?, summary?}}`). The service does not know the UI language; the client resolves it as described in the [plugin contract](plugin-contract.md#展示文本与多语言).

The optional `PluginInfo.icon` string comes from the plugin manifest. The
optional `plugins[].icon` string in the marketplace response comes from the
catalog. Both fields use the [plugin icon syntax](plugin-contract.md#display-icon).

### Marketplace and installation lifecycle

| Method and path | Request | Result |
|---|---|---|
| `GET /v1/plugins/marketplace` | Optional `refresh=1` or `refresh=true` | `{version,source,updated_at?,warning?,plugins}` |
| `POST /v1/plugins/install` | `{repository,ref?,path?,build?,replace?,enable?}` | Installed `PluginInfo` |
| `POST /v1/plugins/{id}/start` | None | Enabled or configuration-pending `PluginInfo` |
| `DELETE /v1/plugins/{id}` | None | `{ok:true}` |
| `POST /v1/plugins/{id}/rollback` | None | Selected `PluginInfo` |

The install request is limited to 8192 bytes. Unknown fields, trailing JSON and
an empty repository are rejected. Boolean options default false. Installation
uses the existing commit-locked GitHub installer. `enable=true` also updates
the plugin configuration; a service restart activates the installed version.
For an inactive installed Go Action-only plugin or external-channel backend
with `live_start: true`, the explicit start route requests activation in the
current service. It enables the instance, starts the verified process and
publishes its capabilities when ready. An instance with missing required
values returns `needs_settings: true` and activates on the first successful
settings save. Sending the same start request to the running configured
version is idempotent. Start failures return an error and publish no runtime.
Uninstall disables the plugin and removes its installation lock entry. Running
code stops and old files are cleaned up after restart. Rollback selects the
retained previous version and also requires restart. History and local plugin
configuration remain intact.

`PluginInfo` includes optional `repository`, `commit`, `has_previous`,
`previous_version`, `events` and `actions`. Each action exposes
`{id,key,title,accepts?,placements?,post_actions?,timeout_ms?}`. The existing
`configured_enabled`, `running_version` and `restart_required` fields remain
the source of activation state.

Marketplace `source` is `remote`, `cache` or `bundled`. A network or registry
validation failure returns a usable fallback catalog with an optional warning.
Each catalog entry includes `id`, `name`, `author`, `category`, `version`,
`summary` and `repository`. Optional fields are `official`, `description`,
`highlights`, `how_to`, `privacy`, `zero_config`, `ref`, `path`,
`build_fallback` and `capabilities`. Catalog metadata never bypasses installer
validation. See the [plugin contract](plugin-contract.md#marketplace-catalog).

Action `cancel_on_context` defaults false. CLI and Desktop opt in for explicit
user Actions. CLI permits up to five minutes plus five seconds of transport
margin for Action requests; other non-streaming requests keep their 60-second
budget. The Engine Action deadline remains at most five minutes. Request
cancellation revokes upload readers when the opt-in flag is true.

## Sync backend selection

`GET /v1/sync-backends` returns an array of `{id, implementation_id, transport_kind, pairing, external_channels, state}` for assembled sync backends. One installed Go sync implementation can have multiple runtime instances. `pairing` describes device-pairing support and never applies to external channels. This projection contains no address, ticket, credential or process-private state. `state` reports local process availability and does not attest to current remote connectivity.

`POST /v1/code` and `POST /v1/pairing-ticket` accept an empty body or `{ "backend_id": "instance.id" }`. An empty selection uses Tailcat. Unknown selections and unsupported transports fail explicitly and never fall back. Backend selection does not grant trust. The existing fingerprint and complete device/binding revision approval still applies.

Pair an installed external-channel backend through `POST /v1/channels/pair`.
Send `{backend_id, code}` for a new pairing or `{backend_id, resume: true}`
to resume an existing provider pairing. The response is the external peer
projection. The pairing code and group context are private. The CLI command
`unic channel pair INSTANCE` reads the code from standard input.
`unic channel pair INSTANCE --resume` requests explicit recovery without a code.

CLI exposes `unic device backends` and `unic device addr --backend INSTANCE`. Desktop exposes the same backend selection in the invitation task. Standard backend policies use `plugins.instances."INSTANCE".sync` through the existing revision-checked configuration route. Enablement changes require service restart. Global sync and policies apply to the assembled running backend set while restart is pending. A zero-backend service retains local content, history and Actions.

The single-instance external-process acceptance and remaining contract decision are recorded in the [workflow receipt](../exec/sync-plugin-workflow.md).

### Instance creation and local pairing selection

`POST /v1/plugin-instances` accepts `{id, implementation_id, revision}` using the current configuration revision. It creates a disabled alias and returns its plugin projection. Conflicts return HTTP 409. Schema/settings routes address that alias. Installation is unchanged. CLI equivalent: `unic plugin instance add IMPLEMENTATION INSTANCE`, then configure, enable and restart.

`POST /v1/peers` accepts `{addr, backend_id?}`; `POST /v1/join` accepts `{code, backend_id?}`. `backend_id` selects the receiver's local instance, which can differ from the issuer's instance. Unknown fields, trailing JSON, unavailable selections and implementation mismatch are rejected without fallback. `unic device add [CODE|TICKET] --backend INSTANCE` uses these same routes. A missing local selection only permits an exact singleton match with the ticket implementation descriptor.

`GET /v1/status` retains one row per trusted device and includes its sanitized `bindings`. Pending approval includes `paths` plus the existing exact pending binding revision set. No address, credential or RPC token is included. Trusted devices remain listed after their last path is removed. Channels retain their separate kind and never gain fingerprints or device bindings.

`PeerStatus` entries in `GET /v1/status` also project:
- `fingerprint`: device identity fingerprint (devices only; omitted for external channels).
- `user_name`: optional user-assigned custom name (omitted when unset; `name` remains the effective display name, falling back to `reported_name`).
- `reported_name`: peer-reported name (omitted when unset).
- `revision`: current Engine CAS revision (populated for both devices and external channels).
- `last_error`: stable, redacted error class for external channels (omitted when empty; raw backend text is never exposed).

`PATCH /v1/peers/{id}` accepts a JSON body of at most 4096 bytes containing a positive `revision` and required `name` string (`""` clears the custom name and reverts `name` to `reported_name`), rejecting unknown fields and trailing JSON. It delegates to Engine `RenameDevice` with CAS and returns the updated `PeerStatus`. Missing or invalid fields, names exceeding 64 bytes, control characters or leading/trailing whitespace return HTTP 400 (`invalid_argument`); unknown peers return HTTP 404 (`not_found`); stale revisions return HTTP 409 (`conflict`).

`DELETE /v1/peers/{id}` accepts an optional `?revision=N` query parameter. When present, `N` must be a positive integer (otherwise HTTP 400 `invalid_argument`) and revocation uses Engine `UnpairDevice` CAS, returning HTTP 409 (`conflict`) without unpairing on a stale revision or HTTP 404 (`not_found`) for an unknown peer. When omitted, the route retains whole-device unpairing against the current revision.

`PATCH /v1/bindings/{id}` requires both `revision` and `enabled`. `DELETE /v1/bindings/{id}?revision=N` removes only that path. Both use Engine CAS and revocation barriers; stale revisions return HTTP 409. `unic device list --json` exposes the current path revisions; `unic device path pause|resume|remove BINDING_ID REVISION` submits them. Desktop device details expose the same per-path switch and removal operation. Whole-device removal still revokes every path.

### Local external-channel registration

`POST /v1/channels` accepts `{backend_id,name}` on the owner-only service socket.
It registers a locally approved channel on the selected external backend
instance and returns the same peer projection as status. Registration is
idempotent by instance and registration name until removal.
`unic device channel-add INSTANCE NAME` uses this route.
`unic device channel pause|resume|remove ID REVISION` uses the existing
revision-checked update and removal routes. Channel IDs carry no device trust.
