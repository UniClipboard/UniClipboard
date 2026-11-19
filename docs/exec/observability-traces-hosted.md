# Hosted trace target verification

- Checked: 2026-11-19 UTC.
- Target: existing PostHog US Cloud / UniClipboard, project 722858.
- Capability and target identity: VERIFIED_READ_ONLY.
- Local production-driver TLS acceptance: PASS.
- Hosted trace ingestion and synthetic root/child read-back: NOT_RUN; controlled
  credential injection is unavailable after automatic approval rejection.
- Local implementation and process acceptance: [Traces receipt](observability-traces.md).

## Existing target and access

The authenticated PostHog connector returned project 722858 with name
`UniClipboard`, API host `https://us.i.posthog.com`, administrative read access
and an available ingestion project token. The check retained only those
sanitized facts. It did not print or persist the token. No new target, project,
Collector, project policy or resource was created.

The connector's initial active project differs from 722858. An unqualified APM
query would therefore inspect the wrong project. The `switch-project` tool can
select 722858 for subsequent connector calls. This read-only target check did
not change that selection or query the other project's spans.

## Trace capability

The connector's current `posthog:instrument-tracing` skill and `docs-search`
independently specify a general application OTLP HTTP/protobuf endpoint:

```text
https://us.i.posthog.com/i/v1/traces
Authorization: Bearer <existing ingestion project token>
```

This capability was checked directly. It was not inferred from working logs or
metrics. The AI-only `/i/v0/ai/otel` endpoint is outside this implementation.
See the official [getting-started guide](https://posthog.com/docs/distributed-tracing/start-here)
and [Go guide](https://posthog.com/docs/distributed-tracing/installation/go).

The existing runtime binds its release credential to the exact configured
release base endpoint. A synthetic build must preserve that binding when it
adds `/v1/traces`. Runtime `OTEL_*` variables do not authorize a new destination.
The checking process had no `UNIC_POSTHOG_*` or OTLP endpoint/header environment
values. This does not mean that the verified connector lacks a project token.

## Local production-driver acceptance

The [tracecloud driver](../../e2e/tracecloud/README.md) builds race-enabled
production `unic` and `unicd` without test tags. It explicitly initializes one
private profile, disables sync and all other remote signals, and enables traces
with a test-only 100% root sampling override. It issues four public CLI sends
with three unique idempotency keys. It waits eleven seconds while preserving
the normal five-second trace batch period.

The first local TLS run accepted one 765-byte OTLP request with exactly three
`unic.submission` roots and three `unic.commit` children. Repeating one send
did not add a child. The receiver rejected content, request, private-path,
environment and credential canaries before retaining decoded spans. Local
exporter state remained active with zero failures/drops and the other signals
disabled. Source hashes remained unchanged. The daemon exited normally; its
private profiles, database and programs were removed. The later final run also
validates exact Resource keys/values and child timing containment.

Final-source replay and private local artifacts: `.cache/observability-acceptance/local-driver`.
The final driver ran on `f06ba4a7238e62db35353435ce86f843da671622` and retained
stable source hashes, three root/child pairs and a passing cleanup receipt.
Each run retains source/program identity, sanitized configuration, decoded
spans, assertions, cleanup and `SHA256SUMS`. Earlier development runs are separate from the retained final-source receipt.

## Remaining hosted execution and approval boundary

The available connector exposes `apm-services-list`, `query-apm-spans`,
`apm-trace-get` and APM aggregates. Their discovery proves tool availability,
not successful read access to this project's ingested spans. Before read-back,
select and verify project 722858. Restrict queries to the isolated synthetic
service version and upload UTC window. Retain only counts, finite names and
validated root/child relationships.

No hosted trace upload or read-back occurred. Automatic approval rejected the
attempt to inject the connector-captured ingestion token through generated
shell command text. The stated reason was that the token would enter shell
arguments and execution records. The requested safer alternative is controlled
private environment injection. This execution tool exposes no environment-map
parameter. Checks of `.env`, `.env.local`, `.env.posthog`,
`.cache/0019/cloud.env` and `.cache/obs-c/cloud.env` found no existing regular
configuration file. No broader credential search, temporary credential file,
indirect execution or retry of the rejected action was performed.

The missing evidence is a completed production-binary synthetic upload and
project-scoped matching query after a safe credential channel is available.
The endpoint and ingestion credential are available. Do not describe them as
missing configuration. The sanitized approval outcome is retained in the
[small receipt](evidence/observability-traces/receipt.json). An HTTP success
response alone cannot establish ingestion: the current official
[Node.js guide](https://posthog.com/docs/distributed-tracing/installation/nodejs)
warns that an unknown project token can still receive HTTP 200.

No secret, personal history, database, binary or source snapshot is retained
in this receipt. The rejected hosted action started no process and sent no
telemetry. The connector's original active-project selection was unchanged.
