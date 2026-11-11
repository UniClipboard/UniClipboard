# 0020: Tailcat sync observability implementation plan

- Status: Core runtime implemented; local process and incoming real-stack path acceptance passed. Symmetric passive status is blocked upstream. Final receipt below.
- Date: 2026-11-10
- Decision: [0020 design](../decisions/proposed/0020-tailcat-sync-observability.md).
- Current baseline: [observability architecture](../architecture/observability.md) and [observability contract](../contracts/observability-contract.md).

## 1. Scope and completion rules

Deliver connection-failure diagnostics, session stability and recovery, passive direct/DERP classification, payload performance, remote clipboard availability and deliberate remote-history retrieval.

Actual paste detection is excluded. Do not add input hooks, accessibility monitoring, pasteboard read/provider observation or inferred paste events. Do not introduce cross-device identity linking, wire ACKs, trace propagation or exact per-path network-byte accounting.

This plan contains proposed work, not a runtime receipt. Preserve working sync and unrelated workspace changes. Verify the exact checkout and dependency identity before implementation. Implementation started on branch `v2` at `edad4381b8a080d583c7a558c517b2d671c8fdf3`. Existing 0020 documents and registration changes were preserved. Acceptance artifacts identify the baseline commit, source-file hashes and actual binary hashes.

Use real-process E2E as the test mechanism. Write the failure inventory and process scenarios before runtime code. Do not write unit tests after implementing code. Keep fixture controls out of production public APIs and preserve a sanitized reproducible artifact at the end of each stage.

## 2. Work sequence

### P0. Freeze semantics and prepare E2E scenarios

- [x] Read the current Engine, backend, pipeline, history and observability contracts.
- [x] Recheck pinned tailcat/Tailscale APIs and upstream Client status availability.
- [x] Specify finite diagnostic DTOs, terminal ownership, path freshness, size buckets, histogram boundaries and reachable series counts.
- [x] Specify UTC daily analytics marker load, admission, persistence, disable and crash behavior. State bounded duplicate/loss limitations explicitly.
- [ ] Build E2E fixture scenarios from section 3 before production instrumentation. Reuse existing CLI/process, local receiver and session/wire fixture infrastructure.
- [ ] Record unchanged runtime failures before attributing later failures to this feature.

Exit evidence: reviewed catalog and executable E2E failure scenarios. A fake path source may verify export plumbing; it cannot prove the tailcat classifier.

### P1. Connection and passive-path vertical slice

- [x] Add the smallest finite observer at built-in tailcat assembly. Keep Engine independent of tailcat/OTel types and avoid a plugin RPC framework.
- [x] Capture network-start, dial and handshake terminal outcomes with monotonic timings, outside locks.
- [x] Add Server-side status reduction and verify private session-to-peer mapping.
- [ ] Obtain a maintained public Client status interface, preferably upstream; pin an immutable released revision. Verify concurrent startup/close behavior and outgoing mapping.
- [x] Use one bounded passive worker with the proposed 10-second cadence. Classify missing/stale/unmapped observations as unknown.
- [x] Drive two real services through connection, authorized session admission and a real transfer. Decode the new metrics at a local OTLP receiver.

Exit evidence: connection metrics and path classifications from real processes. Local direct and controlled relay-only cases must use the real pinned tailcat stack. If Client status is unavailable, record P1 as partial with the exact blocker. Do not substitute active probes or remove working sync to satisfy the gate.

### P2. Session stability and recovery

- [x] Add one terminal session claim and explicit expected-end reasons.
- [x] Track recovery only after unexpected admitted-session loss while reconnection remains eligible.
- [x] Record connected exposure, successful recovery durations, pending ages and abandoned/interrupted outcomes.
- [x] Resolve recovery through either incoming or outgoing replacement admission.
- [x] Bound all private bookkeeping by existing session/peer ownership. Do not persist a new recovery database.

Exit evidence: injected disconnect/recovery scenarios with exact episode counts and pending states; deterministic duplicate-connection replacement is excluded from fault frequency.

### P3. Payload performance

- [x] Reuse queue and whole-call pipeline metrics. Add actual payload bytes, declared size distributions and body-I/O duration.
- [x] Record partial bytes once on failure. Distinguish bytes read from successful validated receive.
- [x] Associate only fresh transfer-start path snapshots. Use unknown when unsupported; never block body I/O waiting for status or discovery.
- [ ] Verify size buckets, empty content, multiple parts and multiple targets without content/path labels.

Exit evidence: real multi-process transfers with known synthetic sizes and decoded counters/histograms. No cross-device clock subtraction or sender-success-as-receive claim.

### P4. Product value

- [x] Add the automatic remote-application event at accepted application completion; retain per-operation volume in metrics.
- [x] Extend `history_applied` with finite stored-origin classification. Keep explicit retrieval distinct from automatic application.
- [x] Implement at-most-once normal daily admission per analytics identity/UTC day in the service worker, including restart state. Document crash/export loss limitations.
- [x] Update final event filtering and privacy allowlists. Keep analytics identity private to the service.
- [ ] Verify IPC retries, duplicate reports, source mapping, daily rollover, restart, disable/re-enable and queue saturation.

Exit evidence: exact synthetic event counts from real IPC/application-result scenarios. Scripted host application reports prove telemetry semantics, not native clipboard operation or paste.

### P5. Fault, resource and integration acceptance

- [ ] Run all section 3 scenarios with decoded logs, metrics and product events.
- [ ] Check series cardinality and unknown-path coverage with the final catalog.
- [ ] Measure sampler worker count, CPU, memory, collection cost and scheduling activity at 0, 1 and many eligible sessions. Compare against the same sync fixture without new collection.
- [ ] Verify no extra dial or active probe is caused by sampling, including idle sessions and unavailable status.
- [x] Retain current exporter queue limits, disable barriers and shared two-second shutdown budget.
- [x] Run existing observability/PostHog E2E and the full CLI E2E suite once after the final relevant code changes. Rerun only failures or checks affected by subsequent changes.
- [x] Run headless build/dependency checks and Desktop build. No GUI launch is required or authorized by this plan.

Exit evidence: checksummed local artifact, source/binary identity and explicit PASS/FAIL/NOT_RUN per scenario. Set resource acceptance thresholds before runtime implementation after measuring the baseline; do not invent pass thresholds after results.

### P6. Hosted read-back and documentation landing

- [ ] When Cloud upload is separately authorized, use isolated synthetic profiles and the selected project's normal release deployment metadata.
- [ ] Query new metric/event data independently in the recorded upload window. Verify two-process aggregation, histogram samples, source categories and privacy.
- [x] Define versioned queries for stage failure rates, session exposure, pending recovery, passive path share, payload latency and value cohorts.
- [ ] Update living architecture/contracts only for implemented and verified behavior; bump only relevant architecture metadata using docscheck.
- [ ] Record remaining platform/upstream/native gaps, then land and archive the decision through the repository documentation lifecycle.

Exit evidence: independent hosted receipt and updated living documentation. Dashboard/alert publication, release publication and native OS acceptance remain separate from local receiver or Cloud-query success.

## 3. Failure inventory and E2E acceptance matrix

| Scenario | Required observation |
|---|---|
| Network start fails before any session | One phase-specific failure; business startup behavior follows the existing contract |
| Invalid address / damaged credentials | Finite categories; no secret text; no silent credential replacement |
| Dial timeout / connection refusal / cancellation | Correct phase and terminal count; cancellation shown separately from failure |
| Dial succeeds, identity handshake fails | Successful dial plus failed handshake; never a successful session |
| Pending approval or permission rejection | No bypass; no network-root-cause claim |
| Direct endpoint and Relay both present | Direct classification; configured DERP does not override it |
| Controlled real direct / relay-only network | Classifier agrees with independently controlled real-stack conditions |
| Missing, inactive, stale or unmapped status | Unknown; session not falsely disconnected |
| Peer relay reported | Separate peer_relay category; never DERP |
| DERP to direct, direct to DERP, unknown gap | Only fresh consecutive known transitions counted; timing uncertainty recorded |
| Path changes during transfer | start_path preserved; no claim of exact route or path byte share |
| Passive sampler runs | Zero sampler-caused dials/DiscoPing calls; bounded single worker |
| Transport startup/close races with status | No panic, deadlock, resource leak or business cancellation |
| Simultaneous incoming/outgoing connections | Deterministic replacement counted as expected; one admitted session terminal claim |
| Remote EOF / I/O failure | Observed reason only; no invented remote intent or network cause |
| Repeated failed reconnects | One recovery episode, multiple attempt outcomes, increasing pending age |
| Recovery arrives through incoming connection | Existing episode closes once as recovered |
| Revoke, suspend or stop during recovery | Episode abandoned/interrupted as specified; not a successful recovery sample |
| Still disconnected at collection end | Pending-age evidence; no omission from a success-only chart |
| Service crash/restart | No cross-process duration; interrupted/loss limitations visible |
| Known-size text/image/file, empty and multipart | Exact fixture payload counts and bucket membership; no bytes in labels |
| Partial body write/read then failure | Partial bytes once; no false validated receive/application success |
| One submission to two targets | Two transfer measurements; no duplicate user-action product event |
| Policy rejection, superseded queue item, sync disabled | Existing semantic results; no fabricated transfer or abnormal session end |
| Automatic remote application succeeds/fails | Daily event only on accepted success; operation metrics remain accurate |
| Explicit local/remote/external/action/migration history | Correct finite source kind; no automatic-application event for manual retrieval |
| IPC retries, duplicate apply reports and reconnect | One operation event; terminal telemetry not replayed |
| Multiple automatic applications, restart and UTC rollover | One normal daily admission per identity/day; next day may admit again |
| Daily marker persistence fails / worker crashes | Bounded, documented degradation; no business I/O/blocking or false exactly-once claim |
| Analytics disable/re-enable or master false at startup | Existing barriers honored; identity rotates; daily marker follows identity; disabled actions not replayed |
| Metrics disable/re-enable while transfers/sessions continue | Fresh baselines; disabled-period bytes/time not exported later |
| Exporter offline, queue full, 429 or HTTP stall | Existing bounded loss/timeout behavior; business sync continues |
| Endpoint/key/content/path/error canaries in status/failures | Absent from wire telemetry and retained artifacts |
| Cardinality pressure / many sessions | Finite reachable labels, current cap and bounded bookkeeping preserved |
| All telemetry disabled / zero sessions | No observation worker activity beyond documented lifecycle cleanup |
| Observer panic or slow export receiver | Business result unchanged; no callback wait for exporter/network/disk |

Controlled local DERP proves relay classification, not public DERP availability. Public network conditions require a separate receipt. Native clipboard application requires separately authorized native acceptance. No actual-paste test belongs in this matrix.

## 4. Reproducible artifacts and validation

Each stage emits a sanitized directory with:

```text
manifest.json              source identity, dependency revisions, build flags, binary hashes
commands.json              exact commands, environment key names, exit codes
scenarios.json             expected/actual values and PASS/FAIL/NOT_RUN
timeline.jsonl             monotonic fixture timeline; fixture-only correlation labels
otlp.decoded.jsonl         reduced resources, finite metric labels and sanitized logs
analytics.decoded.jsonl    approved event properties; fixture-only anonymous identities
network-conditions.json    real-stack fixture rules, path coverage and independent controls
resource-summary.json      baseline/candidate measurements and preset thresholds
replay.sh                  deterministic local entry point
SHA256SUMS                 integrity manifest
```

Record hosted results separately with project identity, time window, query definitions, sanitized results and independent per-signal assertions. Never retain actual endpoints containing credentials, device keys, real installation IDs or clipboard content. Fixture correlation must not become production telemetry fields.

Do not fabricate a runnable selector before the new E2E suite exists. The repository entry points are `make sync-observability-e2e` and `make sync-path-e2e`. Each checks its required parent PASS lines, so zero selected cases, skipped prerequisites, build failure or a failed parent cannot pass the entry point. The former builds tagged nonvisual hosts; the latter builds production services without unic_e2e and uses only local maintained DERP/STUN. Existing `make posthog-e2e` and `make headless-e2e` remain regression checks, not proof of this new feature. Run documentation validation with:

```sh
go run ./tools/docscheck
```

The checker requires Git metadata. If unavailable, record that failure and perform bounded link/registration checks without claiming full docscheck acceptance. Do not initialize a repository or fabricate verification commits.

## 5. Current receipt

| Layer | State |
|---|---|
| Discussion scope and paste exclusion | Recorded |
| Pinned tailcat API/source inspection | Complete in discussion; recheck before implementation |
| Decision and execution planning | Complete |
| Public Client passive-status dependency | Unresolved; no upstream change performed |
| Runtime changes and new E2E fixtures | Implemented; core and injected fault process suites PASS |
| Local real-stack direct/DERP acceptance | PASS for incoming mapping on production services; outgoing status unsupported |
| Resource and privacy runtime acceptance | Privacy PASS; matched nonrace 0/1/8-session limits PASS; race RSS limit failed at 8; wakeups/cost NOT_RUN |
| Hosted upload/read-back and dashboards | NOT RUN |
| Native clipboard/platform acceptance | NOT RUN |

Documentation-only validation belongs in the task handoff. None of the runtime checkboxes above are complete merely because this plan was written.


## 6. Local implementation receipt (2026-11-10)

### Implemented behavior

- The service injects `syncdiag.Observer` into the built-in plugin. The bounded
  existing metrics queue carries pipeline and sync records in one envelope.
- Network startup, dial and identity handshake produce phase-matched terminal
  counts/durations and finite typed reasons. Pending approval is not session admission.
- Sessions share one terminal close claim. The host marks local service shutdown
  before Engine withdraws active state, preserving local_stop classification.
- Existing metrics workers own one passive ten-second timer. Zero work stops that
  timer; disable waits for the generation and clears session/recovery baselines.
- Incoming mapping matches the accepted virtual IP against the pinned tailcat node
  key ULA representation. Raw status/keys/endpoints never leave tcnet. Outgoing is unknown.
- Recovery has one episode per eligible disconnected peer, regardless of failed
  attempts. Successful recovery, pending samples and abandoned/interrupted ends remain separate.
- Payload instrumentation counts successful body writes/reads, partial failure,
  authorized/admitted size, cached start path and body I/O duration.
- Engine classifies selected stored history and emits accepted remote receipt
  application. The service counts every automatic application in metrics and persists
  daily analytics admission before SDK enqueue. A marker failure suppresses the daily
  event while business sync remains usable.

### Executed evidence

| Check | Result and scope |
|---|---|
| First pre-instrumentation process run | Expected FAIL: connection metric absent. Development changed source during that deliberately red run; no acceptance claim. |
| Core process suite | PASS: connection phases, known payload sizes, daily coalescing, manual remote-history source, fake path transitions, recovery, fresh metrics baseline, restart, UTC rollover and privacy. |
| Injected process faults | PASS: exact 17-byte partial send/read failures, pending recovery during repeated typed dial timeouts, one subsequent recovery, and nonregular daily marker suppressing analytics without blocking sync. |
| Production real-stack paths | PASS: two production services, local maintained DERP/STUN, no application-triggered active probes. Incoming direct was independently checked by delivering new content after closing the only DERP. Forced relay mode exported no direct sample. Both modes retained unknown outgoing coverage. |
| Existing CLI + observability + PostHog regression | PASS, 552.473 seconds. It includes native-free application-result fixtures, generation barriers, exporter faults and release assembly. |
| Production headless acceptance | PASS, 49.763 seconds. The adapter-process subprocess entry is intentionally skipped outside its child invocation. |
| Desktop build | PASS; no GUI launched. |
| Resource comparison, race daemons | 0/1 sessions PASS. At 8, RSS FAIL: 100512 vs 75456 KiB, exceeding the preset +16384 KiB limit. Goroutines, heap and CPU passed. Preserve this failure. |
| Resource comparison, matched nonrace daemons | 0/1/8 sessions PASS with unchanged limits. At 8, both had 51 goroutines and 40208 KiB RSS; heap increase was 504472 bytes; observed CPU was 0.01 vs 0.02 seconds over 11 seconds. These are process observations, not scheduler-wakeup or collector-cost acceptance. |

The resource limits are +8 goroutines, +8 MiB heap, +16 MiB RSS and +0.25 CPU
seconds over eleven seconds relative to unchanged baseline. They were fixed before
resource execution, but after runtime implementation. This deviates from the original
P0 timing requirement. Do not claim the complete P0/P5 resource gate.

### Remaining acceptance gaps

| Boundary | State |
|---|---|
| Maintained symmetric public Client status | BLOCKED_UPSTREAM: latest tag v0.7.0 and checked main lack Client.Status; no fork or private access used |
| Peer-relay, stale/inactive/unmapped reducer inputs and startup/close races | Implementation handles these cases; dedicated real-stack fault acceptance NOT_RUN |
| Invalid/damaged transport credentials, identity/protocol mismatch and observer panic | Finite handling implemented; dedicated process scenarios NOT_RUN |
| Revocation/suspension during pending recovery and crash coverage | Bookkeeping implemented; dedicated injected process scenarios NOT_RUN |
| Exact image/file/multipart/empty/multiple-target metric assertions | Runtime implemented; ordinary CLI content cases pass, dedicated metric assertions NOT_RUN |
| External/action/migration source mapping and all daily failure permutations | Mapping implemented; remote/local operation paths accepted, complete matrix NOT_RUN |
| Sampler scheduling wakeups and isolated collection cost | NOT_RUN; code inspection and worker/goroutine evidence do not replace these measurements |
| Hosted upload/read-back, dashboards/alerts, release publication | NOT_RUN; no hosted write performed |
| Native clipboard and actual paste | Native NOT_RUN; paste excluded by scope |

Do not mark all P0–P6 gates complete or archive the decision. The remaining upstream
and acceptance boundaries are explicit; working sync remains intact.

### Replay and artifacts

Run `make sync-observability-e2e` for core/fault process acceptance. Run
`make sync-path-e2e` for production incoming direct/DERP acceptance. Artifacts live in
`.cache/0020/acceptance` and `.cache/0020/frozen-real-path`; each contains source/binary identity,
sanitized decoded telemetry, assertions, commands, replay entry and SHA256SUMS.
The two path services retain the normal sixty-second export cadence. The driver builds
with `ts_omit_captiveportal,ts_omit_portmapper` to suppress external discovery services.
No unic_e2e transport, test route or clipboard adapter is linked in that driver binary.

Resource comparisons use the unchanged baseline commit above and the same daemon
build mode on each side. `TestSyncObservabilityResources` fails without a baseline
binary. The standard nonrace comparison command is:

```sh
TMPDIR=/private/tmp UNIC_E2E=1 \
UNIC_E2E_BIN="$PWD/.cache/0020/resource-bin" \
UNIC_SYNC_BASELINE_DAEMON="$PWD/.cache/0020/baseline-bin/unicd-resource" \
UNIC_E2E_ARTIFACTS="$PWD/.cache/0020/resources-final" \
go test -race ./e2e/cli -run '^TestSyncObservabilityResources$' -count=1 -v
```

The runner may use race detection independently of the daemon binaries; manifests
record each daemon's actual build settings. Do not treat race-shadow RSS as release
RSS or discard the failed race comparison. No source bundles, binaries, profiles,
clipboard content, raw transport logs or credentials belong in the retained receipt.

Final review additionally bounds status reads to one snapshot per collection and
binds delayed terminal cleanup to its close-time collection epoch. The daily marker
uses one fixed temporary name; a process scenario verifies stale temporary removal.
Analytics-only disable/re-enable verifies private marker removal, identity rotation,
continued operational counts and no disabled-period daily backfill.

### Frozen final-source checks

The final source snapshot passed core/fault E2E in 46.669 seconds, real-stack path
E2E in 125.395 seconds, nonrace-daemon resource comparison in 79.798 seconds,
production headless acceptance in 49.161 seconds and affected pipeline/product
regressions in 4.970 seconds. Desktop build and all-package test compilation passed.
No unit tests were added or executed. The earlier full CLI/observability/PostHog run
is a separate source snapshot; affected checks were repeated after telemetry review.

Final artifact roots are `.cache/0020/acceptance`, `.cache/0020/frozen-real-path`,
`.cache/0020/frozen-resources` and `.cache/0020/frozen-headless`. The consolidated
`.cache/0020/receipt` records commands, outcomes, source/binary identity and recursive
checksums. Earlier development/source-change failures and the race-RSS failure remain
separate from this receipt. `make sync-path-e2e` writes a new replay to `real-path`;
that default replay destination is separate from the frozen receipt directory.

At eight sessions the final matched nonrace comparison observed 51 goroutines on
both sides, 39200 vs 39520 KiB RSS, 3640056 vs 4095824 bytes heap and 0.04 CPU
seconds on both sides over eleven seconds. These measurements meet the unchanged
limits. They do not establish scheduler wakeups or isolated collector cost.
