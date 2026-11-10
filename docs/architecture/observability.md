---
paths:
  - engine/product_events.go
  - engine/devices.go
  - engine/backend.go
  - engine/history_sync.go
  - engine/restore.go
  - internal/clipboardintegration
  - tools/package
  - e2e/posthogcloud
  - e2e/cli/posthog_e2e_test.go
  - engine/diagnostics.go
  - engine/options.go
  - engine/engine.go
  - engine/state.go
  - engine/history.go
  - engine/submission.go
  - engine/pipeline.go
  - engine/peersend.go
  - engine/receive.go
  - engine/clipboard.go
  - internal/observability
  - internal/syncdiag
  - internal/session
  - internal/tcnet
  - internal/plugins/tailcat
  - internal/wire
  - internal/localconfig
  - app/service/internal/host/node.go
  - internal/localapi/diagnostics.go
  - app/service/internal/server/diagnostics.go
  - app/service/cmd/unic/cli.go
  - app/service/cmd/unic/main.go
  - app/service/cmd/unic/logs_export.go
  - e2e/cli/logs_export_e2e_test.go
  - app/service/cmd/unic/diagnostics.go
  - app/service/cmd/unicd
  - e2e/cli/observability_e2e_test.go
  - e2e/cli/sync_observability_e2e_test.go
  - e2e/syncpath
verified: dceb702ad0930a396ee01466f3dd1da49631dba9
---
# Client logs, metrics and product analytics

The UI-free service owns OTLP/HTTP logs and metrics plus PostHog product events. All remote switches default enabled; `config.toml` can disable the master switch or any individual signal. Missing release deployment metadata degrades uploads without preventing service startup.
Current workspace implementation and acceptance scope are recorded in the [PostHog execution receipt](../exec/0019-posthog-integration.md). Local process evidence and Cloud read-back are recorded separately. The selected Cloud project is US Cloud / UniClipboard (722858).

## Current behavior

- `host.Open` constructs diagnostics before `engine.New`. It injects a structured logger and a callback using public Engine types.
- Engine produces bounded categorical records at observation admission, processor completion, unique submission completion, send queue departure, send completion, receive completion, and apply ticket completion or invalidation.
- Receive admission records malformed part sizes or mismatched totals as categorical `rejected` / `invalid_argument` outcomes before reserving resources. External channels supply their actual part byte lengths. No rejected bytes, source identity or raw error text enters the record.
- Idempotent submission waits do not create another commit metric. A send result does not prove receive or apply success.
- Local logs use an asynchronous 1024-record queue, JSON Lines, private files, 4 MiB segments, a 20 MiB combined bound, and seven-day retention. Filesystem operations use `os.Root` under the profile directory.
- Local and remote log levels are independent. The socket API and CLI support a per-target override lasting 1..3600 whole seconds. Overrides do not survive restart and do not enable uploads.
- Configuration reloads update levels and signal switches. The endpoint remains fixed for the process. Changes appear as `telemetry.endpoint` in restart-required state.
- Each remote signal has an independently cancelled generation. Disabling a signal closes admission, cancels the current export, discards queued data, and waits for cleanup. Re-enabling creates fresh aggregation and queues.
- The remote log queue is the only asynchronous log SDK queue. A custom processor uses public `sdk/log.Processor` methods, clones accepted records, and batches at most 128 records for the official HTTP exporter.
- Metrics use the official SDK and a ManualReader. The host worker controls collection and export every 60 seconds. PostHog `/i` endpoints use delta temporality; pathless Collector endpoints retain cumulative temporality. Histograms have fixed second-based boundaries; cardinality is capped at 1024 sets per instrument, and exemplars are disabled.
- Export attempts have a three-second HTTP timeout and no exporter retries. Logs, metrics and analytics share one two-second shutdown flush budget.
- All log fields pass an enum allowlist before encoding. Unapproved LogValuer values are never resolved. The log bridge receives a clean background context.
- OTel v1.47 merges environment Resource attributes even with `WithResource`. Metrics replace Resource before export. The private logs HTTP transport replaces Resource using the standard OTLP protobuf before sending. Both keep only service name/version, OS type, and architecture.
- Export failures update local counters and finite error categories. Exporter degradation messages only enter the bounded local path and respect the local level. They are rate-limited and never feed the failing exporter.

## Offline log archive

`unic dev logs export [--output FILE]` streams retained local JSONL segments into a
private tar.gz with a per-entry SHA-256 manifest. It executes in the CLI and needs
no service connection or system clipboard. It never includes profile configuration,
credentials, databases or history. Rooted source/output handles and exclusive
creation reject symlinks and existing destinations. Fixed per-file lengths bound
concurrent appends; vanished unopened segments can be skipped during retention.
This is an on-disk snapshot, not a queue flush or an atomic cross-file snapshot.
See section 12 of the [observability contract](../contracts/observability-contract.md#12-offline-local-log-export).

## Product analytics

The host injects `Options.ProductEvents` alongside Logger and Diagnostics.
Engine emits pairing trust commits, unique Action completions, manual send terminal results and successful explicit history application.
Events are emitted outside the Engine state mutex (`e.mu`); lifecycle and commit guards may still be held. Callbacks must not call Engine methods or wait for lifecycle work. Events do not rely on lossy GUI notifications.
Engine also emits accepted remote receipt application and stored-origin history classification. The service coalesces remote receipt notifications into a persisted identity/UTC-day availability event. Per-operation automatic volume remains in metrics. See section 11 of the observability contract.
The service emits startup events. Desktop registration carries a bounded local launch key until the first ready acknowledgement; later reconnects omit it. The service remembers the last ready launch key to suppress duplicate ready calls and uncertain-result retries. The key never enters telemetry.
The service is the sole exporter; Desktop does not receive the analytics identifier.

`analytics.go` uses the official PostHog Go SDK with one aggregate 1024-event admission bound, 64-event batches, five-second batching, no retries and a three-second HTTP timeout.
A final transport filter removes SDK-added system properties. The transport bounds response reads and rejects destinations other than the selected origin and `/batch/`.
Person processing and GeoIP are disabled. A separate random profile identifier supports installation retention without using device identity.
The identifier persists while enabled, rotates after disable, and is never attached to logs or metrics.
An invalidation marker prevents reuse after an interrupted identity cleanup where it can be persisted.

The master and three signal switches hot reload. Explicit false takes effect before startup initialization.
Endpoint changes require restart; changing the endpoint never forwards the compiled project token to the new destination.
`tools/package` accepts validated build-time deployment metadata. No runtime `POSTHOG_*` or `OTEL_*` override grants upload or changes the project.
The exact settings and public hook are defined in the [observability contract](../contracts/observability-contract.md).

## Invariants

- Business execution never performs diagnostics network or disk I/O and never waits for diagnostics queue space.
- Engine diagnostic and product callbacks execute outside the Engine state mutex. Lifecycle/commit guards may remain held, so callbacks cannot call Engine methods or wait for lifecycle work. Callback panic is isolated from business results.
- No Provider, exporter, or internal type appears in public Engine Options.
- No device identity, address, pairing material, clipboard content, path, arbitrary plugin text, or raw error enters telemetry.
- Derived loggers share current level controls. A local threshold cannot filter a record accepted by the remote threshold.
- No new request starts after the disable barrier returns. Previously transmitted requests cannot be retracted.
- A metrics generation never replays work from a disabled period.
- Telemetry is lossy diagnostics, not durable delivery, billing, or audit history.

## Code map

| Path | Responsibility |
|---|---|
| `engine/product_events.go` | Finite public product-event hook and callback panic isolation |
| `internal/observability/analytics_daily.go` | Worker-owned private daily admission marker |
| `internal/syncdiag`, `internal/session/observability.go`, `internal/tcnet/path.go`, `internal/observability/sync.go` | Finite built-in records, session/recovery ownership, passive reduction and OTLP instruments |
| `internal/observability/analytics.go`, `deployment.go` | SDK lifecycle, private identity, final event filtering and release endpoint/token binding |
| `e2e/cli/posthog_e2e_test.go` | Local TLS, delta aggregation, all five events, privacy, faults and queue/generation acceptance |
| `engine/diagnostics.go` | Public record types, categorical content/backend mapping, callback isolation, apply termination |
| `engine/submission.go`, `pipeline.go`, `peersend.go`, `receive.go`, `clipboard.go` | Business execution points and measurements |
| `internal/observability/config.go`, `controller.go` | Validated settings, snapshots, level overrides, lifecycle generations |
| `internal/observability/logs.go` | Safe slog handler, async local files, retention |
| `internal/observability/metrics.go` | Instrument catalog, validation, histogram and health counters |
| `internal/observability/export.go`, `privacy.go` | Official SDK/exporter assembly, bounded workers, cancellation, final Resource filtering |
| `app/service/internal/server/diagnostics.go` | Socket routes and strict flat JSON validation |
| `internal/localapi/diagnostics.go` | Shared diagnostics client methods |
| `app/service/cmd/unic/diagnostics.go` | Level and telemetry CLI commands |
| `app/service/cmd/unicd/*_e2e.go` | Build-tagged nonvisual test entry, fixed fixtures, local TCP transport seam |
| `e2e/cli/observability_e2e_test.go` | Real-process HTTP/protobuf, privacy, lifecycle, fault and resource acceptance |

## Tailcat sync observations

The built-in plugin receives a finite `syncdiag.Observer` at service assembly.
Connection attempts are recorded before RPC error reduction. Admitted sessions share
one close claim for business cleanup and diagnostic termination. The session layer
tracks observed connected exposure and one recovery episode per eligible disconnected
peer. Pending ages accompany recovered, abandoned and interrupted terminal counts.
Bookkeeping uses existing peer/session ownership and does not survive process restart.

The existing metrics worker samples active sessions and pending recovery every 10
seconds. It stops its sampling timer when no work remains. Disable tears down that
worker and clears baselines. The collector does not dial or call Ping/DiscoPing.
The transport reads status once per collection and returns only a private reduced lookup that yields finite path categories.
Incoming mapping uses the pinned tailcat ULA/node-key representation and rejects
ambiguous matches. Active status must have a LastWrite within 30 seconds. A valid
selected direct endpoint wins over a configured relay. Raw status never enters queues.
The public Client status API is unavailable in tailcat v0.7.0, so outgoing paths remain
unknown. Real-stack path and shutdown acceptance must be reported separately from
loopback/fake-path export acceptance in the [0020 receipt](../exec/0020-tailcat-sync-observability.md).

Authorized/admitted declared size and actual partial body bytes are separate measures.
Body timing excludes queue, control headers and storage work after the last body I/O.
The start-path label uses a cached sample no older than 10 seconds. It cannot prove
the complete route or exact per-path traffic. Sender completion is not receiver ACK.
Successful receipt application proves host-reported clipboard availability, not paste.

The analytics worker persists a private identity/day high-water marker before daily
SDK enqueue. One reserved temporary filename bounds crash residue; startup and disable clear it. Marker I/O occurs outside business callbacks. A failed marker suppresses
the daily event while sync and operational volume continue. Persist-before-enqueue
can lose an event; it never backfills failed export. Clock rollback suppresses dates
through the stored high-water date. Disable removes the marker with the identity.

## Cloud verification

`e2e/posthogcloud` builds two production services without test tags and drives ten synthetic submissions through the public CLI.
It uses isolated profiles with sync disabled, the normal 60-second metrics period, and explicit deployment metadata.
The Cloud query receipt confirms two anonymous startup events, ten manual submission outcomes, two sanitized warning logs, ten committed operations and corresponding histogram counts.
No person profiles were created. This acceptance is scoped to the recorded project and time window; it is not an installation or release publication.
The driver emits source/binary hashes and sanitized local state. Cloud query results are stored separately so local readiness cannot masquerade as ingestion success.

## Specifications

- [Observability contract](../contracts/observability-contract.md).
- [Engine contract](../contracts/engine-contract.md).
- [Acceptance plan and receipt](../exec/0014-client-observability-otlp.md).

## Gaps

- Traces, cross-device propagation, reliable offline upload, crash reports, and production Collector deployment are not implemented.
- PostHog delta counts and histogram sums are verified locally across processes. US Cloud read-back also confirms a combined commit delta of 10 and histogram sample counts of 3 and 7. A pathless cumulative Collector still needs isolated stream identity before multi-client deployment.
- Release assembly supports an ingestion-only project token bound to its exact endpoint. US Cloud / UniClipboard (722858) is selected and verified using isolated production builds; the ingestion token remains in private local build configuration, outside source and receipts. No production credential-store UI is shipped.
- Nonvisual acceptance uses the real session/wire protocol over the host's local TCP test transport. It does not establish public DERP, native clipboard, true system sleep, or Windows/Linux native acceptance.
- The execution receipt records fault and resource evidence. It does not turn missing wakeup measurements or untested platform behavior into acceptance.
