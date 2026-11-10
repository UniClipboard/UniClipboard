# Local service and desktop adapter contract

## Scope

`unic` and `unic-desktop` communicate with the single `unicd` that owns a private state directory.
The protocol uses HTTP over the existing owner-only Unix socket, including AF_UNIX on supported Windows hosts.
There is no TCP control listener. Socket access grants same-user control; an adapter session is a concurrency mechanism.
Protocol version remains 1.

## Headless operations

| Method and path | Request | Result |
|---|---|---|
| `POST /v1/submissions` | `{request_id,snapshot}` | Engine `SubmissionResult` |
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
A retry with identical parameters returns the first result. Different parameters conflict.
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

`unic submit [text]` reads stdin when text is omitted. `-f` submits local file references; `--png PATH` imports PNG.
`unic submission REQUEST_ID` queries a result. `unic received list|show|dismiss` exposes transient receives.
`unic history action ID KEY [--apply] [--request-id KEY]` separates execution from application.
`unic copy`, `paste`, `history use`, and Action `--apply` require desktop abilities.
No adapter yields `unavailable`; unsupported capabilities yield `unsupported`; denied access yields `permission_denied`.
CLI success exits 0. Operation failures exit 1. Missing top-level commands exit 2.
Submission and Action JSON expose stable codes and independent commit/network/application results.

`unic history export ID --output DIR` executes in the CLI process.
It streams every part into an `os.Root` rooted at DIR, uses exclusive creation, and rejects unsafe names.
File parts retain their declared names; representations use `part-N.txt`, `.html`, `.png`, or `.bin`.
Duplicate names, existing files, symlinks at output file names, and path escape fail instead of overwriting.
A failed export removes only files created by that export.
Exported copies belong to the user and are independent of Engine cleanup.

## Service lifecycle

`GET /v1/service` returns `{id,pid,directory,desktop_owned}` on the owner-only local socket.
`id` is a random 128-bit generation identifier. `directory` is the service state-directory identity.
`POST /v1/service/stop` accepts `{id}` and rejects a mismatched generation with HTTP 409.
A successful response flushes `{ok:true}` before requesting service cancellation.
It acknowledges shutdown admission, not completion. Socket removal follows Engine closure and lock release.
The caller waits for socket removal before starting a replacement.
No lifecycle endpoint is exposed over TCP. Socket permission remains the same-user authority boundary.
A launcher does not stop a service whose reported directory differs from its selected profile.

`unicd --desktop-owned` reserves stdin for an anonymous parent-lifetime pipe.
EOF or a read failure cancels the service, including after abrupt parent death.
Plain `unicd` remains independent of stdin and ignores desktop startup settings.
Normal launch restarts the existing service and creates an owned generation.
Lightweight launch starts or reuses a standalone generation and exits its launcher.
It refuses to reuse an owned generation. Mode and initial visibility changes require a new desktop launch.

A directory-in-use startup failure exits `unicd` with code 75.
The desktop may retry only that code within its bounded startup deadline while an earlier service retires.
Other daemon startup failures exit 1. Invalid daemon arguments exit 2.


## Product analytics ownership

The service owns one PostHog analytics generation and random identifier per profile.
Registration may carry a 16-character alphanumeric `launch_id` for a pending Desktop process startup. The client omits it after the first successful ready acknowledgement, including later reconnects. The service keeps only the last ready launch key to suppress duplicate ready calls and retries after an uncertain acknowledgement. Registration without a launch key is valid and does not announce a new Desktop startup. The launch key is local-only and never enters telemetry.
The adapter never receives the analytics identifier or ingestion token.
Action, send, pairing and explicit history-application events come from authoritative Engine completions, not activity notifications.
`GET /v1/diagnostics/telemetry` includes analytics health and pending/loss counters, with no identity or credential values.
See the [observability contract](observability-contract.md#10-posthog-product-events).
