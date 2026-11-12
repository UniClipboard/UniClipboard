# Local service and desktop adapter contract

## Scope

`unic` and `unic-desktop` communicate with the single `unicd` that owns a private state directory.
The protocol uses HTTP over the existing owner-only Unix socket, including AF_UNIX on supported Windows hosts.
There is no TCP control listener. Socket access grants same-user control; an adapter session is a concurrency mechanism.
Protocol version remains 1.

## Headless operations

| Method and path | Request | Result |
|---|---|---|
| `POST /v1/submissions` | `{request_id,snapshot,device_ids?}` | Engine `SubmissionResult` |
| `GET /v1/submissions/{request_id}` | None | Current Engine submission result |
| `GET /v1/capabilities` | None | Engine `Capabilities` |
| `GET /v1/received` | None | Engine `ReceivedContent[]` |
| `GET /v1/received/{receipt_id}/content?part=N` | Zero-based part, default 0 | Content stream |
| `DELETE /v1/received/{receipt_id}` | None | Dismiss transient record |
| `POST /v1/history/{id}/actions/{key}` | `{request_id,apply}` | `{item,submission,applied,apply_error?}` |

A snapshot has `Parts` with `Format` and base64 `Data`, and `Files` with absolute `Path`, `Name` and `Size`.
File paths refer to same-user local files. Engine imports stable references, checks changes on reads, and owns no source file.
A request body is limited to 96 MiB. Representation and file limits also follow the Engine content contract.
No operation interprets newest history as the current system clipboard.

The caller supplies a nonempty submission `request_id` of at most 128 bytes.
The service admits at most 1024 explicit request records and expires completed records after 30 minutes.
The service compares the submitted snapshot digest before importing it again.
A retry with an identical snapshot returns the first result. A different snapshot conflicts. The host cache does not hash `device_ids`; changing only that field returns the original result without reaching Engine selection validation. This is a known gap relative to Engine request idempotency.
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

Clipboard reads, explicit copy, history application and restore execute in the desktop process.
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
A normal desktop exit stops capture/application and its owned service.
A standalone lightweight service continues network reception and explicit headless submissions without a desktop.

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
File parts retain their declared names; representations use `part-N.txt`, `.html`, `.png`, or `.bin`.
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
- `unicd --desktop-owned` reserves stdin for an anonymous parent-lifetime pipe. EOF or a read failure cancels the service, including after abrupt parent death. CLI commands borrow it and never stop it.
- `unicd --cli-temporary` reserves stdin for the launcher's initial ownership pipe and lives while at least one lease exists.
  Any other argument list exits with code 2.

Normal desktop launch restarts any existing service, including a temporary one, and creates a desktop generation.
Lightweight launch starts or reuses a standalone generation. It refuses a desktop-owned generation and refuses a temporary generation with a clear error. It never adopts a temporary generation.
A direct `unicd` start meets the exclusive profile lock. It does not take over active temporary clients.

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

### Quick-panel host preference

The shared configuration adds `desktop.quick_panel_shortcut`. It defaults to Command+Ctrl+V on macOS and Ctrl+Alt+V on Windows/Linux. Accepted values are `Ctrl+` followed by a backtick, `CmdOrCtrl+Shift+Space`, `Ctrl+Alt+V`, `Cmd+Ctrl+V`, and the empty string (disabled). The existing configuration revision check and TOML hot reload apply. The service persists the preference; only the graphical host registers it. Registration failure does not revert the saved configuration and is shown separately from the effective host registration. No target identity, paste status or OS input command enters service DTOs. See the [Desktop quick-panel contract](desktop-quick-panel-contract.md).

## Desktop language configuration

The existing configuration response and TOML save API include `desktop.language`.
Accepted values are `zh-CN`, `en`, `ja`, `pt-BR`, `ru`, and the empty string (Simplified Chinese).
Missing configuration uses `zh-CN`. Unknown nonempty codes reject the whole configuration and preserve the previous valid state.
The field is host-only and does not change Engine settings, protocol version, device identity or sync payloads.
Saving uses the existing revision check. Desktop consumes the saved state through normal refresh and does not request a restart for this field.
