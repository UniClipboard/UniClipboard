# Sync observability follow-up acceptance

## Failure inventory written before scenarios

The process boundary is the production CLI and service, the existing tagged local
TCP transport, real session identity/wire processing, and the local TLS receiver.
The TCP timeout/disconnect controls are injected conditions. They do not establish
Tailcat route, peer-relay, native clipboard, or public network acceptance.

| Failure | Required observable behavior |
| --- | --- |
| Pending recovery survives device revocation | Revocation abandons exactly one episode. It never counts recovery. Subsequent collection no longer produces pending samples. |
| Pending recovery survives sync suspension | Suspension interrupts exactly one episode. Resume starts a fresh session and does not recover the interrupted episode. Fresh content still reaches the paired receiver. |
| Temporary failed dial is confused with a terminal episode | Pending samples exist before each control-plane change. Repeated failed dials do not close the episode. |
| Revoked session reconnects after removing fault controls | The revoked device remains absent. No recovered episode or automatic application appears. |
| Sensitive peer or clipboard material leaks | Decoded metrics and product events contain no content, peer identity, address, request ID, profile path, or ticket. |
| Daily persistence fails after startup | A private bounded filesystem obstruction suppresses the daily event. Automatic application and operational counters remain usable. |
| Daily marker corruption at startup is accepted | A malformed or invalid-day marker fails closed. Automatic application remains usable. |
| Forced termination invents recovery | A new service process has fresh operational metric baselines. Persisted daily coalescing survives the restart. |
| Wrong wire version or forged identity proof is classified as successful | A synthetic TCP peer fails the real handshake. The exporter records exactly one finite protocol/identity error per attempted handshake. No session or history is created. A later legitimate peer still pairs and synchronizes. |
| An action or imported item is classified as local/remote | Real CLI Action and v1 fixture import produce their expected finite source kind when explicitly applied. Idempotent repeats produce no second history event. |
| Telemetry pressure changes business success or hides resource cost | Use the same paired TCP workload in disabled, normal, debug, and queue-pressure modes. Three batches of twelve fresh copies must all reach the receiver. Record submission latency, observed synchronization latency, process CPU/RSS/heap/goroutines/file descriptors, OTLP request/body totals and receiver TLS socket bytes. This is a matched observation, not a release overhead limit or Tailcat network measurement. |
| External source classification is replaced by a local origin or remote-device availability | A test-only external-channel backend calls the public Receiver. Real receive, history and adapter application must report external history source. Repeating the same transfer/view must not create another history item. External channels must produce zero remote-device application metrics and daily availability events. |
| A plugin observer panic prevents later business or observer delivery | A test-only plugin observer panics from every committed callback. External receive and a fresh local commit must both complete and both callbacks must execute. Plugin observer panic is distinct from the internal syncdiag.Observer interface; the latter remains outside this fixture. |

## Execution

The scenarios are implemented in the
[process runner](../../e2e/cli/sync_followup_e2e_test.go). They use explicitly
initialized private profiles and real `unic` / `unicd` programs. The `unic_e2e`
tag supplies a local TCP transport, directory-backed adapter, runtime-state
control, and opt-in fixture plugins. The tag supplies no product telemetry
records. The final frozen-source run passed with the
[Traces acceptance](observability-traces.md). Source commit:
`f06ba4a7238e62db35353435ce86f843da671622`. The complete related regression
passed 14 top-level suites and 76 subtests in 192.101 seconds. All 51 process
artifact manifests have stable source hashes, nonempty binary identities and
passing cleanup receipts.

| Scenario | Process assertion |
| --- | --- |
| Pending recovery revocation | One `abandoned` episode; no `recovered` episode; no further pending histogram samples over eleven seconds; no subsequent delivery to the revoked device. |
| Pending recovery suspension | One `interrupted` episode; no further pending samples over eleven seconds; resume and a new content transfer succeed without recovering the interrupted episode. Suspension uses `SetRuntimeState`, not the content-only `sync.enabled` switch. |
| Wrong wire version | A synthetic TCP peer sends a wrong version to the real handshake; one `protocol_mismatch` error appears. No session or history is created. |
| Forged identity proof | A synthetic TCP peer sends an invalid Ed25519 signature to real identity verification; one `identity_mismatch` error appears. A later legitimate peer pairs and receives fresh content. |
| Daily marker faults | Malformed JSON, invalid date, marker symlink, nonempty startup temporary directory, occupied runtime temporary file, and runtime rename obstruction all preserve two successful remote applications and suppress daily analytics. Only reserved files inside private profiles are obstructed. |
| Crash | Kill one private daemon with SIGKILL during pending recovery. Restart with the same initialized profile. Fresh content applies; operational counters start fresh; no old recovery episode is reconstructed; persisted same-day analytics is not repeated. |
| Local/action/migration sources | A real offline synthetic v1 import and real CLI Action/history operations yield exactly one explicit history event for each finite source. Retried applications do not duplicate events. |
| External source and duplicate | A test-only external-channel plugin uses the public Receiver. Explicit history application reports `external`. Duplicate transfer/view is declined and counted as `receive/duplicate`; history remains singular. External content does not increment remote-device availability. |
| Plugin observer panic | A separate test-only plugin panics from every committed observer callback. Both the external receive and a subsequent local commit complete. Two callbacks execute despite the first panic. |
| Matched sync cost | Disabled, normal, debug, and queue-pressure modes each execute three batches of twelve copies. Every copy reaches the receiver before the next copy. Queue-pressure modes prove actual local drops. |

The first complete development execution passed all business and cost assertions.
Its canonical source-identity checks detected concurrent Traces edits, so it is
not frozen-source acceptance. The external extension first failed without its
fixture routes, then exercised the actual external receive path. Earlier invalid
test assumptions were corrected: content sync disable does not suspend sessions,
and the public external Receiver rejects duplicate messages.
A combined regression exposed an adapter-readiness race in the external fixture:
startup deliberately ignores pre-existing receipts. The scenario now completes
a public clipboard-read command round trip before injecting fresh external
content. This proves the adapter ready barrier without timing sleeps.

## Resource method

Use the same daemon build mode, content size, paired topology, adapter, TLS
receiver, polling cadence, and number of copies in all four cost modes. Record
each CLI submission latency and the elapsed time until the receiver's directory
adapter exposes the expected content. The latter includes CLI startup, adapter
scheduling and the runner's 100 ms observation polling. It is not a pure network
latency or an actual paste measurement.

Record CPU seconds, RSS, Go heap, goroutines, and numeric file-descriptor counts
for both daemons before and after each batch. Retain only the descriptor count,
never `lsof` paths. Record OTLP request/body totals and TLS socket bytes from the
local receiver. These bytes exclude business TCP traffic. The TCP fixture does
not measure Tailcat, DERP, peer-relay, or public network cost. Prefer non-race
daemons for release resource observations; a race-run's RSS includes race runtime
cost. No release overhead threshold is established by this follow-up.

## Final matched nonrace observations

The final four-mode run passed twelve batches of twelve copies in 78.06 seconds.
Every copy reached the directory adapter. The runner used race detection; both
service programs were nonrace builds. The median of each mode's three per-batch
p50/p95 values is shown below. Units are milliseconds.

| Mode | Submission p50 | Submission p95 | Availability p50 | Availability p95 |
|---|---:|---:|---:|---:|
| disabled | 26.489 | 30.366 | 422.744 | 444.186 |
| normal | 27.008 | 30.307 | 423.749 | 443.978 |
| debug | 26.321 | 32.441 | 424.579 | 443.841 |
| queue-pressure | 26.145 | 30.820 | 423.293 | 444.771 |

Disabled batches made zero telemetry requests and transferred zero TLS bytes.
The [resource receipt](evidence/observability-traces/resources.json) retains each
batch's CPU/RSS/heap/goroutines/FD counts and request/body/TLS-byte deltas for both
processes. These TLS bytes exclude business TCP traffic. Source identities and
cleanup passed for all eight private process profiles. This acceptance does not
supersede the historical race-daemon eight-session RSS failure or the C sampler
measurements. It establishes no release overhead threshold.

## Replay and retained evidence

Build the tagged daemon and CLI into a temporary directory. Set `TMPDIR` to
`/private/tmp` on macOS. The runner requires permission to bind loopback ports,
read its child-process counters, and run `lsof`.

```sh
go build -race -tags unic_e2e -o "$bin/unicd" ./app/service/cmd/unicd
go build -race -o "$bin/unic" ./app/service/cmd/unic
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN="$bin" \
UNIC_E2E_ARTIFACTS="$dest" go test -race ./e2e/cli \
  -run '^TestSyncObservability(Recovery|Daily|Handshake|Crash|Source|External)Followup$' \
  -count=1 -v
```

For the matched cost run, build both programs without `-race` and select
`^TestSyncObservabilityCostFollowup$`. The Go test runner may still use race
detection. Each manifest records the daemon's actual build settings.

The canonical runner writes exact source/program identities, redacted
configuration, decoded telemetry, per-scenario assertions, resource samples,
commands, replay entry, cleanup receipt, and SHA256SUMS. Keep only small sanitized
receipts in the repository. Remove fixture profiles, credentials, legacy source
fixtures and programs after execution. No personal history, source snapshot,
binary, database, or large raw log belongs in the retained repository evidence.

## Remaining boundaries

- Internal `syncdiag.Observer` panic isolation is **NOT RUN**. The fixture above
  tests the public plugin Observer only. The host installs its sync observer
  before backend initialization and exposes no safe replacement hook; no new
  production hook was added.
- Damaged Tailcat credentials and real-stack stale/peer-relay faults remain
  **NOT RUN** in this follow-up. The raw-peer scenarios prove actual identity and
  protocol verification over the private TCP transport, not Tailcat credential
  loading or routing.
- OS ENOSPC/unlink, external DNS infrastructure, whole-system sleep, native
  clipboard, actual paste, and other-platform process runs are **NOT RUN**.
- Outgoing passive Client path status remains independently
  [BLOCKED_UPSTREAM](0020-tailcat-client-status.md).
- The previously accepted [C group](observability-c-acceptance.md) remains
  accepted; this follow-up does not reopen its payload, sampler, exporter-fault,
  or hosted logs/metrics results.
