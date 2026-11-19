# Observability C acceptance

- Scope: process acceptance for [0014](0014-client-observability-otlp.md) and
  [0020](0020-tailcat-sync-observability.md), macOS arm64, Go 1.27.3.
- Traces: [independent implementation plan](observability-traces.md).
- Outgoing passive path status: [BLOCKED_UPSTREAM](0020-tailcat-client-status.md).

## Results

The final local runner passed eight top-level suites and 49 subtests in 244.591
seconds. The sampler runner passed the 0, 1 and 8-session comparisons. Production
services with the pinned Tailcat stack passed both local path cases in 126.094
seconds. See the [assertions](evidence/observability-c/assertions.json), the
[decoded payload metrics](evidence/observability-c/metrics.json), and the
[build and source manifest](evidence/observability-c/manifest.json).

The full `go test -race ./...` regression passed. The documentation checker
passed with seven architecture staleness warnings.

## Fault and metric matrix

| Scenario | Assertion and evidence |
|---|---|
| DNS lookup rejection, TLS trust failure, connection refusal | The real exporter receives a typed error. Both signals report the expected finite code. The sender commits and a paired receiver applies a fresh clipboard fixture. |
| HTTP 401, 429, 503 and delayed response | Same business assertion. The diagnostics report unauthorized, rate limited, unavailable and timeout respectively. Disable returns before the exporter timeout and no new requests reach the fixture in its post-disable window. |
| Queue saturation and local retention | The delayed receiver holds the log worker. The bounded queue records a drop. Business commit still succeeds. Log rotation and refusal to follow a profile symlink are covered by the process scenarios. |
| Partial transfer and dial timeouts | Both endpoints report exactly 17 bytes for the failed transfer. The receive path emits no automatic application. Repeated failed dials leave recovery pending and one reconnect closes the episode. |
| Daily marker persistence | A nonregular marker suppresses the analytics event while the receiver applies the content and increments the remote application counter. |
| Payload measurement | The separate payload scenario uses one sender and two paired receivers. It checks byte equality, exact successful byte counters, histogram counts, sums and bucket distributions in both directions. The four inputs are text plus HTML, PNG, a 2048-byte file and an empty file. |
| Real Tailcat direct and DERP paths | Local direct delivery continues after relay removal. UDP-disabled delivery reports the incoming DERP path. Outgoing coverage is unknown. |

The DNS fixture uses Go's local special-use name rejection. DNS infrastructure
failure and reconnection through a real external resolver remain untested.
This run also leaves OS-level ENOSPC/unlink failures, real system sleep,
native clipboard and cross-platform native acceptance open. The 0020 receipt
still owns untested credentials/identity failures, peer-relay and stale status
on the real stack, pending recovery revocation/suspension, crash and full
analytics source coverage. The local DERP fixture does not test public DERP.

## Sampling and resource method

The test-only host measures elapsed time in the collector invoked by the real
metrics worker. Use matched non-race daemons, a 10-second sampler, the normal
60-second metrics export period and synthetic local TCP peers. Compare metrics
disabled and enabled on the same daemon at 0, 1 and 8 sessions. Exclude the
initial configuration wake before taking each 21-second measurement. Record
collector durations, process CPU, RSS, heap, Go goroutines and Darwin kernel
wake counters. The wake counters represent the whole process. They cannot
attribute wakeups exclusively to the collector. The collector timings exclude
export serialization, network I/O and scheduling delays outside the call.

| Sessions | Disabled collector calls | Enabled collector calls | Enabled collector timings |
|---:|---:|---:|---|
| 0 | 0 | 0 | No periodic calls after settling |
| 1 | 0 | 2 | 12.7, 29.7 µs |
| 8 | 0 | 2 | 29.1, 31.8 µs |

At eight sessions, disabled/enabled process CPU was 0.04/0.03 seconds. Darwin
interrupt wakeups were 1226/1120. Package idle wakeups were 2/0. RSS varied
between samples. These short windows establish the observed cadence and call
cost. They provide neither a wakeup attribution nor a release overhead limit.

The 0014 load fixture also executes 50 commits in each of three runs per mode.
The median of the three per-run latency quantiles is below. The measurement
includes a local HTTP round trip. Raw measurements and request counts are in the
[resource artifact](evidence/observability-c/resources.json).

| Mode | p50 ms | p95 ms | p99 ms |
|---|---:|---:|---:|
| Disabled | 8.803 | 15.677 | 16.457 |
| Normal | 8.647 | 16.043 | 21.016 |
| Debug | 8.614 | 12.357 | 17.180 |
| Queue pressure | 8.107 | 13.195 | 19.652 |

Per-mode handle counts, synchronized delivery latency quantiles and network-byte
measurements remain open. These measurements do not supersede the historical
0020 eight-session race-daemon RSS failure.

## Hosted read-back

The authenticated connector queried US Cloud project 722858. Independent
queries used the service version and UTC window in each upload receipt.

| Run | Verified in Cloud |
|---|---|
| 0014 logs, metrics and events | Two sanitized warning logs, commits split 3 + 7, matching delta histogram counts and buckets, two anonymous startups and ten manual-send results. |
| 0020 production Tailcat over forced local DERP | 22 successful bytes in each direction. One 22-byte histogram sample in each direction. Four incoming DERP samples and seven outgoing unknown samples. No sample classified as direct. |

The Cloud artifacts include the upload states, exact query definitions,
sanitized results and checks. See [0014 read-back](evidence/observability-c/cloud-0014.json)
and [0020 read-back](evidence/observability-c/cloud-0020.json).

## Replay

Run the [local replay script](evidence/observability-c/replay.sh) on macOS with
Go, Python 3 and permission to bind loopback endpoints. Pass an artifact
directory as the single optional argument.
Verify the retained evidence with `shasum -a 256 -c SHA256SUMS` from its directory.

Cloud replay writes synthetic telemetry to the configured project. Set
`UNIC_POSTHOG_HOST`, `UNIC_POSTHOG_PROJECT_ID` and the project's ingestion-only
`UNIC_POSTHOG_PROJECT_TOKEN` in the process environment. Run
`go run ./e2e/posthogcloud --artifact .cache/obs-c/cloud` for logs and pipeline
metrics. Run `sh e2e/syncpath/cloud.sh .cache/obs-c/cloud-sync` for sync metrics.
Use each run's new service version and time window when repeating the queries.
