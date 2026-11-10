# PostHog integration: implementation and acceptance plan

- Status: Implemented and accepted locally and in the selected US Cloud project. Baseline full-suite failures and native acceptance limits are recorded below.
- Date: 2026-11-10.
- Runtime specifications: [observability architecture](../architecture/observability.md) and [observability contract](../contracts/observability-contract.md).
- Baseline: [current observability](../architecture/observability.md).

## 1. Scope, accepted decisions and prerequisites

### 1.1 Accepted decisions

- Upload logs, metrics and product events by default.
- Expose the master switch and independent signal switches in `$UNIC_HOME/config.toml`.
- Apply explicit false before exporter initialization, preserve it across restart, and support hot reload.
- Allow only the project ingestion token in distributed packages. Never distribute administrative/query credentials.
- Use an independent random analytics identifier per profile. Never derive it from device identity.
- Rotate the analytics identifier after analytics is disabled. Do not merge installations across devices.

Do not request these decisions again.
Direct Cloud delivery and delta OTLP export are the planned implementation choices from 0019.
Record any change to those choices in the decision before implementing an alternative.

### 1.2 Remaining external inputs

| Input | Required before | Local progress without it |
|---|---|---|
| Cloud region and development project | Hosted upload and region-specific release settings | Use a local TLS receiver and a synthetic ingestion token. |
| Production project and ingestion token | Production packaging and deployment | Finish all local implementation and acceptance. |
| Metrics enabled in the development project | Hosted metrics acceptance | Verify native OTLP requests and aggregation locally. |
| Separate read credentials | Hosted query read-back | Preserve local decoded request evidence. |

Recheck current PostHog capabilities and the selected Go SDK release at implementation time.
Missing Cloud inputs do not block local implementation when it is authorized.
Do not guess a region or send synthetic acceptance data to a production project.
Do not create projects, dashboards, releases or uploads as part of writing this plan.

### 1.3 Boundaries

Reuse the current OTel log/metric collectors, privacy allowlists and cancellation barriers.
Keep Engine free of PostHog/OTel SDK types and keep `unic`/`unicd` free of UI dependencies.
Do not add replay, traces, crash reporting, feature flags, experiments, disk event spooling or mobile shells.
Preserve unrelated staged UI work and the independent release design.
Never launch a GUI, browser or VM for this plan's nonvisual checks.

## 2. Ordered implementation work packages

Each package defines E2E scenarios before changing runtime behavior.
Do not write unit tests after implementation.
If isolation is essential, record all relevant failure modes before that isolated implementation.
Use sequential packages; a later package must preserve each earlier working path.

### P0. Freeze contracts, source identity and receiver fixtures

- [x] Verify checkout, branch, HEAD, dirty paths and scoped source hashes again. Do not reuse this plan's older source identity as live proof.
- [x] Record the accepted default-on policy in the observability contract, with the pending implementation state explicit.
- [x] Define `telemetry.analytics.enabled`, analytics health state, loss counters and identifier lifecycle.
- [x] Freeze final event names, property enums and per-operation completion semantics from the proposal.
- [x] Extend the existing local HTTPS receiver to decode OTLP and SDK analytics requests, record monotonic timelines and redact credentials before persistence.
- [x] Define tagged fixture operations and controlled receiver delays for initialization, timeout, disable and queue saturation scenarios.
- [x] Make all existing non-PostHog E2E profiles explicitly disable upload. Default-on changes must not make unrelated tests contact Cloud.

Primary locations: `docs/contracts/observability-contract.md`, `docs/contracts/local-service-contract.md`, `e2e/cli/observability_e2e_test.go`, proposed `e2e/cli/posthog_e2e_test.go`, and build-tagged fixtures under `app/service/cmd/unicd/`.

Exit evidence: scenario inventory, sanitized receiver records and fixture source identity. New runtime-dependent cases may be pending; do not label them passed.

### P1. Configuration and a working logs/metrics vertical slice

- [x] Update defaults, strict TOML decoding and generated comments together. Omitted switches resolve to true; explicit false remains false.
- [x] Resolve release defaults before validating the final effective configuration. A developer build without release credentials starts business services and reports safe degraded/unavailable telemetry.
- [x] Keep malformed explicit URLs/configuration as validation failures. Distinguish them from absent release deployment metadata.
- [x] Support a validated `/i` OTLP base path while retaining pathless Collector configuration.
- [x] Pass the selected ingestion token through controlled host assembly. Keep it out of config output, SDK logs, diagnostics and artifacts.
- [x] Include endpoint/project/token selection in one immutable deployment identity. Do not accidentally send another project's token when an endpoint is overridden.
- [x] Preserve restart-required endpoint semantics and generation-safe signal hot reload.
- [x] Prove default-enabled logs and metrics arrive at the local TLS receiver. Prove a pre-existing master false causes zero requests from process start.

Primary locations: `internal/observability/config.go`, `controller.go`, `export.go`, `internal/localconfig/config.go`, `manager.go`, `app/service/cmd/unicd/daemon.go`, `production.go`, and `app/service/internal/host/node.go`.
Release metadata injection must fit existing packaging in `tools/package/main.go`; do not implement decision 0018 as an incidental dependency.

Exit evidence: exact `/i/v1/logs` and `/i/v1/metrics` requests, safe credential-presence assertions, startup/disable timelines and successful ordinary service operations.

### P2. Correct multi-client metric aggregation

- [x] Set delta temporality consistently in the metric reader and exporter on the planned PostHog path.
- [x] Retain the five existing instruments, units, finite attributes and histogram boundaries.
- [x] Add delta-specific acceptance assertions and keep existing cumulative Collector scenarios unchanged apart from explicit default-off fixture settings.
- [x] Run two processes with matching Resource dimensions and known operation counts 3 and 7. Verify combined delta 10 and zero additional increments on idle collection.
- [x] Restart and disable/re-enable one process. Verify no old-generation aggregation is replayed.
- [x] Merge fixed histogram fixtures across processes. Verify count, sum and every bucket before checking percentile tolerance.
- [x] Inject one failed export. Report interval loss and export failure counters without pretending the lost interval was recovered.

Primary locations: `internal/observability/metrics.go`, `export.go`, and process-level observability/PostHog scenarios.

Exit evidence: decoded temporality and histogram buckets, deterministic aggregate assertion report and restart timeline. Cloud query semantics remain a separate gate in P6.

### P3. Analytics lifecycle and the first product event

- [x] Inspect and pin the maintained Go SDK with the Go package manager. Inventory queue, worker, enrichment, transport and Close behavior before choosing settings.
- [x] Implement the smallest host-owned typed analytics adapter. SDK types and arbitrary property maps must not enter Engine APIs.
- [x] Create private profile identity only when analytics becomes effectively enabled. Reuse it across enabled restarts; clear/rotate it on analytics or master disable, including when startup reads false.
- [x] Use atomic private-file operations and the existing single-owner profile lifecycle. Do not expose the identifier through ordinary diagnostics.
- [x] Explicitly disable person processing and GeoIP. Apply a final property allowlist after SDK enrichment.
- [x] Enforce the proposed combined queue/batch/timeout bounds through public SDK settings and a cancellable transport. Do not add a second event queue to hide an unbounded SDK queue.
- [x] Implement disable as discard/cancel/wait. Reserve flush for normal exit within the shared two-second budget.
- [x] Extend safe diagnostics with analytics state/loss counts and expose no raw SDK errors.
- [x] Emit standalone service `app_started` once after successful startup. Verify default-on, pre-start false, hot disable, restart, identity rotation and re-enable.

Primary locations: proposed `internal/observability/analytics.go` and typed event definitions, `controller.go`, `config.go`, `privacy.go`, `internal/localapi/diagnostics.go`, `app/service/internal/server/diagnostics.go`, and daemon assembly.
Add only required SDK dependencies to `go.mod`/`go.sum`.

Exit evidence: one decoded anonymous startup event, identity lifecycle assertions, zero requests after disable acknowledgement and measured bounded shutdown.

### P4. Complete operation events and Desktop ownership

- [x] Define the service as the sole analytics exporter for a profile. Desktop sends only typed lifecycle/action information over the private protocol.
- [x] Keep the analytics identifier inside the service; Desktop does not need the raw identifier to report a typed startup.
- [x] Record one Desktop startup for the pending local process launch key when its adapter becomes ready. Clear the client key after acknowledgement and retain one bounded server key for uncertain retries; heartbeats, reconnects and subscriptions must not add startup events.
- [x] Record `pairing_completed` only when a new trusted relationship commits, never when a request is merely approved or an existing device reconnects.
- [x] Record `history_applied` only after a user-requested history application succeeds. Automatic receive/apply must not emit it.
- [x] Record one `manual_send_completed` per explicit operation after its terminal send outcome. A queued submission is not a completion.
- [x] Record `action_completed` from the Action execution result. A later apply error must not relabel a successfully committed Action as an execution failure; use separate allowlisted apply outcome if needed.
- [x] Tie event admission to existing local operation/request ownership. Avoid duplicate events from IPC retries and multiple targets without retaining business IDs in payloads.
- [x] Add the public finite ProductEvents hook and authoritative trust, Action, terminal-send and explicit-history-apply emission points.

Primary locations: `app/service/internal/host/node.go`, `history.go`, `submissions.go`, `adapter.go`, `internal/localapi/adapter.go`, the adapter server, and the real Desktop startup path under `app/desktop/cmd/unic-desktop/`.
Update `docs/contracts/engine-contract.md` only if a public hook actually changes.
Do not modify the staged Desktop demo merely to make its switches appear connected.

Exit evidence: nonvisual real IPC/adapter and operation scenarios with exact event counts. Native window interaction stays NOT RUN unless separately authorized.

### P5. Fault, privacy and production-binary acceptance

- [x] Complete every applicable scenario in section 3, including initialization/disable races, failed identity persistence, forged properties and SDK enrichment.
- [x] Exercise the production intervals, not only shortened tagged-fixture intervals.
- [x] Run the production headless harness without test tags. Verify service/CLI dependency inventories remain UI-free.
- [x] Measure pending-record saturation and worker cleanup; exercise exit budgets. Existing observability resource fixtures retain their heap/CPU samples. Full retained-byte accounting and native idle wakeup measurement remain unverified; queue length is not an RSS claim.
- [x] Verify synthetic clipboard/path/pairing/search canaries never reach transmitted payloads or retained evidence.
- [x] Generate the integrity manifest and deterministic local replay entry point from section 4.

Exit evidence: local acceptance directory with all scenario states, production binary manifest, checksums and reproducible commands. Any unresolved failure blocks a local acceptance claim.

### P6. Cloud development-project read-back

Prerequisites: selected region, development project token, Metrics enabled and separate read credentials.

- [x] Recheck endpoint/schema support against the selected project before sending fixtures.
- [x] Send synthetic data from production binaries built from the accepted runtime source. The Cloud driver records its own binary hashes; it does not reuse tagged local-fixture binaries.
- [x] Query logs, metrics and analytics in a bounded recorded window. Do not treat a successful HTTP response as query visibility.
- [x] Verify the 3+7 metric aggregate, histogram counts, unique events and absence of person profiles. Capture query definitions and sanitized results.
- [x] Record region/project, source and binary hashes, observation window and independent per-signal outcomes.
- [x] Record versioned query definitions and initial dashboard requirements in this document. Remote dashboard/alert publication is outside this task.

Exit evidence: hosted receipt linked to the local manifest. Missing Cloud inputs leave P6 NOT RUN while preserving completed local work.

### P7. Documentation, packaging and handoff

- [x] Verify release metadata selects US Cloud / UniClipboard and includes only the allowed ingestion token. Both local archives were built; no installation or publication was performed.
- [x] Document the default-enabled behavior and exact master/per-signal disable examples in current runtime documentation and generated configuration comments.
- [x] Update architecture and contracts with the actual implementation; remove stale default-off claims from affected live pages.
- [ ] Run docscheck bumps only after checking the corresponding code. Do not refresh unrelated stale pages merely to suppress warnings.
- [x] Record exact local/hosted/native outcomes and remaining gaps in the receipt. Do not infer Windows/Linux or native acceptance from one macOS run.
- [ ] Review only task-owned changes. Preserve unrelated staged work. Commit/push/PR/release are separate actions when requested.
- [ ] Complete the repository decision landing/archive workflow only when implementation and its stated acceptance are complete.

Exit evidence: reviewable scoped diff, updated contracts, checks passing, immutable receipt and concise operational handoff.

### 2.1 Validation command contract

The following commands are planned implementation checks, not evidence that they ran during document preparation.
The implementation must add the proposed `TestPostHog` suite before using its selector as an acceptance claim.
Fail if a required suite selects zero scenarios or skips due to missing fixture binaries.

```sh
# Build nonvisual, process-level acceptance binaries.
mkdir -p .cache/0019/bin
go build -race -tags unic_e2e -o .cache/0019/bin/unicd ./app/service/cmd/unicd
go build -race -o .cache/0019/bin/unic ./app/service/cmd/unic

# Use absolute paths for UNIC_E2E_BIN and UNIC_E2E_ARTIFACTS.
UNIC_E2E=1 UNIC_E2E_BIN="$PWD/.cache/0019/bin" UNIC_E2E_ARTIFACTS="$PWD/.cache/0019/acceptance" go test -race ./e2e/cli -run '^(TestObservability.*|TestPostHog.*)$' -count=1 -v

# Production binaries, no test tag; preserve its own manifest.
make headless-e2e

go build -o .cache/0019/bin/unic-desktop ./app/desktop/cmd/unic-desktop
go run ./tools/docscheck
git diff --check
```

Run the full existing CLI E2E suite after fixture defaults are updated to prevent Cloud contact.
If txtar cases change, regenerate their catalog with the repository's existing catalog command.
Use one final targeted rerun after the last relevant code change; repeat broader checks only for new failures or changed scope.
The Desktop build does not authorize launching it or establish native acceptance.

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
| Identity persistence failure | No fallback to device identity or a shared catch-all ID; analytics degrades while business service remains usable. |
| Master false read at startup after previous enablement | No request; stale analytics identity is cleared so the next enablement rotates it. |
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

## 6. Phase tracking and current receipt

| Work package | State | Completion evidence |
|---|---|---|
| P0 Contracts and fixtures | Complete | Scenario definitions and source manifest |
| P1 Configuration/logs/metrics slice | Complete | Decoded requests and first-start false timeline |
| P2 Delta metrics | Complete | Multi-process totals, histogram merge and reset assertions |
| P3 Analytics startup slice | Complete | Startup event, identity and disable/exit assertions |
| P4 Operation events/Desktop ownership | Complete (nonvisual) | Real IPC operation counts |
| P5 Local fault/production acceptance | Accepted with baseline failures | Checksummed local artifact and production manifest |
| P6 Cloud read-back | Complete | Per-signal hosted query receipt |
| P7 Documentation/packaging handoff | Closing | Verified live docs and final scoped diff |


| Layer | Status |
|---|---|
| Existing source and contract inspection | Complete |
| Official PostHog logs/metrics/events capability check | Complete; checked 2026-11-10 |
| Execution plan and failure inventory | Implemented; independent living-doc fact-check completed |
| Runtime/configuration/SDK changes | Implemented; posthog-go v1.42.0 |
| Local ingestion E2E | PASS: seven suites / 22 scenarios; production release assembly uses actual 60-second metrics period |
| Cloud project, region, token and Metrics availability | US Cloud / UniClipboard 722858; selected ingestion metadata and metric read-back verified |
| Hosted query read-back | PASS: logs, metrics and anonymous product events |
| Remote dashboards, installation/publication and native GUI/clipboard interaction | NOT RUN / outside this task |

Documentation validation results belong in the task progress record.


## 7. Completed acceptance receipt (2026-11-10)

### Local verification

- `make posthog-e2e`: PASS, seven suites and 22 scenarios. The complete run with Desktop reconnect acceptance completed in 89.572 seconds. The final acknowledgement-clear refinement passed targeted IPC acceptance; missing launch-key registration was also checked separately.
- The untagged production-release scenario passed pre-start false, normal 60-second metrics export, and exact endpoint/token binding.
- `TestObservabilityPipeline`: PASS after replacing its prematurely fatal clipboard polling predicate with a bounded API read.
- The full CLI run completed all txtar cases, existing resource/fault scenarios, and the PostHog suites. Its only failure was that polling issue; the corrected case passed separately.
- `make headless-e2e`: PASS on the final runtime source (47.07-second production scenario), including UI-free dependency checks and real adapter IPC.
- Desktop build: PASS. No graphical application was launched.
- `go test -race ./...`: not green. Default macOS TMPDIR first triggered the existing cache symlink guard. With `TMPDIR=/private/tmp`, the same 22 Engine/host/migration tests failed both at untouched baseline `620dc21485407e2f097937ffdb194df037dfbf56` and with this integration. No additional failing names were found.
- Existing Desktop test packages passed. Do not represent the unchanged baseline failures as passing tests.

The baseline comparison is saved in `.cache/0019/baseline-comparison.json`.
The local PostHog artifacts are under `.cache/0019/acceptance/`, with decoded data, request timelines, assertions, checksums and per-suite replay scripts.
`.cache/0015-acceptance/manifest.json` identifies the production headless run.
Manifests identify exact tested source/binary hashes. A final review refined callback-lock wording and added explicit Desktop reconnect deduplication. Updated IPC and production acceptance identify the final source. Cloud ingestion was repeated after the reconnect refinement, and its independent final-source receipt is listed below.

### Cloud verification

Selected project: [UniClipboard 722858](https://us.posthog.com/project/722858), US Cloud.
The authenticated connector was explicitly pinned to that project before data queries.
The runner used two isolated production processes, no test tags, sync disabled and default-enabled telemetry.
No live user profile or system clipboard was accessed.

The final-source query window is **2026-11-10 03:37:47–03:39:06 UTC** (11:37:47–11:39:06 Asia/Shanghai). An earlier independent window is retained under `.cache/0019/cloud/`.

| Assertion | Actual | Result |
|---|---|---|
| Startup events | 2 | PASS |
| Manual submission terminal events | 10, all skipped because sync was explicitly disabled | PASS |
| Person processing / GeoIP | All fixture events propertyless; person processing false; GeoIP disabled | PASS |
| New person profiles | 0 | PASS |
| Sanitized logs | 2 config.rejected warnings; only the four allowed Resource keys | PASS |
| Committed operation metric | Delta total 10 (3 + 7) | PASS |
| Commit duration histogram | Sample counts 3 and 7; each bucket array sums to its sample count | PASS |
| Exporter state | Both processes reported all three active, zero export failures and zero drops | PASS |

Current Cloud storage separates metric series metadata from sample rows.
Inspect `system.information_schema` before querying; do not assume point attributes are inline fields on `posthog.metrics`.
The exact series fingerprints and SQL used for these fixtures are in the sanitized read-back artifact.
This is fixture acceptance, not a governed production usage measure.

Cloud evidence:

- `.cache/0019/cloud-final/upload-receipt.json`: source/binary hashes, time window and local exporter states.
- `.cache/0019/cloud-final/readback.json`: exact SQL and sanitized raw results.
- `.cache/0019/cloud-final/hosted-receipt.json`: asserted expected/actual results and project identity.
- `.cache/0019/cloud-final/SHA256SUMS`: evidence integrity.

Reproduce only when an actual Cloud upload is intended:

```sh
# This private, ignored file was provisioned from the selected project's token.
. .cache/0019/release.env
TMPDIR=/private/tmp go run ./e2e/posthogcloud --artifact .cache/0019/cloud
```

The driver leaves read_back=PENDING until an independent query receipt is produced.
A later run has a new time window; never reuse this run's query result as proof of the later upload.
The token is absent from source, query receipts and ordinary configuration output.

### Packaging and handoff

`tools/package -desktop` produced local headless and Desktop archives for darwin/arm64 using the selected US deployment metadata.
The manifest reports source_unchanged=true, the selected ingestion host, ingestion_token_configured=true, and signed=false.
The Desktop archive reuses the exact same unic/unicd payloads as the headless archive.
Packages are under `.cache/0019/packages/`; they were not installed, signed or published.
Only the ingestion token may be extractable from those binaries, as explicitly accepted by the user.

Remaining independent work: native OS/clipboard interaction, Windows/Linux runtime acceptance, signed publication, dashboard/alert publication, and the repository's unchanged baseline test failures.
