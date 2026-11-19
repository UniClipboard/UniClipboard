# Traces implementation plan

- State: Planned. The current service exports logs, metrics and product events.
- Dependencies: [observability architecture](../architecture/observability.md) and
  [observability contract](../contracts/observability-contract.md).

## First release slice

Use the OpenTelemetry Go SDK already used by the service. Add a service-owned
TracerProvider and the matching OTLP/HTTP trace exporter. Use the existing
configured endpoint and exact credential binding. Add an independent
`telemetry.traces.enabled` switch with default `false`. Prove the first slice
against the local TLS receiver before selecting a hosted ingestion target.

Start a root span for a synthetic manual submission. Attach a commit child span
using context passed within the process. Use fixed names and the existing finite
stage and result vocabulary. Include no clipboard content, paths, device IDs,
request IDs, raw errors, baggage or downstream socket metadata. Do not propagate
trace context over device or plugin protocols in this slice.

Use one bounded in-memory batch queue and a parent-based sampler. Start with an
explicit low root sampling rate for enabled profiles. Select the actual rate and
queue capacity after measuring the synthetic baseline. Run trace export off the
business path. On disable, stop admitting new spans and discard pending spans.
Test the in-flight cancellation barrier and the shared service shutdown budget
before exposing the switch.

## Process acceptance

Add the real-process scenarios before implementation.

| Scenario | Required result |
|---|---|
| Default, master disabled, or trace switch disabled | Zero trace requests across a complete export window. Existing signals retain their configured state. |
| Enabled synthetic submission at a test-only 100% sample rate | One trace and one commit child. Repeating the same idempotency key produces no second commit span. |
| Sampling below 100% | Parent and child make the same sampling decision. Record offered and exported root counts. |
| Export failure, slow receiver or saturated queue | Business submission succeeds. Queue memory stays bounded, drops are visible locally, and no export recurses. |
| Disable and re-enable | In-flight export cancels. Re-enabled generation exports only newly admitted spans. |
| Privacy | Decode OTLP resource and span fields. Reject canaries in the body, attributes, status, events and links before writing artifacts. |
| Cost | Compare disabled, enabled-unsampled and sampled runs with the same workload. Record process CPU/RSS, submission latency, requests and bytes. |
| Hosted read-back | Query the selected project by the synthetic service version. Match the root/child relationship against local expectations and store only sanitized aggregates. |

Keep propagation as a separately reviewed change. It needs a wire contract,
explicit trust boundaries and tests for forged or oversized incoming context.
