# 0019: PostHog Cloud integration

- Status: Upload defaults, ingestion-token distribution and analytics identity accepted; remaining integration choices under review. Runtime implementation is not authorized by this design task.
- Date: 2026-11-10.
- Scope: Integration design for logs, application metrics and product events.
- Deployment: PostHog Cloud; project and region remain undecided.
- Current implementation: [observability architecture](../../architecture/observability.md) and [observability contract](../../contracts/observability-contract.md).
- Execution plan: [0019 acceptance plan](../../exec/0019-posthog-integration.md).

## 1. Why this decision is needed

The existing host already collects bounded structured logs and pipeline metrics through OpenTelemetry.
PostHog should become a destination without duplicating this instrumentation.
Product analytics answers a separate question: which product capabilities people use and whether they return.
Pipeline stage counts alone cannot establish product activation or user retention.

This document proposes changes. It does not supersede the current contract until approved and implemented.
Native crash reporting, session replay, traces, feature flags and experiments require separate decisions.

## 2. Verified capability and source baseline

Source inspected: branch `v2`, commit `fd77245cb00fd9efb7a77688ba45e0f67d0d1d8e`.
Unrelated staged Desktop UI changes and existing release-design documents were present.

| Signal | Current code | PostHog path | Proposed integration |
|---|---|---|---|
| Logs | `internal/observability/logs.go`, `export.go` | `/i/v1/logs` | Reuse slog filtering, bounded processor and OTLP/HTTP exporter. |
| Metrics | `internal/observability/metrics.go`, `export.go` | `/i/v1/metrics` | Reuse existing instruments and official OTLP exporter. |
| Product events | No PostHog client | Official Go SDK capture/batching | Host-owned typed events with a strict property allowlist. |

PostHog documents OTLP ingestion for both signals using a project token in a Bearer header.
Use the selected region's ingestion host: `https://us.i.posthog.com` or `https://eu.i.posthog.com`.
Metrics is currently open alpha and requires project enablement. Recheck availability before implementation and hosted acceptance.
See [Go logs](https://posthog.com/docs/logs/installation/go) and [OTLP metrics](https://posthog.com/docs/metrics/installation/other).

The current endpoint validator permits only a pathless HTTPS base URL.
The exporters append `/v1/logs` and `/v1/metrics`, so pointing the existing setting at a Cloud host is insufficient.
Normal `unicd` assembly also supplies no credential reader.
The prototype telemetry switch does not establish working upload configuration or integration.

## 3. Recommended delivery path

```text
Engine typed diagnostics -> host observability -> OTel logs/metrics -> PostHog Cloud
Desktop/CLI user action -> host typed analytics -> PostHog Go SDK -> PostHog Cloud
```

Start with direct Cloud delivery. It satisfies the requested destination without operating another service.
All runtime assembly remains in the consuming host. Engine must not import PostHog or OTel SDK types.
Keep local logging independent of Cloud availability and upload settings.
Use the maintained [official Go SDK](https://posthog.com/docs/libraries/go) for product events.
Inspect and pin the chosen SDK release before implementation; do not implement a parallel capture protocol.

An owned gateway is an alternative when centralized quotas, project routing or shielding client IPs from PostHog becomes a requirement.
It adds an operated service, authentication, abuse controls and receiver acceptance.
It does not remove the need for correct metric temporality or client-side privacy filtering.
Do not create a generic vendor-routing framework for this integration.

## 4. Endpoint and credential decisions

Retain `telemetry.endpoint` as an HTTPS OTLP base URL, but allow a validated base path such as `/i`.
Append the signal paths explicitly and test exact resulting URLs.
Continue rejecting user information, query strings, fragments and redirects.
Preserve existing Collector endpoints without a base path.
The analytics host is a validated region host; never infer region from a default SDK endpoint.
Endpoint changes remain restart-required.

Use only the PostHog project ingestion token in the client.
Never distribute a personal API key, secret API key or query/admin credential.
Read/query credentials used by acceptance tools belong outside the application and its artifacts.
Do not place ingestion tokens in URLs, ordinary configuration output, logs or diagnostics.

Direct distribution requires a narrow amendment to the existing prohibition on shared tokens in public packages.
The project ingestion token is extractable from a distributed client; it cannot act as user authentication or abuse protection.
Accepted user decision (2026-11-10): allow an explicitly designated ingestion-only project token through release assembly and host injection.
Keep administrative credentials prohibited. Separate development and production projects.
No token is requested or installed during this design task.

## 5. Metrics semantics across many clients

The current exporter sends cumulative counters and histograms.
Its Resource contains only service name/version, OS and architecture.
Multiple matching clients therefore lack distinct stream identity at the receiver.
Changing only the URL could produce incorrect aggregation.

Recommend delta temporality for counters and histograms on the PostHog path.
Keep finite attributes and avoid persistent installation IDs on metric series.
PostHog documents native delta counters and histogram aggregation in its [metrics architecture](https://posthog.com/docs/metrics/architecture).
Actual multi-process counts, restart behavior and histogram merging remain acceptance requirements.
Do not claim these results from documentation alone.

Retain the existing five instruments:

| Instrument | Intended view |
|---|---|
| `unic.pipeline.operations` | Operation rates by stage/result/backend/content category. |
| `unic.pipeline.duration` | Stage latency distributions in seconds. |
| `unic.pipeline.errors` | Failure rates by stable code and stage. |
| `unic.observability.dropped` | Lost diagnostic records by signal/reason. |
| `unic.observability.export_failures` | Export failures by signal/error category. |

Keep loss visible. With retries disabled, failed delta exports lose that interval's data.
Do not treat these metrics as an audit ledger.
If delta compatibility fails acceptance, reconsider explicit stream identity and its privacy/cardinality cost before shipping.

## 6. Upload configuration and identity

User decision (2026-11-10): uploads are enabled by default and configurable in `$UNIC_HOME/config.toml`.
The master switch and the logs, metrics and analytics switches all default to true.
An explicit false value always wins over the default, including on first startup.
Load and validate the profile configuration before creating exporters or accepting remote records.
Changing log level does not change any upload switch.

The following is the proposed configuration after implementation. It is not supported by the current runtime in full:

```toml
[telemetry]
enabled = true
# endpoint is supplied by the selected release/project configuration

[telemetry.logs]
enabled = true
level = "warn"

[telemetry.metrics]
enabled = true

[telemetry.analytics]
enabled = true
```

Set `[telemetry] enabled = false` to stop all remote signals.
Set a signal's enabled field to false to stop only that signal.
Valid changes take effect through the existing config watcher without restarting the process.
Disabling waits for the cancellation/discard barrier; restarting must preserve the explicit false setting.
Invalid edits retain the last valid configuration and must never restore enabled defaults.
A missing release endpoint or ingestion token produces a safe unavailable/degraded state, never an inferred destination.
No first-run opt-in is required by this design.
The implementation must amend the current default-off contract and generated configuration template together.

Accepted user decision (2026-11-10): use one independent random analytics identifier per local profile, created only when analytics is effectively enabled.
It must not derive from device identity, public keys, host name, MAC address, account data or clipboard content.
Desktop and its service use the same profile identifier through private IPC.
Rotate it after analytics is disabled; do not reconnect the old identifier on re-enable.
Do not merge identities across devices. Retention represents installations/profiles, not people.

Set `$process_person_profile=false` and explicitly disable GeoIP enrichment.
Do not call identify, alias or person-property APIs.
These settings avoid person profiles; they do not make a stable identifier unlinkable.
Direct HTTPS delivery still exposes the network source address to the receiver.
See [anonymous events](https://posthog.com/docs/data/anonymous-vs-identified-events).

Keep logs and metrics free of analytics identifiers in the first version.
Correlate them by release/platform/time at aggregate level.
Per-installation log investigation requires a later explicit privacy decision.

## 7. Initial product event vocabulary

Use finite names and typed properties. Emit at accepted actions or authoritative completion points.
Never reconstruct completion from GUI notifications.

| Event | Definition | Owner |
|---|---|---|
| `app_started` | One successful product startup per process; distinguish Desktop and standalone service. | Product host |
| `pairing_completed` | One new trusted relationship successfully committed locally; not code entry or a reconnect. | Service completion boundary |
| `history_applied` | One successful user-requested history application; not automatic receive/apply. | Service application completion |
| `manual_send_completed` | One user-requested send operation reaches its terminal result; not one event per target. | Service submission completion |
| `action_completed` | One user-requested Action reaches its terminal result. | Service Action boundary |

Common allowlisted properties: `schema_version`, `product`, published `product_version`, OS and architecture.
Per-event properties may include finite `result`, `content_kind`, `backend`, `action_category` and stable failure `code`.
External action names map to a fixed category; do not send plugin/action identifiers.
Do not include exact sizes, search terms, history IDs, target counts, paths or arbitrary labels.
Recheck each available completion boundary before implementation; add only the smallest typed hook needed.

Desktop and daemon must not emit duplicate events for the same operation.
IPC retries and duplicate completion notifications must not create another product event.
An event UUID serves delivery deduplication; business IDs remain local.
Do not convert each capture, queue transition or sync packet into a product event.
Use metrics for that volume.

## 8. Lifecycle, resource and privacy invariants

Analytics uses one owned controller and a verified bounded SDK queue.
Proposed budget: at most 1024 pending events across all SDK queues, 2 KiB per encoded event, batches of at most 64, five-second batching, three-second request timeout and no automatic retries.
These are acceptance targets, not claims about SDK defaults.
Inventory SDK queue, batch and worker retention before pinning settings.
The SDK transport must refuse new requests after cancellation and cancel requests already in progress.
An SDK Close call that flushes is not the disable barrier.

All signals share the existing two-second exit budget.
Disabling analytics closes admission, invalidates the generation, cancels HTTP work, discards queues and waits for worker cleanup.
No requests may start after disable returns. Re-enable creates a new generation.
No event disk spool or replay of activity collected while analytics was disabled.
Report finite health/error states and loss counts without leaking SDK error strings or headers.
Keep analytics SDK diagnostics out of its own exporter.

Retain existing prohibitions on content, previews, hashes, file names/paths, device names/IDs, addresses, tickets and pairing material.
Apply a final allowlist after SDK enrichment so automatic properties cannot bypass these rules.
No automatic Resource detectors or `OTEL_*` overrides may broaden collection or change the destination.

## 9. Accepted decisions and remaining review

| Question | Recommendation | Alternative / impact |
|---|---|---|
| Default upload policy? | Enabled by default; configurable and disableable in `config.toml` (user decision). | Explicit false persists across restart and takes effect before exporter startup. |
| Direct Cloud or gateway? | Direct Cloud. | Gateway adds operations but centralizes routing/limits and hides client IP from PostHog. |
| Distributed ingestion token? | Accepted: allow a narrow ingestion-only exception. | Personal, secret and query/admin API credentials remain prohibited. |
| Installation retention? | Accepted: independent random analytics identifier scoped to enabled periods. | No device-identity derivation, person profiles or cross-device merge; rotate after disabling analytics. |
| Metric stream semantics? | Delta without installation labels. | Cumulative requires isolated stream identity and larger cardinality. |
| Cloud region/project? | User selects region; separate development and production projects. | No implicit US default or shared test/production data. |
| Alpha metrics readiness? | Validate the selected project before release. | Defer metrics activation while retaining instrumentation and independent signal switches. |

The user has accepted default-on uploads, the ingestion-token exception and the independent random analytics identifier.
Cloud region/project and the remaining integration choices are still under review.
Implementation must update the live contract and architecture in the same change.
This decision remains in proposed status while integration review and implementation are pending.
