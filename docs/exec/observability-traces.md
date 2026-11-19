# Service-owned submission traces

- State: Implemented. Final-source acceptance is recorded below.
- Baseline: `28e1c5a93fb908a394034ff84d9086ef638f0300`.
- Worktree: `3adcd614-3a07-4ac9-a893-6447cb95ae2b`.
- Branch: `feat/observability-traces`.
- Specifications: [architecture](../architecture/observability.md),
  [contract](../contracts/observability-contract.md#13-sampled-explicit-submission-traces).
- The [failure inventory](observability-traces-failures.md) and first process E2E
  preceded runtime implementation. No unit tests were added.

## Runtime

The existing service diagnostics assembly now owns an OTel v1.47 TracerProvider
and matching official OTLP/HTTP trace exporter. The only new runtime setting is
`telemetry.traces.enabled`, default `false`. The telemetry master also controls
admission. Endpoint restart semantics and exact release credential binding are
shared with logs and metrics. Runtime `OTEL_*` values do not grant upload,
change sampling, or contribute Resource fields.

Unique host submission admission starts `unic.submission`. The Engine commit
call is its `unic.commit` child. Host retries reuse the original request result
without another span. Import rejection can produce only a root. Both spans
measure local processing, not network completion or paste. The public Engine
and device/plugin protocols gain no propagation fields or OTel dependencies.
Incoming trace context, baggage and socket metadata are ignored. Only finite
result/code attributes are admitted. The final protobuf transport replaces
Resource and strips Scope attributes before network I/O.

The queue holds at most 256 completed spans. One batch holds at most 128 spans.
The worker exports every five seconds or at batch capacity. It never waits for
queue space. Timeout is three seconds; retries and redirects are disabled.
`ParentBased(TraceIDRatioBased(0.01))` keeps child sampling consistent. Pressure
can drop individual completed spans and leave incomplete trees; this is explicit
lossy diagnostic behavior. Local counters include queue, exporter and disable
loss. Disable cancels in-flight requests, discards pending work, and waits for
SDK/transport cleanup. Re-enable cannot receive spans from the old generation.
All signals retain one shared two-second shutdown flush budget.

## Sampling and capacity decision

The initial candidate was measured against four matched 1000-submission process
runs: disabled, enabled/unsampled, enabled/100% and the production 1% sampler.
The synthetic 100% build exported 2000 spans in 16 requests, with 204032 request
body bytes. The candidate 1% build exported 11 roots and 11 children in one
request, with 2349 body bytes. This was a 98.8% body-byte reduction for this run.
Disabled and unsampled runs made zero requests. Production-candidate CPU was
1.67 seconds versus 1.66 disabled. Their p99 local submission latencies were
3.517 versus 3.496 ms. These short single-run observations do not establish a
release overhead limit or statistical significance.

Adopt 1% for the first opt-in release. The workload offered roughly 500 roots/s;
at that rate, one five-second period expects about 50 completed spans at 1%.
A queue of 256 plus a batch of 128 accommodates that synthetic baseline while
keeping a fixed memory bound. This is not a throughput guarantee. Slow exports
and bursts still drop spans. Increasing capacity is not a substitute for
measurement. The saturation E2E observes local drops while commits succeed.

The final matched runs retain CPU, RSS, handles, p50/p95/p99 submission latency,
HTTP request counts/body bytes and encrypted TLS socket bytes. The separate
[0020 follow-up](observability-sync-followup.md) measures matched submission and
remote clipboard availability through a private TCP/adapter fixture. Those
latencies include adapter scheduling and polling; they are not pure Tailcat
transport measurements.

## Acceptance

The minimal production-process TLS E2E first failed on the unknown trace config
key. After implementation, it passed default-off full-window admission and
exact root/child/idempotency assertions. The development matrix subsequently
passed all trace switch, exporter, saturation, generation, restart, handle and
shutdown scenarios. A review found discarded-export loss undercount; a process
assertion was added before its correction. Development resource artifacts whose
binary identities were removed too early are not final evidence.

Final-source commit: `f06ba4a7238e62db35353435ce86f843da671622`.

| Check | Result |
|---|---|
| Production race trace processes | PASS: 13 scenarios, 108.50 seconds; default/master/signal off, idempotency, exporter faults/loss counters, saturation, generation cancellation, restart, privacy, handles and shared close budget. |
| Independent production TLS driver | PASS: three roots, three children, four CLI send commands, exact Resource, parent/timing containment and credential binding; normal five-second export period. |
| Logs/metrics/events and 0020 process regression | PASS: 14 suites and 76 subtests, 192.101 seconds. Source hashes remained unchanged; all 51 process profiles were removed. |
| All-package race regression | PASS: `go test -race ./...`. Native suites were not enabled. |
| Vet and changed-code tagged lint | PASS. Lint v2.15.0 reports zero new issues relative to the baseline. |
| Full affected tagged lint | BASELINE_FAIL: two existing issues in daemon_e2e.go and transport_e2e.go. No blanket lint PASS is claimed. |
| Documentation | PASS with seven pre-existing architecture staleness warnings; observability verification points at the checked source commit. |

The final cost run passed all four modes in 56.53 seconds. Each used a nonrace
production daemon and the same 1000-submission workload. The runner used race
detection. The production 1% run exported 8 roots and 8 children; every exported
child matched its root decision and parent ID.

| Mode | Roots | CPU s | p50 ms | p95 ms | p99 ms | Final RSS KiB | Final FDs | Requests | Body bytes |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| disabled | 0 | 1.81 | 1.467 | 2.042 | 2.382 | 48480 | 37 | 0 | 0 |
| production | 8 | 1.96 | 1.565 | 2.193 | 2.675 | 49760 | 38 | 1 | 1743 |
| sampled | 1000 | 1.78 | 1.441 | 2.051 | 2.394 | 50528 | 38 | 16 | 204032 |
| unsampled | 0 | 1.82 | 1.454 | 2.329 | 3.824 | 49664 | 37 | 0 | 0 |

The TLS receiver measured actual encrypted socket bytes separately from decoded
request-body bytes. The [resource receipt](evidence/observability-traces/resources.json)
retains both directions, before/after resources and matched sync batches. The
short-run differences are observations, not release overhead limits. The 30-switch
functional check released its prior HTTP connection and passed its FD bound.

The [small receipt](evidence/observability-traces/receipt.json) records exact
commands, identities, outcomes, measurements and artifact digests. The repository
retains only the sanitized evidence needed to review this slice. Full private
artifacts are in `.cache/observability-acceptance`.

The build metadata can report `vcs.modified=true` because documentation verification
was updated after the source commit. The source manifest verifies every code,
build and runner file against the frozen identities. The final receipt commit
changes documentation and evidence only.

## Boundaries

- Hosted upload/read-back: see the [hosted receipt](observability-traces-hosted.md).
  The existing authorized project supports traces, but secure credential injection
  was blocked. No hosted success is claimed.
- Outgoing passive Client status remains [BLOCKED_UPSTREAM](0020-tailcat-client-status.md).
- Cross-device propagation, automatic capture/Action/import spans and automatic
  paste analysis are excluded. Existing source metrics/events remain intact.
- External DNS outages, OS ENOSPC/unlink, real system sleep, public peer-relay,
  native clipboard and other-platform native runs are NOT RUN here. No shared
  host network, disk, sleep state, VM or desktop was changed.
- A 1% sample can miss a whole short session. It cannot be used as a reliable
  request ledger or an exact user-action count.

## Replay and cleanup

Run `make traces-e2e`, or pass a private output directory to the
[replay script](evidence/observability-traces/replay.sh). The canonical CLI runner
uses explicitly initialized private profiles and real production programs. The
functional run uses race daemons. The cost run uses matched nonrace daemons;
the test runner may independently use race detection. The standalone production
[driver](../../e2e/tracecloud/README.md) separately verifies local TLS and supports
an already-authorized hosted environment without creating a target.

Artifacts contain exact source/program hashes, sanitized decoded OTLP, per-case
assertions, resource samples, replay commands, cleanup receipts and SHA256SUMS.
The runner reaps private services, closes the receiver, removes profiles,
databases and programs, then seals cleanup and checksums. Repository evidence
contains only small sanitized receipts. It contains no credentials, personal
history, database, program, source snapshot or large log.
