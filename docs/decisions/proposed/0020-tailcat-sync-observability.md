# 0020: Tailcat sync reliability, performance and product value

- Status: Design adopted; core runtime implemented and locally accepted. Symmetric passive Client status remains blocked upstream; hosted and native acceptance are separate.
- Date: 2026-11-10
- Execution plan: [0020 implementation plan](../../exec/0020-tailcat-sync-observability.md).
- Current behavior: [observability architecture](../../architecture/observability.md).
- Existing specifications: [observability contract](../../contracts/observability-contract.md), [sync backend contract](../../contracts/sync-backend-contract.md), [Engine contract](../../contracts/engine-contract.md), and [history contract](../../contracts/history-contract.md).
- Intended landing: the existing observability architecture and observability contract; update backend and Engine contracts only where their public interfaces change.

## 1. Problem and accepted scope

Existing pipeline diagnostics describe queue departure, local send completion, receive completion and clipboard application. They do not explain tailcat connection establishment, connection loss, recovery or the selected network path.

Measure all four questions:

1. Why does a connection fail?
2. Why does a session disconnect repeatedly?
3. Where does sync spend time?
4. Does remote content become available, and do users deliberately retrieve it?

The user explicitly excluded actual paste tracking. Do not add global input monitoring, accessibility observation, pasteboard read detection, lazy-provider read callbacks or inferred paste events for this feature. Successful clipboard application and deliberate history retrieval are distinct value signals. Neither proves that another application consumed the content.

This document proposes runtime work. It does not establish implementation, network acceptance, hosted ingestion or dashboard publication.

## 2. Verified starting point

Source inspection on 2026-11-10 found:

- The module pins `github.com/tailscale/tailcat v0.7.0` and a specific `tailscale.com` revision in `go.mod`.
- `internal/tcnet/tcnet.go` wraps tailcat Server and Client. It exposes no path observation to the session layer.
- `internal/plugins/tailcat/tailcat.go` discards raw tailcat logs because they can contain secret addresses. Preserve this protection.
- `internal/session/engine.go` owns dialing, handshake, session replacement, receive loops and reconnect backoff.
- Engine emits finite diagnostics through `Options.Diagnostics`. The service owns bounded OTLP export and PostHog delivery.
- `engine.ProductEvent` and the final analytics allowlist do not currently carry history source classification. `history_applied` does not distinguish remote history.
- Current public diagnostic records do not carry connection stages, path snapshots or byte measurements.

The [tailcat v0.7.0 source](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go) provides these interfaces:

| Interface | Evidence | Limitation |
|---|---|---|
| `Client.Ping(ctx)` | DERP meow/meowed round-trip time | Always measures DERP; cannot classify the business path |
| `Client.DiscoPing(ctx)` | Probe reply endpoint or DERP region and latency | Actively encourages direct discovery; not passive observation |
| `Server.Status()` | Peer status containing direct-path and relay fields | Server side only; snapshot, not a per-transfer route receipt |

There is no exported `Client.Status()` in this pinned version. The internal backend already builds status, but private access is not an acceptable application interface.

The pinned Tailscale `populatePeerStatus` implementation sets `Relay` even when a direct path is selected. `CurAddr`, `PeerRelay`, activity and freshness must be interpreted together. A configured relay is not evidence that payload used DERP. The [upstream endpoint implementation](https://github.com/tailscale/tailscale/blob/a2263542f260/wgengine/magicsock/endpoint.go) is the reference for this revision.

## 3. Decisions and ownership

### 3.1 Preserve the existing telemetry architecture

- Reuse the current service-owned exporters, signal switches, independent log levels, privacy filters, cardinality limits, loss counters and shutdown budget.
- Keep telemetry admission nonblocking. Never perform network or disk I/O on a business callback.
- Use finite typed records. Do not add arbitrary property maps or a generic telemetry plugin framework.
- Collect observations where the transport/session/business decision is made. Do not infer results from GUI or SSE replay.
- Keep tailcat and OTel implementation types out of public Engine options and generic plugin DTOs.
- Start with the built-in tailcat backend. Do not extend external plugin RPC solely to support an unimplemented observation capability.
- Wire transport/session observations to the existing service controller through a small typed callback at built-in plugin assembly. Define the record at the lowest shared boundary that avoids a dependency cycle. Engine remains the owner of pipeline and product-result records.
- Call observers outside state locks. Isolate observer panic and telemetry failure from connection and sync results.

No new user configuration is proposed. Collection follows existing telemetry and log switches. Sampling intervals and finite catalogs are implementation constants defined in the contract before coding.

### 3.2 Obtain symmetric passive status through a maintained interface

Prefer an upstream public Client status method with the same snapshot semantics as Server status. Pin an immutable released revision containing it. Verify nil/start/close behavior and concurrent access before integration.

Do not use reflection, `unsafe`, private field access, log parsing or a silent long-lived fork. If upstream delivery is unavailable, record the passive client-side gap as blocked. Keep partial work runnable and label unsupported paths `unknown`; do not claim symmetric passive acceptance.

Use `DiscoPing` only in an explicit bounded diagnostic experiment. Keep its results separate from passive population metrics. Do not add periodic active probes to production in this scope.

### 3.3 Observe path selection without overstating precision

Use the finite path categories `direct`, `derp`, `peer_relay` and `unknown`. Preserve `peer_relay` as a separate category if the library reports it; do not merge it with DERP.

- Require a live matching session and a valid fresh status snapshot. The private mapping from transport peer to session must be verified for incoming and outgoing connections.
- A valid direct endpoint takes precedence over a configured relay. Never export the endpoint.
- Classify a selected DERP fallback only when the pinned status implementation and snapshot freshness establish it. A relay field alone is insufficient.
- Inactive, missing, stale, unmapped or unsupported state is `unknown`.
- An empty snapshot is not a disconnection notification. Session lifecycle remains authoritative.
- Sample only existing eligible sessions. Use one bounded service worker with a proposed 10-second cadence, plus on-demand snapshots at existing transfer boundaries. This is a proposed default, subject to resource acceptance.
- Never create a client, dial or trigger discovery solely to observe a path. Do not turn startup or reconnect into a wait for telemetry.
- Count transitions only between consecutive fresh, known samples in one session generation. An unknown gap breaks continuity.
- Report DERP-to-direct delay as a sampled observation with up to one sampling interval of timing uncertainty. Exclude sessions first observed as direct. Report lost/unknown observations separately.

Path sample share is not byte share or exact time share. A transfer-start snapshot is not proof of every packet's route. Classify a transfer summary as `changed` only when a change was observed during it; otherwise name the field `start_path`, not `transfer_path`.

Exact direct-versus-DERP byte accounting and exact path residence time are outside this scope. Ordinary payload byte measurements remain in scope.

## 4. Measurement catalog

The final types, labels, boundaries and invalid-record behavior are in section 11 of the observability contract. The following catalog records the adopted semantics. Avoid duplicating existing `unic.pipeline.*` counts.

| Proposed instrument | Type / unit | Unit of observation and dimensions |
|---|---|---|
| `unic.sync.connection.attempts` | Counter / `{attempt}` | Once per completed network-start, dial or identity-handshake attempt; `phase`, `direction`, `result` |
| `unic.sync.connection.duration` | Histogram / `s` | Same attempts; `phase`, `direction`, `result` |
| `unic.sync.connection.errors` | Counter / `{error}` | Failed attempts only; `phase`, finite `reason` |
| `unic.sync.session.ends` | Counter / `{session}` | Once per admitted session end; finite `reason`, `expected` |
| `unic.sync.session.duration` | Histogram / `s` | Admitted session start to terminal end; `expected` |
| `unic.sync.session.observed_time` | Counter / `s` | Sum of monotonic connected session time observed inside enabled collection windows; no peer label |
| `unic.sync.recovery.ends` | Counter / `{episode}` | Once per recovery episode end; `result` = recovered, abandoned or interrupted |
| `unic.sync.recovery.duration` | Histogram / `s` | Recovered episodes only; do not include abandonment as success |
| `unic.sync.recovery.pending` | Histogram / `s` | Ages of currently pending recovery episodes at each collection; interval sample distribution, not terminal recovery time |
| `unic.sync.path.samples` | Counter / `{sample}` | Eligible session snapshots; `direction`, `path` |
| `unic.sync.path.transitions` | Counter / `{transition}` | Consecutive fresh known observations; `from`, `to` |
| `unic.sync.path.upgrade_duration` | Histogram / `s` | First known DERP sample to first subsequent direct sample in the same uninterrupted observation sequence |
| `unic.sync.transfer.bytes` | Counter / `By` | Actual payload bytes passed through the session body reader/writer; `direction`, `result` |
| `unic.sync.transfer.size` | Histogram / `By` | Declared authorized/admitted payload size once per transfer; `direction`, `content_kind` |
| `unic.sync.transfer.duration` | Histogram / `s` | First payload I/O to body completion/failure; `direction`, `result`, `start_path`, `size_class` |
| `unic.sync.remote_applications` | Counter / `{application}` | Each accepted automatic remote-device receipt application; no labels |

Proposed size classes are `empty`, `le_64k`, `le_1m`, `le_16m`, `gt_16m`, using binary byte boundaries. Unknown size is `unknown`. No raw size, error or identifier becomes a label.

Connection phases are `network_start`, `dial`, and `identity_handshake`. Accepted-side handshake attempts have direction `incoming`; dials have direction `outgoing`; process-wide network start uses `none`. Preserve failed network start even if no session is created.

Use finite reasons supported by typed errors and authoritative local decisions: timeout, cancellation, unavailable, invalid address, unusable credentials, identity mismatch, protocol mismatch, permission rejection, remote close, I/O failure, local stop, suspension, revocation, state replacement and unknown. Do not inspect free-form error strings. A timeout is not proof of NAT failure, remote downtime or DERP failure. DERP map fetch and DERP connection failures remain indistinguishable unless a maintained typed interface exposes them.

Separate expected session ends: local stop, suspension, revocation and deterministic duplicate replacement. A remote EOF is an observed remote close; its cause is unknown unless the protocol supplies an explicit reason. Concurrent terminal paths share one terminal claim.

### 4.1 Recovery semantics and denominators

A recovery episode begins when an admitted session ends unexpectedly while that peer remains eligible for reconnection. It ends when a replacement authorized session is admitted, eligibility ends, or the process/collection generation stops. Repeated failed attempts do not create additional episodes. A first-ever connection attempt is not recovery. Recovering through an incoming connection also closes the episode.

Use private existing session ownership for matching; export no peer/session identifiers. Do not persist a new recovery ledger across process restart. Restart and collection shutdown terminate local episodes as interrupted, rather than fabricating recovery durations. Lossy export can still omit the terminal observation.

Pair successful recovery percentiles with pending-age samples and abandoned/interrupted counts. Never report successful recovery duration as the entire outage population.

Define these queries:

- Dial failure rate: failed completed dials / all completed dials. Report cancellations separately.
- Handshake failure rate: failed completed handshakes / all completed handshakes. Do not divide by dial attempts.
- Unexpected disconnect frequency: unexpected admitted-session ends / observed connected session hours. This is session exposure, not installation uptime.
- Passive direct share: direct samples / known path samples. Also show unknown samples / all samples and the sampling cadence. Do not call this bandwidth share.
- Pipeline latency: retain existing stage-matched distributions. Commit, processing and receive work may overlap; do not add their percentiles.

### 4.2 Performance interpretation

Use monotonic clocks within each process. Keep existing queue timing and whole-call send/receive timing. Add payload timing to distinguish body transfer from control/authorization and storage work. Empty payloads are counted but excluded from throughput calculations.

Count partial bytes on failed transfers once. These are application payload bytes, not WireGuard/DERP traffic, retransmissions or billable network bytes. Distinguish successful validated receive from bytes read before validation. One sender with two targets produces two transfers, not two unique user submissions.

No cross-device trace propagation, remote ACK or protocol change is proposed. Sender success continues to mean local send completion. Receiver success means local commit/publication. Apply success means the host reported clipboard application. None proves paste. Cross-device wall-clock subtraction is not an accepted latency measurement.

## 5. Product value and ROI

Keep operational volume in metrics. Keep installation reach and repeat use in product analytics.

| Proposed event change | Authoritative point | Meaning |
|---|---|---|
| Add `remote_content_applied` | Successful automatic application of remote-device content, after accepted `ReportApplyResult` | Remote content became available on the clipboard |
| Extend `history_applied` with `source_kind` | Successful explicit history application, classified from the selected history record | Deliberate retrieval of local or remote history |

Use a finite `source_kind`: `local`, `remote`, `external`, `action`, `migration`, `unknown`, mapped from the actual stored origin without exporting device identity. Do not classify a record as remote merely because a backend is enabled. Remote history retrieved manually must not also emit the automatic-application event.

Emit at most one `remote_content_applied` event per analytics identifier per UTC day. Count every valid automatic application through metrics. The service analytics worker owns the daily admission marker and a small private persistence record, outside business callbacks. Bind the marker to the enabled analytics identity epoch; clear it on identity rotation. Persist no receipt, history ID, content hash or source-device identity. Load state before admitting these events after restart. Specify mark/enqueue ordering and crash limitations before coding; exactly-once Cloud delivery is not promised. Do not backfill disabled periods or failed queued delivery.

Keep explicit `history_applied` events at their existing operation frequency. Add source classification without adding a second event for the same operation. IPC retries, duplicate result reports and reconnects must not increase counts. Reuse the existing anonymous installation identifier; do not attach it to metrics/logs or merge sender and receiver identities.

Initial value queries:

1. Daily installations with at least one remote automatic application.
2. Daily installations with at least one successful explicit remote-history application.
3. Repeat use among a fixed UTC-day cohort on D1 and D7, separately for the two signals. Require the observation window to have elapsed.
4. Remote-history retrieval reach among installations with observed remote availability in the same window. Label missing history-only recipients and telemetry loss as coverage limits; do not present this as a complete conversion funnel.

These measure availability and deliberate use. Frequent automatic overwrites can increase volume without increasing value. Do not claim people counts, pasted items, time saved or monetary ROI from these signals.

Implementation priority reflects expected engineering ROI:

| Work | Cost assessment | Expected benefit |
|---|---|---|
| Typed connection/session diagnostics | Medium | Separates failure stages and identifies unstable sessions |
| Symmetric passive path status | Medium; upstream delivery dependency | Explains path distribution without probing bias |
| Payload size and timing | Medium | Separates large-content effects from control and queue delays |
| Automatic availability and remote-history source | Low to medium | Establishes reach and deliberate repeated use |
| Actual paste observation | Excluded by user | No implementation or exploratory instrumentation |

These are relative estimates, not delivery dates. Reassess sampling, series volume and event volume using process acceptance before rollout. Do not add a new queue or per-peer durable telemetry store.

## 6. Privacy, resource and delivery constraints

- Export only finite categories, aggregate counts, durations and byte measurements. No raw clipboard content, MIME extensions, content hash, file path/name, device/public key, address, pairing material or connection/transfer ID.
- Keep raw tailcat logging disabled. Add typed observations before errors become arbitrary strings.
- Status objects contain identities and endpoints. Reduce them inside the transport boundary. Never enqueue or serialize the full object.
- Reuse the existing 1024-series cap per metric instrument. Enumerate reachable label combinations before implementation; reduce dimensions if the catalog exceeds that cap. No unbounded per-peer instruments.
- Reuse current metrics collection/export cadence and exporter queues. A path sampler is separate from export and stops when no applicable observation is enabled. Disabling analytics alone must not stop operational metrics.
- Stop workers at the existing generation barrier. Re-enable starts fresh baselines; disabled-period bytes, time and episodes are not replayed.
- Repeated reconnect failures update counters. Rate-limit repetitive state logs through the existing bounded logging path; do not emit an analytics event per retry.
- Telemetry remains lossy. Collection coverage and exporter losses accompany all reliability and value interpretations.

## 7. Alternatives and implementation boundary

Rejected alternatives are raw-log ingestion, frequent DiscoPing as population measurement, configured DERP as route proof, per-packet instrumentation, global input monitoring and cross-device analytics identity linking.

The first vertical slice must connect typed connection diagnostics and passive status through the production service to decoded OTLP in a real-process E2E artifact. Keep existing sync runnable while the upstream Client status dependency is resolved. Then add recovery, payload timing and value signals in that order.

Runtime implementation, upstream contribution, hosted fixture upload and remote dashboard publication are separate actions. This document authorizes none of them by itself. The [execution plan](../../exec/0020-tailcat-sync-observability.md) defines the required evidence before any completion claim.


## 8. Implementation boundary and local evidence

The built-in observer is `internal/syncdiag.Observer`; service assembly installs it
without changing external plugin RPC. The metrics worker owns the passive timer.
The [execution receipt](../../exec/0020-tailcat-sync-observability.md) distinguishes
loopback process evidence, production real-stack path evidence, resource comparisons
and unrun failure scenarios. Incoming direct and controlled DERP classifications have
local production-process evidence. Outgoing passive status remains unknown because
neither v0.7.0 nor upstream main exposes Client.Status as of 2026-11-10. No dependency
fork or active probe substitutes for that gap. Keep this decision proposed until the
remaining adoption/acceptance boundary is resolved; do not archive it as fully landed.
