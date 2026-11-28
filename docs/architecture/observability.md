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
  - engine/transfer.go
  - engine/receive.go
  - engine/clipboard.go
  - internal/observability
  - internal/procmetrics
  - internal/syncdiag
  - internal/session
  - internal/tcnet
  - internal/plugins/tailcat
  - internal/wire
  - internal/localconfig
  - app/service/internal/host/submissions.go
  - app/service/internal/host/node.go
  - app/service/internal/host/resource.go
  - internal/localapi/diagnostics.go
  - app/service/internal/server/diagnostics.go
  - app/desktop/internal/resourcereport
  - app/desktop/cmd/unic-desktop/roles.go
  - app/desktop/cmd/unic-desktop/roles_native.go
  - app/desktop/cmd/unic-desktop/gui.go
  - app/service/cmd/unic/cli.go
  - app/service/cmd/unic/main.go
  - app/service/cmd/unic/logs_export.go
  - app/desktop/cmd/unic-desktop/diagnostics_actions.go
  - internal/logarchive
  - e2e/cli/logs_export_e2e_test.go
  - app/service/cmd/unic/requirements.go
  - app/service/cmd/unic/session.go
  - app/service/cmd/unic/sendwait.go
  - app/service/cmd/unic/diagnostics.go
  - app/service/cmd/unicd
  - e2e/cli/traces_e2e_test.go
  - e2e/tracecloud
  - e2e/cli/observability_e2e_test.go
  - e2e/cli/resource_observability_e2e_test.go
  - e2e/cli/sync_observability_e2e_test.go
  - e2e/syncpath
  - bindings/mobile/diagnostics.go
  - bindings/mobile/engine.go
  - bindings/mobile/resource.go
  - bindings/e2e/mobile-diagnostics
  - bindings/scripts/build-mobile.sh
  - app/expo/modules/unic-engine/ios/ResourcePowerCollector.swift
  - app/expo/modules/unic-engine/ios/MetricKitSubscriber.swift
  - app/expo/targets/keyboard/KeyboardResourceCollector.swift
  - app/expo/modules/unic-engine/android/src/main/java/app/uniclipboard/engine/AndroidResourcePowerCollector.kt
  - app/expo/src/screens/settings/DiagnosticsSettings.tsx
  - app/expo/src/screens/settings/useDiagnosticCapture.ts
  - app/expo/src/screens/settings/ios/DiagnosticsPage.tsx
  - app/expo/src/screens/settings/android/DiagnosticsSection.tsx
verified: 3585ab6cae3d3f5ba80b18b870efdf8e038f67ab
---
# Client logs, metrics, traces and product analytics

The UI-free service owns OTLP/HTTP logs, metrics and sampled traces plus PostHog product events. All four signals default enabled; `config.toml` can disable the master switch or any individual signal. Missing release deployment metadata degrades uploads without preventing service startup.
Current workspace implementation and acceptance scope are recorded in the [PostHog execution receipt](../exec/0019-posthog-integration.md). Local process evidence and Cloud read-back are recorded separately. The selected Cloud project is US Cloud / UniClipboard (722858).

## Current behavior

The in-process mobile binding instantiates the same controller per Engine with
`service.name=unic-mobile`.
It subscribes to categorical Engine diagnostics, product events, the
`engine.Options.Tracer` hook, and the built-in sync collector. On successful
`Start`, the main mobile Engine calls `StartProduct("mobile")` (emitting
`app_started` and stamping `product=mobile` with `os=ios|android` on all mobile
product events), while short-lived app extensions (`extension_mode=true`) call
`EnableProduct("mobile")` so they reuse the shared App Group `analytics-id` and
daily marker without emitting `app_started` on each extension activation. The
Boolean `mobile-telemetry.json` in the private profile holds the upload
preference. An absent preference defaults to enabled. An unreadable preference
disables upload.
The native settings page supports a 10-minute temporary local `debug` log-level
override (`SetLocalLogLevelOverride`) and exports a redacted `.zip` diagnostic
bundle (`ExportDiagnosticArchive`) through the operating system share sheet (and
Android document save). The archive flushes the local log worker within a
bounded timeout, streams retained on-disk `logs/segment-*.jsonl` files under
`os.Root` size/symlink limits into `logs/engine/`, and writes `manifest.json`
with per-file SHA-256 digests plus safe app/runtime/telemetry/log-level state
(falling back to recent in-memory redacted records with explicit warnings if
disk logs are unavailable). Production binding builds
accept the same validated PostHog Cloud host and ingestion-only token as service
builds. Missing deployment metadata leaves local diagnostics active. Mobile
emits the shared logs, metrics, product-event, and trace hooks (`unic.submission`,
`unic.observe`, `unic.action`, `unic.commit`, `unic.sync.send`,
`unic.sync.receive`, and `unic.apply`).

- `host.Open` constructs diagnostics before `engine.New`. It injects a structured logger and a callback using public Engine types.
- Engine produces bounded categorical records at observation admission, processor completion, unique submission completion, send queue departure, send completion, receive completion, and apply ticket completion or invalidation.
- Receive admission records malformed part sizes, over-limit headers, or mismatched totals as categorical `rejected` outcomes (`invalid_argument`, `limit_exceeded`, `resource_exhausted`) before reserving resources. External channels supply their actual part byte lengths. No rejected bytes, source identity or raw error text enters the record.
- Receive failures/rejections and failed peer sends emit `WARN` `pipeline.failed` logs using only the allowlisted `stage`, `result`, `backend`, `content_kind`, and `code` attributes. Built-in peer-session body read/write idle timeouts are preserved as `timeout` across session close reasons, Engine diagnostics, and Operation target errors.
- Idempotent submission waits do not create another commit metric. A send result does not prove receive or apply success.
- Local logs use an asynchronous 1024-record queue, JSON Lines, private files, 4 MiB segments, a 20 MiB combined bound, and seven-day retention. Filesystem operations use `os.Root` under the profile directory.
- Local and remote log levels are independent. The socket API and CLI support a per-target override lasting 1..3600 whole seconds. Overrides do not survive restart and do not enable uploads. The CLI command is `unic dev logs level`. When the service is a CLI-temporary `unicd` (`--cli-temporary`), setting an override holds the CLI usage lease until expiry, cancellation, reset, replacement or a failed level query. Query and reset return after their response. `unic dev diagnostics telemetry` reads signal state and dropped-record counters.
- Configuration reloads update levels and signal switches. The endpoint remains fixed for the process. Changes appear as `telemetry.endpoint` in restart-required state.
- Each remote signal has an independently cancelled generation. Disabling a signal closes admission, cancels the current export, discards queued data, and waits for cleanup. Re-enabling creates fresh aggregation and queues.
- The remote log queue is the only asynchronous log SDK queue. A custom processor uses public `sdk/log.Processor` methods, clones accepted records, and batches at most 128 records for the official HTTP exporter.
- Metrics use the official SDK and a ManualReader. The host worker controls collection and export every 60 seconds. PostHog `/i` endpoints use delta temporality; pathless Collector endpoints retain cumulative temporality. Histograms have fixed second-based boundaries; cardinality is capped at 1024 sets per instrument, and exemplars are disabled.
- Export attempts have a three-second HTTP timeout and no exporter retries. Logs, metrics, traces and analytics share one two-second shutdown flush budget.
- All log fields pass an enum allowlist before encoding. Unapproved LogValuer values are never resolved. The log bridge receives a clean background context.
- OTel v1.47 merges environment Resource attributes even with `WithResource`. Metrics replace Resource before export. The private logs HTTP transport replaces Resource using the standard OTLP protobuf before sending. Both keep only service name/version, OS type, and architecture.
- Export failures update local counters and finite error categories. Exporter degradation messages only enter the bounded local path and respect the local level. They are rate-limited and never feed the failing exporter.

## Distributed, pipeline and mobile traces

Both the service (`service.name=unicd`) and the in-process mobile binding (`service.name=unic-mobile`) inject their `observability.Controller` into `engine.Options.Tracer`, a pure standard-library/Engine-type hook with zero OTel dependencies in `engine/`.

- **Roots and local spans**:
  - `unic.submission`: started by `host.commitExplicit` or mobile `CommitContent` / `CommitSharedItem` / `ResendHistoryItem` after unique admission. Import failures produce only a failed root; idempotent retries produce no extra spans.
  - `unic.observe`: started inside Engine `runSubmission` only when `ObserveClipboard` admits a real clipboard change (`baseline`, `unchanged`, `empty`, `echo` and duplicate sequence retries create no span).
  - `unic.action`: started inside Engine `runSubmission` for each unique admitted `RunAction` execution with `action_category=builtin|external`.
  - `unic.commit`: child of `unic.submission`, `unic.observe` or `unic.action` (or local root when invoked directly), measuring processing, persistence and dispatch; ended before `entry.done` closes so child timing stays contained in its parent.
  - `unic.sync.send` (`client`): child of `unic.commit` per target send execution, tagged with `backend` and `content_kind`.
  - `unic.sync.receive` (`server`): created at `admitReceive` (or early `OpenReceive` limit rejection), parented to the validated remote W3C `traceparent` when present.
  - `unic.apply`: child of `unic.sync.receive` for received `BeginApply` tickets (when the receive was sampled), or independently sampled root for `BeginApplyHistory`, ended on `ReportApplyResult` or ticket invalidation (`cancelled`).
- **Cross-device propagation**: When `unic.sync.send` is sampled, Engine writes W3C Trace Context v00 `traceparent` (`00-<32hex>-<16hex>-01`) into optional `rpc.Clip.TraceParent` and built-in `wire.Clip.TraceParent` (wire version remains 1). Unsampled sends (99% in production) omit the field. The built-in session layer sanitizes inbound `wire.Clip.TraceParent` before RPC admission; malformed peer headers are silently discarded as empty and never reject valid clips. `tracestate` and `baggage` are never propagated or accepted.

Each host owns an OTel v1.47 TracerProvider and matching official HTTP trace
exporter. It uses `ParentBased(TraceIDRatioBased(0.01))`, one 256-span queue and
at most 128 spans in an export batch. The worker exports every five seconds or
at batch capacity. It never waits for queue space. Overload can drop individual
spans and leave incomplete trees; these are lossy diagnostics. The explicit
sampler overrides used in synthetic builds are unavailable through TOML or
runtime environment. The [trace receipt](../exec/observability-traces.md) records
matched cost measurements and the sampling/capacity rationale.

Local host roots ignore caller OTel context, baggage and socket metadata. A private
context key carries only the local generation and SpanContext across Engine
boundaries. Span names are fixed to the seven names above. Allowed attributes
are strictly `result`, optional Engine `code`, `backend`, `content_kind`, and
`action_category` (SpanLimits: 5 attributes, 64 bytes max). Spans contain no
events, links or status descriptions. The final protobuf transport replaces
Resource and removes Scope attributes, including any SDK-added environment
values.

A trace generation follows the same disable barrier as logs and metrics. Disable
detaches admission, cancels in-flight HTTP, discards pending spans and waits for
worker/SDK/transport cleanup. A child (including a pending receipt's retained
trace context) cannot attach to a re-enabled generation. Local process totals
expose drops and finite export failures in `traces` state; health metrics
include `signal=traces` when metrics are enabled.

## Offline log archive

Desktop diagnostic export and `unic dev logs export [--output FILE]` call the
`internal/logarchive` Go package directly. It streams retained local JSONL segments into a
private `.zip` diagnostic bundle (`logs/engine/segment-*.jsonl` plus `manifest.json`
with build/platform identity, per-entry SHA-256 digests and `engine_logs` counts).
The CLI command works before initialization and while `unicd` is stopped. The
shared exporter never starts a temporary service or accesses the system clipboard.
When an existing `unicd` socket is reachable, it performs a bounded best-effort read-only query to attach safe `runtime`
counts/capabilities and `diagnostics` (`telemetry`, `log_levels`) summaries; otherwise
`service_snapshot` is marked `offline`. It never includes profile configuration,
credentials, databases or history. Rooted source/output handles and exclusive
creation reject symlinks and existing destinations. Fixed per-file lengths bound
concurrent appends; vanished unopened segments can be skipped during retention.
This is an on-disk snapshot, not a queue flush or an atomic cross-file snapshot.
See section 12 of the [observability contract](../contracts/observability-contract.md#12-offline-local-log-export).

## Product analytics

The host injects `Options.ProductEvents` alongside Logger and Diagnostics.
Engine emits pairing trust commits, unique Action completions, manual send terminal results and successful explicit history application.
Events are emitted outside the Engine state mutex (`e.mu`); lifecycle and commit guards may still be held. Callbacks must not call Engine methods or wait for lifecycle work. Events do not rely on lossy GUI notifications.
Engine also emits successful device-receipt application, stored-origin history classification, and `daily_active` on committed real local clipboard observations (excluding baseline, unchanged, empty and echo). The remote-application event requires a nonempty peer ID and no channel ID; temporary-transfer and external-channel receipts do not qualify. The service and mobile binding coalesce remote receipt notifications (`analytics-remote-day`) and genuine usage (`analytics-active-day`, triggered by committed local observations or any non-startup product event, never by idle `app_started`) into persisted identity/UTC-day events (at most one per identity per UTC day). Per-operation automatic volume remains in metrics. See sections 10 and 11 of the observability contract.
The service emits startup events. Desktop registration carries a bounded local launch key until the first ready acknowledgement; later reconnects omit it. The service remembers the last ready launch key to suppress duplicate ready calls and uncertain-result retries. The key never enters telemetry.
The service is the sole exporter; Desktop does not receive the analytics identifier.

`analytics.go` uses the official PostHog Go SDK with one aggregate 1024-event admission bound, 64-event batches, five-second batching, no retries and a three-second HTTP timeout.
A final transport filter removes SDK-added system properties. The transport bounds response reads and rejects destinations other than the selected origin and `/batch/`.
Person processing and GeoIP are disabled. A separate random profile identifier supports installation retention without using device identity.
The identifier persists while enabled, rotates after disable, and is never attached to logs or metrics. A CLI-temporary service start counts as a process start (`app_started`), not an installation; the identifier persists across temporary restarts. Disabling the telemetry master or analytics switch suppresses product events and analytics-identifier creation.
An invalidation marker prevents reuse after an interrupted identity cleanup where it can be persisted.

The master and four signal switches hot reload. Explicit false takes effect before startup initialization.
Endpoint changes require restart; changing the endpoint never forwards the compiled project token to the new destination.
Versioned CLI packaging reads the product version from `release/products.toml` and preserves the validated deployment metadata. `--version` exits before telemetry initialization.
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
| `e2e/cli/posthog_e2e_test.go` | Local TLS, delta aggregation, product events, privacy, faults and queue/generation acceptance |
| `engine/diagnostics.go` | Public record types, categorical content/backend mapping, callback isolation, apply termination |
| `engine/submission.go`, `pipeline.go`, `peersend.go`, `receive.go`, `clipboard.go` | Business execution points and measurements |
| `internal/observability/config.go`, `controller.go` | Validated settings, snapshots, level overrides, lifecycle generations |
| `internal/observability/logs.go` | Safe slog handler, async local files, retention |
| `internal/observability/metrics.go` | Instrument catalog, validation, histogram and health counters |
| `internal/observability/export.go`, `privacy.go` | Official SDK/exporter assembly, bounded workers, cancellation, final Resource filtering |
| `app/service/internal/server/diagnostics.go` | Socket routes and strict flat JSON validation |
| `internal/localapi/diagnostics.go` | Shared diagnostics client methods |
| `app/service/cmd/unic/diagnostics.go`, `requirements.go`, `session.go`, `sendwait.go` | Level/telemetry commands, service acquisition and temporary override lifetime |
| `app/service/cmd/unic/logs_export.go` | Offline rooted archive and SHA-256 manifest |
| `app/service/cmd/unicd/*_e2e.go` | Build-tagged nonvisual test entry, fixed fixtures, local TCP transport seam |
| `e2e/cli/observability_e2e_test.go` | Real-process HTTP/protobuf, privacy, lifecycle, fault and resource acceptance |
| `e2e/cli/resource_observability_e2e_test.go` | Resource and power telemetry OTLP export, validation, disable barrier and cli-temporary exclusion |
| `internal/procmetrics` | CGO-free cross-platform CPU, OS memory footprint, Go heap, sleep-jump and macOS wakeups / Linux context switches probes |
| `internal/observability/resource.go` | Resource, mobile power, MetricKit validation and OTLP instruments (§14) |
| `app/desktop/internal/resourcereport` | Desktop `tray`, `gui` and `quick_panel` 60s and exit sample reporters over `/v1/diagnostics/resource-sample` |
| `bindings/mobile/resource.go` | Mobile `HostResourceProvider` bridge, MetricKit ingestion and pure-Go fallback collector |

## Host resource and mobile power telemetry

- `unicd` (excluding `--cli-temporary`) samples itself once per 60-second metrics export cycle via `internal/procmetrics` (`CGO_ENABLED=0`) and remains the sole desktop OTLP exporter (`service.name=unicd`).
- Desktop `tray`, `gui` and `quick_panel` processes run `app/desktop/internal/resourcereport`, submitting bounded (<= 1 KiB) samples to `POST /v1/diagnostics/resource-sample` while `telemetry.enabled && telemetry.metrics.enabled` is true. UI visibility callbacks capture monotonic transition timestamps non-blockingly without running probe I/O or waiting on probe/IPC locks; single-transition spill within threshold attributes cleanly to the prior state, whereas multi-switch, queue-overflow, or delayed cumulative counter windows are silently discarded and re-anchored (Decision C1), and disabling cancels in-flight IPC and resets baselines immediately.
- Mobile (`unic-mobile`) collects native OS CPU, memory (`phys_footprint` on iOS, `Debug.MemoryInfo.totalPss` on Android), Android aggregate UID `TrafficStats` bytes, Android piecewise timestamped background mode/screen/multicast-lock/suspended residency (`power_samples` with standby `background_screen_off` deep-sleep tolerance and `logcatProcess`-gated `overlay_event`), system thermal residency, Android-only strict >=10-minute unplugged system battery drain segments (per-value broadcast monitoring with 60s tick retry when transiently unregistered; iOS marks `system_battery.drain_rate` `unsupported` while keeping `power_source` and CPU/Memory/Thermal per Decision A1), and iOS main-app `MXMetricManager` 24-hour payloads (`unic.power.ios_metrickit.*`) with entry-bound consent generation, SHA-256 content-identity deduplication, and atomic persist-before-commit / seen-before-prune delivery up to Go ingestion admission (Decision B1).
- Hardware interrupt wakeups (`unic.power.wakeups`, macOS `proc_pid_rusage`) and voluntary context switches (`unic.power.context_switches`, Linux/Android `/proc/self/status`) are kept strictly separate. Missing or unsupported probes increment `unic.resource.probe_status` instead of zero-filling.

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
The [Client status dependency record](../exec/0020-tailcat-client-status.md) defines
the conditions for accepting outgoing path classification.

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
The [C acceptance receipt](../exec/observability-c-acceptance.md) also records
read-back of the two production services' sync bytes, size histograms, and
passive path samples over a forced local DERP.

## Specifications

- [Observability contract](../contracts/observability-contract.md).
- [Engine contract](../contracts/engine-contract.md).
- [Acceptance plan and receipt](../exec/0014-client-observability-otlp.md).

## Gaps

- Hosted PostHog Cloud trace upload/read-back (`observability-traces-hosted.md`), reliable offline upload, crash reports, and production Collector deployment remain open.
- PostHog delta counts and histogram sums are verified locally across processes. US Cloud read-back also confirms a combined commit delta of 10 and histogram sample counts of 3 and 7. A pathless cumulative Collector still needs isolated stream identity before multi-client deployment.
- Release assembly supports an ingestion-only project token bound to its exact endpoint. US Cloud / UniClipboard (722858) is selected and verified using isolated production builds; the ingestion token remains in private local build configuration, outside source and receipts. No production credential-store UI is shipped.
- Nonvisual acceptance uses the real session/wire protocol over the host's local TCP test transport. It does not establish public DERP, native clipboard, true system sleep, or Windows/Linux native acceptance.
- The C receipt records process-wide macOS wake counters and isolated collector timings at 0, 1 and 8 sessions. Its scope is the measured test configuration.
