# PostHog integration: implementation and acceptance plan

- Status: Design only; runtime implementation and ingestion acceptance NOT RUN.
- Date: 2026-11-10.
- Decision: [0019 proposal](../decisions/proposed/0019-posthog-integration.md).
- Baseline: [current observability](../architecture/observability.md).

## 1. Required decisions

The user accepted default-on uploads with config.toml controls, distribution of the ingestion-only project token, and an independent random analytics identifier.
Do not request those decisions again.
Resolve the Cloud region/project and remaining integration choices before implementation.
Confirm Metrics enablement in the selected Cloud project.
Use a development project for hosted acceptance. Keep production collection disabled during development.

## 2. Smallest complete implementation sequence

1. Define contract amendments and E2E scenarios before writing runtime code.
2. Add validated OTLP base-path support and controlled ingestion-token assembly. Retain current queues, filters and lifecycle barriers.
3. Exercise real `unic`/`unicd` processes against a local TLS OTLP receiver. Decode requests and verify exact PostHog paths and headers without retaining token values.
4. Configure delta metrics and prove multi-process aggregation, resets and histogram merging.
5. Pin the official Go SDK after checking all queue and cancellation behavior. Add the analytics controller, persistent upload settings, private identity handling and health status.
6. Complete one vertical slice: default-enabled real product startup, captured `app_started`, disabling through `config.toml` and zero subsequent requests.
7. Add the four user-operation events at authoritative boundaries. Keep normal clipboard observations in metrics only.
8. Verify Cloud ingestion and query results in the selected development project. Build dashboards only after semantics pass acceptance.
9. Update the live architecture and contracts, run docscheck with the appropriate architecture bumps, and record exact implemented/accepted scope.

Do not add unit tests after implementation.
Use real multi-process E2E as the acceptance mechanism.
If isolation is essential, write the complete relevant failure inventory before that isolated implementation.

## 3. Failure inventory and required E2E evidence

| Scenario | Required result |
|---|---|
| First run with no upload overrides | All three signals default enabled; valid release destination/token are used. |
| Master switch false before first startup | Zero telemetry HTTP requests, including during initialization; local logs and clipboard operations still work. |
| Config hot reload to master/signal false | Barrier completes without restart; corresponding requests stop. |
| Restart after explicit false / invalid configuration edit | False persists across restart; invalid edits retain last valid settings and never restore enabled defaults. |
| Missing release endpoint or ingestion token | Safe unavailable/degraded state; no guessed destination or business failure. |
| Only one signal enabled | Only that signal uploads. Raising log level never enables another signal. |
| PostHog host plus `/i` base | Exact `/i/v1/logs` and `/i/v1/metrics` paths; one Bearer header; no token in query or diagnostics. |
| Existing pathless Collector endpoint | Existing signal paths remain correct. |
| Wrong region, token rejected, 429, TLS/DNS failure, stall | Safe degraded state, bounded timeout and queues; no business failure or recursive export. |
| Redirect to another host | No redirected request and no credential forwarding. |
| Hostile SDK/OTEL environment and automatic properties | Final destination/resource/event properties match approved configuration and allowlists. |
| Same dimensions from two processes, counts 3 and 7 | Combined increase is 10; repeated collection without new work adds zero. |
| Metrics restart and disable/re-enable | No negative spike, stale delta or double count; new work appears once. |
| Known histogram samples from two processes | Total count, sum and merged buckets match fixtures; percentile tolerance is explicit. |
| Queue saturation / receiver offline | Declared aggregate queue bounds hold; losses are counted; clipboard actions remain responsive. |
| Disable during initialization, queued work or in-flight HTTP | No new request after barrier return; old generation cannot resume after re-enable. |
| Normal exit / SDK default Close would block | Entire diagnostics shutdown fits the shared two-second budget; workers terminate. |
| Analytics initially enabled and restart | Identifier is created only when effective analytics is enabled and reused for that profile across restart. |
| Disable and re-enable | Old queue is discarded; identifier rotates; disabled-period actions are never replayed. |
| Independent profiles/devices | No business/device identity is exported or merged. No person profiles are created. |
| Clipboard/path/pairing/search canaries | No canary in request payloads, resources, headers other than the designated token, SDK diagnostics or saved acceptance artifacts. |
| Desktop request plus daemon completion / IPC retry | One operation event per completed user operation; no per-target inflation. |
| Failed or cancelled action | Finite terminal result is accurate; success is never inferred from queue admission. |
| Background receive / ordinary clipboard observation | No user-action product events; existing metrics remain correct. |
| Native prototype settings | Do not claim functional upload settings integration from a demo switch. Native interaction acceptance is separate. |

## 4. Reviewable acceptance artifact

The future harness must emit a self-contained sanitized directory:

```text
manifest.json              source SHA, scoped dirty diff digest, build flags, binary hashes, dependency versions
commands.json              exact replay commands and exit codes
config.redacted.json       endpoint/region/signal configuration; credential presence only
requests.redacted.jsonl    monotonic request timeline, generation, path/status; no credential values
otlp.decoded.jsonl         filtered Resource, log records and metric data points
analytics.decoded.jsonl    event names, approved properties, fixture-only pseudonymous identities
assertions.json            per-scenario expected/actual values and PASS/FAIL/NOT_RUN
resource-summary.json      measured queue/memory/worker bounds and measurement method
hosted-receipt.json        project/region, query window, sanitized read-back counts; local/hosted states separate
replay.sh                  local deterministic replay entry point
SHA256SUMS                 artifact integrity
```

Never save the project token, administrative credentials or real installation identities.
Hosted read-back uses separate read credentials through the acceptance environment.
Acceptance data must use fixed synthetic payloads and isolated profiles.
HTTP success alone does not prove query visibility, correct aggregation or absence of person profiles.
Record local receiver acceptance, actual Cloud ingestion, query read-back and native interaction separately.

## 5. Initial query and dashboard definitions

| View | Definition |
|---|---|
| Installation activity | Distinct enabled-period analytics IDs with `app_started`, split by product/version/platform. Not a people count. |
| Pairing adoption | Installations with `pairing_completed` relative to started installations in an explicit window. |
| Manual history usage | Successful `history_applied` events and participating installations. |
| Send / Action completion | Terminal events split by result; no claim of remote application success from sender completion. |
| Pipeline health | Operation/error rates with a stage-matched denominator; stage p50/p95 latency. |
| Telemetry health | Diagnostic losses, export failures, actual receipt delay and missing intervals. |

Define cohort/window handling before naming an activation or retention metric.
Do not create remote dashboards or alerts during this design task.

## 6. Current receipt

| Layer | Status |
|---|---|
| Existing source and contract inspection | Complete |
| Official PostHog logs/metrics/events capability check | Complete; checked 2026-11-10 |
| Proposed design and failure inventory | Complete; default-on uploads, ingestion-token distribution and random analytics identity accepted; remaining choices pending |
| Runtime/configuration/SDK changes | NOT IMPLEMENTED |
| Local ingestion E2E | NOT RUN |
| Cloud project, region, token and Metrics enablement | NOT SELECTED / NOT VERIFIED |
| Hosted query read-back, dashboards and native upload settings interaction | NOT RUN |

Documentation validation results belong in the task progress record.
