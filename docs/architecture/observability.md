---
paths:
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
  - internal/localconfig
  - app/service/internal/host/node.go
  - internal/localapi/diagnostics.go
  - app/service/internal/server/diagnostics.go
  - app/service/cmd/unic/diagnostics.go
  - app/service/cmd/unicd
  - e2e/cli/observability_e2e_test.go
verified: 0710594f0e20d22f9e2ff06bece61631eff6172d
---
# Client logs and metrics

The desktop host owns optional OTLP/HTTP logs and metrics. Remote telemetry is disabled by default.
The implementation is committed. The verification metadata identifies the scoped feature commit; unrelated desktop UI work is excluded. End-to-end evidence and remaining acceptance limits are recorded in the execution receipt.

## Current behavior

- `host.Open` constructs diagnostics before `engine.New`. It injects a structured logger and a callback using public Engine types.
- Engine produces bounded categorical records at observation admission, processor completion, unique submission completion, send queue departure, send completion, receive completion, and apply ticket completion or invalidation.
- Idempotent submission waits do not create another commit metric. A send result does not prove receive or apply success.
- Local logs use an asynchronous 1024-record queue, JSON Lines, private files, 4 MiB segments, a 20 MiB combined bound, and seven-day retention. Filesystem operations use `os.Root` under the profile directory.
- Local and remote log levels are independent. The socket API and CLI support a per-target override lasting 1..3600 whole seconds. Overrides do not survive restart and do not enable uploads.
- Configuration reloads update levels and signal switches. The endpoint remains fixed for the process. Changes appear as `telemetry.endpoint` in restart-required state.
- Each remote signal has an independently cancelled generation. Disabling a signal closes admission, cancels the current export, discards queued data, and waits for cleanup. Re-enabling creates fresh aggregation and queues.
- The remote log queue is the only asynchronous log SDK queue. A custom processor uses public `sdk/log.Processor` methods, clones accepted records, and batches at most 128 records for the official HTTP exporter.
- Metrics use the official SDK and a ManualReader. The host worker controls collection and export every 60 seconds. Temporality is cumulative. Histograms have fixed second-based boundaries; cardinality is capped at 1024 sets per instrument, and exemplars are disabled.
- Export attempts have a three-second HTTP timeout and no exporter retries. Logs and metrics share one two-second shutdown flush budget.
- All log fields pass an enum allowlist before encoding. Unapproved LogValuer values are never resolved. The log bridge receives a clean background context.
- OTel v1.47 merges environment Resource attributes even with `WithResource`. Metrics replace Resource before export. The private logs HTTP transport replaces Resource using the standard OTLP protobuf before sending. Both keep only service name/version, OS type, and architecture.
- Export failures update local counters and finite error categories. Exporter degradation messages only enter the bounded local path and respect the local level. They are rate-limited and never feed the failing exporter.

## Invariants

- Business execution never performs diagnostics network or disk I/O and never waits for diagnostics queue space.
- Engine diagnostic callbacks execute outside Engine locks. Callback panic is isolated from the business result.
- No Provider, exporter, or internal type appears in public Engine Options.
- No device identity, address, pairing material, clipboard content, path, arbitrary plugin text, or raw error enters telemetry.
- Derived loggers share current level controls. A local threshold cannot filter a record accepted by the remote threshold.
- No new request starts after the disable barrier returns. Previously transmitted requests cannot be retracted.
- A metrics generation never replays work from a disabled period.
- Telemetry is lossy diagnostics, not durable delivery, billing, or audit history.

## Code map

| Path | Responsibility |
|---|---|
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

## Specifications

- [Observability contract](../contracts/observability-contract.md).
- [Engine contract](../contracts/engine-contract.md).
- [Acceptance plan and receipt](../exec/0014-client-observability-otlp.md).

## Gaps

- Traces, cross-device propagation, reliable offline upload, crash reports, and production Collector deployment are not implemented.
- Production multi-client cumulative-stream identity remains a receiver delivery prerequisite. Current telemetry Resource has no device or installation identifier.
- No production credential-store adapter is shipped. Controlled host injection is available; normal daemon assembly supplies no token.
- Nonvisual acceptance uses the real session/wire protocol over the host's local TCP test transport. It does not establish public DERP, native clipboard, true system sleep, or Windows/Linux native acceptance.
- The execution receipt records fault and resource evidence. It does not turn missing wakeup measurements or untested platform behavior into acceptance.
