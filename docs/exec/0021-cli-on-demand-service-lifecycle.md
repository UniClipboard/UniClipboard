# 0021: On-demand CLI service lifecycle implementation plan

- Status: Planning complete; design adoption and implementation pending. Runtime acceptance NOT_RUN.
- Date: 2026-11-10
- Decision: [0021 on-demand CLI service lifecycle](../decisions/proposed/0021-cli-on-demand-service-lifecycle.md).
- Living references: [headless architecture](../architecture/headless.md), [local service contract](../contracts/local-service-contract.md), [Engine contract](../contracts/engine-contract.md), [pipeline contract](../contracts/pipeline-contract.md) and [observability contract](../contracts/observability-contract.md).
- Planning checkout: branch `v2`, HEAD `516c9f951cf45b2d1203748f27b0e887d2a2f215`. The 0021 decision and its registration changes already existed in the working tree and must be preserved.

## 1. Scope and evidence boundaries

Deliver leaf-command dependency declarations, automatic service acquisition, shared temporary usage, last-user shutdown and command-specific completion. Preserve existing standalone/desktop ownership, offline migration exclusivity, clipboard capability checks and the absence of UI dependencies in CLI/service.

Do not implement a start-call-stop wrapper that treats an HTTP response as completion. Do not launch a GUI, create a second Engine in CLI, open the live history database from CLI, or introduce a compatibility layer, idle-retention configuration or durable lease database.

This plan is not permission to implement the unresolved foreground-duration and send-completion proposals. P0 records their adoption and exact contracts before runtime code. The current task writes documentation only. No startup, native interaction, publication, hosted telemetry upload or runtime result is implied.

Use real-process E2E as the test mechanism. Write the failure inventory and executable scenarios before runtime code. Do not add unit tests after implementation. Keep fault controls in fixture processes or build-tagged fixtures, outside production public interfaces.

Recheck repository, branch, worktree, dependency identities and dirty state at implementation start. Freeze a source snapshot and establish a baseline before changing behavior. Do not treat a different checkout or existing unrelated failure as this feature's result.

Concurrent migration-related changes appeared after the initial documentation snapshot. Preserve that work. P0 must resolve the actual shipped migration entry point and packaging from its implementation snapshot; the offline-exclusive target requirement does not depend on retaining the historical helper layout or reintroducing a removed wrapper.

## 2. Work sequence

### P0. Adopt semantics, freeze contracts and prepare process scenarios

- [ ] Read the decision and living contracts against the implementation checkout. Audit current command dispatch, desktop startup and existing E2E entry points. Open: no record of this review.
- [x] Record adoption of the decision's send-completion, invitation-window and temporary log-level behavior. These change current CLI completion; writing this plan does not mark them accepted.
- [x] Produce an exhaustive leaf/alias/flag inventory. For each valid invocation, specify service requirement, adapter capability, completion condition, stdout/stderr format and exit mapping. Include help, invalid syntax and migration's actual target directory.
- [x] Select the maintained live lease mechanism. Specify acquisition, initial owner handoff, disconnect detection, release, admission closure and process-loss behavior. A deferred CLI cleanup or durable reference counter alone is insufficient.
- [x] Specify standalone, desktop-owned and CLI-temporary metadata; generation/profile validation; finite lease/pending-acquisition limits; lifecycle states; and exact local API DTOs. Keep protocol version 1 and change the unreleased contract directly.
- [x] Define the atomic last-release/new-acquire boundary. Specify how a client recognizes a retiring generation before business execution and how old cleanup is prevented from touching a replacement.
- [ ] Specify command-originated operation observation and final outcomes. Re-reading cached submission admission is not a network terminal query. Define no-target, timeout, cancellation, partial-send and uncertain-write behavior without replaying a mutation. Open: C23 (partial send, slow file) is NOT_RUN. A cold-start send with one offline and one online paired device delivers nothing (see receipt, defect 4).
- [x] Freeze startup, probe, acquisition, operation and cleanup budgets. Exempt intentional sessions from a blanket one-shot deadline; use authoritative invitation/override expiry.
- [x] Implement fixture drivers and failure scenarios from section 4. Add a repository-standard entry point only when its suite exists. Require it to fail on zero selected cases, skipped prerequisites, missing binaries or failed child processes.
- [ ] Build an immutable baseline and record existing results and resource measurements. Set numeric performance/resource thresholds before candidate runtime work. Do not change pass thresholds after seeing candidate results. Open: thresholds were fixed after the implementation existed.

Exit evidence: adopted completion table, exact lifecycle/operation contracts, executable failure scenarios, baseline source/binary identity, and preset budgets/thresholds. This is the gate for P1; missing semantics are not an implementation detail to choose silently later.

Baseline values to recheck, not newly approved defaults:

| Existing reference | Current source behavior | P0 obligation |
|---|---|---|
| Desktop startup/readiness | 30-second startup budget; one-second probes | Decide the shared acquisition budget and whether readiness includes lease acknowledgement. |
| Directory-in-use retry | Typed exit 75; 100-millisecond retry inside the startup deadline | Preserve bounded retry; never parse free-form error strings. |
| Desktop child cleanup | 12-second graceful wait, then a five-second forced cleanup wait | Define temporary-service cleanup and last-user completion within a bounded budget. |
| One-shot CLI operations | Common 60-second deadlines | Separate startup time from business execution and cleanup. Do not apply this blindly to watch or validity windows. |
| Temporary log override | Existing 1..3600 whole-second range | Retain authoritative expiry/replacement; do not invent a separate CLI retention period. |
| Connection opportunity, operation termination and lease capacity | No new 0021 contract exists | Freeze exact finite rules, error categories and limits before coding. |

### P1. Implement the temporary-service vertical slice

- [x] Extract only reusable process discovery, sibling resolution, profile/generation validation and readiness primitives into a shared internal module. Do not import desktop-internal startup policy into CLI.
- [x] Add CLI-temporary lifetime and authoritative usage admission in unicd, while preserving existing standalone and desktop owner behavior.
- [x] Hold initial ownership from child creation through readiness and first lease admission. Preserve the command's stdin and keep startup output out of command stdout.
- [x] Use a short profile-scoped transition lock shared with desktop startup. Do not serialize complete invocations under it.
- [x] Add atomic last-user retirement and generation-matched cleanup. Detect dead/disconnected clients through the chosen live mechanism.
- [x] Integrate a single finite command, `unic status`, through the real acquisition path. It must start a missing service, obtain status, release usage and leave no temporary child, socket or profile lock.
- [x] Verify warm standalone and desktop-owned reuse; neither may be stopped after this command.
- [x] Verify two overlapping clients at this stage. The starter may finish while the other continues, with exactly one temporary generation.

Exit evidence: real CLI/unicd processes prove cold status, both persistent reuse modes, shared temporary ownership and last-user cleanup. No new command should be advertised as supported until its own declaration/completion path is wired. Preserve existing manual service use while expanding coverage.

### P2. Integrate the complete command requirement catalog

- [x] Route every top-level dispatch path through requirement resolution, including history, plugin and migration entry points. Resolve aliases to the same leaf declaration.
- [x] Validate arguments before service acquisition. Verify help and invalid syntax cannot start a daemon or alter an existing owner's lifetime.
- [x] Generate dependency badges/help descriptions from the authoritative declaration. Fail coverage when a leaf or conditional flag lacks an explicit requirement. Badges come from the declaration; C02 checks both directions between help and the declared top-level commands.
- [x] Keep local operations local: log export, config path, plugin file operations and help. Do not add service probes to them merely to discover ownership.
- [x] Add finite service queries, history/received streams and local-state mutations, retaining leases until responses/streams and command-owned outputs are complete.
- [x] Preserve migration locking for the actual target profile. A running target owner produces an in-use result; no service is started or stopped. C19b uses a generated v1 source: a running owner blocks migration with `in_use` and is untouched; the same command then succeeds on a free profile.
- [x] Check desktop adapter capability after service acquisition for clipboard commands. Preserve stable capability errors and never start a GUI or substitute history for the clipboard.

Exit evidence: a machine-readable catalog-to-process matrix covers every supported leaf, alias, help path, invalid invocation and conditional adapter dependency. Service creation/reuse/retirement assertions accompany command output assertions.

### P3. Complete sends and session-oriented commands

- [x] Add the adopted finite connection opportunity for eligible peers before the single dispatch decision. Startup readiness is not peer reachability; offline/pending peers cannot cause an indefinite wait.
- [x] Submit content exactly once and observe only the command's operation through the contracted local interface. Keep local commit, network terminal state and clipboard application separate.
- [x] Apply the adopted send-completion rule consistently to borrowed and newly started services. Stop a temporary service only after this command's operation has terminated or its timeout/cancellation contract has resolved. `warm-send` shows the same rule on a borrowed service, which stays running.
- [ ] Retain file/content resources until terminal handling. On deadline, do not imply that a referenced file is no longer in use until the contracted cancellation/resource-release barrier has completed. Open: C23 is NOT_RUN.
- [x] Preserve valid no-target, disabled-sync, no-backend and policy-filter outcomes. Do not convert a committed item into an uncommitted failure or fabricate network success.
- [x] Retain watch usage for the event stream. A disconnected or cancelled watch releases its own usage without stopping another client.
- [x] Implement bounded pairing approval sessions and distinguish confirmed, pending, expired, replaced and interrupted outcomes. Process-tested: approved, pending after the window, interrupted, and request removed. Removed and replaced share one outcome in the code.
- [x] Keep invitation and log-level windows available under a temporary service. Preserve the adopted persistent-owner setter/publication behavior. Describe foreground duration in help and stderr without contaminating stdout.
- [x] Track each invocation's authoritative invitation/override generation. Exiting an older command cannot withdraw a newer client's state.
- [x] Verify stdin pipelines and scripts using a deliberately persistent service for invitation capture. A printed value is not proof of future temporary-listener availability.

Exit evidence: delayed real sends and session windows complete against the same production lifecycle path; exact local/network/apply outcomes and user-visible waiting behavior are asserted. No remote ACK, cross-device trace, actual paste observation or wire-protocol change is introduced.

### P4. Close ownership, crash and desktop interaction races

- [x] Exercise last-release versus new acquisition, including a generation already marked retiring. Retry control-plane acquisition only before mutation admission.
- [x] Cover CLI death before readiness, after first admission, during a query, while sending and while holding a session. Confirm live usage is reclaimed without deferred CLI cleanup. C08 (before readiness), C09 (stream and bounded session, after admission), `cold-start-send-killed` (while sending). A finite query is too short to hold open, so the event stream stands in for it.
- [x] Prove the original starter can terminate while a later borrower remains. The child must outlive the starter when that borrower still holds valid usage.
- [x] Preserve normal desktop's explicit replacement policy under the shared transition lock. Old command cleanup cannot stop the replacement generation. C16 and C17.
- [x] Keep lightweight startup and direct standalone/migration conflicts explicit. Do not promote a temporary generation into a persistent owner implicitly. C18, C18b (in `desktop-startup-e2e`) and C19b.
- [x] Verify wrong-profile endpoints, unknown listeners, permission errors and startup configuration errors never become a kill/unlink fallback.
- [x] Verify cancellation and cleanup use independent contexts and bounded waits. Report cleanup failure separately from already committed business results.
- [x] Preserve request-id uncertainty rules. A restart invalidates transient request/operation records; do not promise durable lookup or automatically replay an uncertain write. `cold-start-send-response-lost`: a killed CLI leaves one commit, nothing is replayed, and a lookup after restart returns `no such submission`. A service replaced mid-submit is not exercised.

Exit evidence: reproducible barrier-driven race/fault scenarios prove exactly which generation survives, which operation was admitted and which leases/resources were reclaimed. Sleeps alone are not race synchronization or proof of cleanup.

### P5. Resource, regression and documentation closure

- [ ] Run the complete section 4 matrix against one frozen source/binary set. Record every required scenario as PASS, FAIL or NOT_RUN. Open: every case has a recorded result, but the source is uncommitted and six cases are asserted by other suites with other binaries.
- [ ] Compare cold acquisition, warm reuse, last-user exit and repeated invocation cost against P0's baseline and preset limits. Measure concurrent client counts, retained workers/FDs and post-exit residue. Match build flags on both sides. Open: thresholds were set after the implementation existed.
- [x] Verify bounded lease/admission capacity and recovery after capacity rejection. No durable counter/file grows per invocation; no idle timer preserves a zero-user service.
- [x] Verify existing analytics identity persists across enabled temporary restarts and startup events remain process starts. Do not introduce lease identifiers into telemetry or reinterpret starts as installations. `TestPostHogTemporaryRestarts`: three temporary services give three process starts with one identity; a disabled profile sends nothing and creates no identity.
- [x] Audit then run existing relevant process regressions once after final runtime changes. Rerun only failures or checks affected by later changes.
- [x] Run production headless build/dependency acceptance, CLI/service builds, applicable target compile checks, and Desktop build. Native startup/clipboard/login acceptance remains separate and requires explicit authorization. Headless build, cross-compile for linux/amd64, linux/arm64, windows/amd64 and darwin/arm64, no UI dependency in `unic` or `unicd`, and the Desktop build. No native acceptance was run.
- [ ] Update living architecture, local service/operation contracts, CLI help and application README/AGENTS only for implemented behavior. Run docscheck and bump reviewed architecture metadata against committed source. Open: the source is not committed, so docscheck metadata is not bumped.
- [ ] Record remaining platform/native gaps. Archive 0021 only after adopted behavior is implemented, landed in living documentation and its required process gates pass. Open: waits for the items above and a committed source state.

Exit evidence: checksummed local receipt, committed-source match, expected resource bounds and regression/build results. A test fixture, compile check or documentation pass cannot substitute for a later acceptance layer.

## 3. Implementation seams and regression routing

| Area | Planned responsibility | Required evidence |
|---|---|---|
| `app/service/cmd/unic` | Leaf parsing, requirement declaration, help badges, operation/session completion and output/exit semantics | Real command catalog coverage; local/no-start, stdin/stdout, finite/session completion. |
| `internal/servicelifecycle` or its shared internal successor | Process/profile/generation control, initial ownership, finite acquisition/retirement and common launch primitives | Single temporary generation, bounded cleanup and no wrong-owner stop. |
| `app/service/cmd/unicd`, service host/server and `internal/localapi` | Lifetime mode, authoritative temporary lease admission, operation observation and generation-bound release | Real IPC transitions, disconnect cleanup, terminal observation and capacity behavior. |
| `app/desktop/internal/lifecycle` | Shared transition serialization with existing normal/lightweight policies | Nonvisual launcher-process takeover/reuse/conflict acceptance; no GUI launch implied. |
| Shipped migration entry point/tool | Exclusive lock on the selected target | Default/alternate-target conflict checks without auto-start or auto-stop. |
| Engine/public SDK | Preserve single Engine/profile and existing operation/resource semantics; change interfaces only for a demonstrated missing contract | Real local-commit/send/apply separation and resource-release evidence. |
| Observability | Existing startup/identity/loss behavior; no lease telemetry or changed upload defaults | Local controlled-receiver regression and privacy scan. No hosted write. |

Known existing regression entry points include `make headless-e2e`, `make desktop-startup-e2e`, `make logs-export-e2e`, `make posthog-e2e` and the CLI testscript suite. Inspect their actual fixture/launch behavior before selecting them. Desktop startup tests are allowed here only when they use nonvisual fixture processes. A target that launches a GUI or VM requires separate authorization.

Existing suites are regressions, not proof of the new acquisition/lease feature. P0 must add a real new suite and document its exact runnable selector/entry point. This planning-only document does not invent a selector for a suite that does not yet exist.

## 4. Failure inventory and E2E acceptance matrix

| ID | Scenario | Required observation |
|---|---|---|
| C01 | Local command, help, alias help or invalid arguments | No daemon creation/probe, no owner change, correct output/exit. |
| C02 | Service-dependent leaf/alias/conditional flag | Declared requirement is used; no uncatalogued default launch path. |
| C03 | Cold finite status/query | One matching temporary service, completed response, then process/socket/profile-lock cleanup. |
| C04 | Warm standalone or desktop-owned service | Same generation remains alive after success, error and cancellation. |
| C05 | Two cold starters | One admitted temporary generation; no duplicate profile ownership or hidden replacement. |
| C06 | Starter finishes while borrower is active | Starter returns; borrower continues; no premature stop or wait for unrelated work. |
| C07 | Last release races with new admission | Admission wins in the live generation or retries before business execution; no accepted client loses service through normal last-release cleanup. |
| C08 | CLI dies before readiness/first admission | Initial ownership is released; startup/child cleanup stays bounded. |
| C09 | CLI force-kill or lease connection loss after admission | Dead usage is reclaimed without a CLI defer; another live user keeps service alive. |
| C10 | Last user exits and no new user appears | No convenience idle retention; verified process termination and lock release within the frozen budget. |
| C11 | Missing/nonexecutable sibling or wrong installed pair | Clear startup error; no PATH fallback or unrelated process replacement. |
| C12 | Bad config, damaged credentials, startup crash or timeout | No silent credential reset; correct failure category; no leaked initial child/socket/lock. |
| C13 | Typed directory-in-use during retirement | Bounded retry under the startup deadline; no error-text parsing or second Engine. |
| C14 | Wrong profile, unknown listener, permission error or probe timeout | No auto-start-as-replacement, arbitrary unlink or PID kill. |
| C15 | Abrupt daemon death/stale socket | Report interruption; only safely recognized absence permits new acquisition; mutations are not replayed. |
| C16 | Old cleanup after desktop replacement | Old generation stop/release is rejected or harmless; new desktop service survives. |
| C17 | Normal desktop replaces an active temporary generation | Existing desktop policy preserved; affected CLI reports interruption/uncertainty without replay. |
| C18 | Lightweight/direct standalone startup meets temporary users | Explicit retirement/conflict policy; no implicit lifetime promotion. |
| C19 | Migration uses default or alternate target | Only the actual target's exclusive ownership matters; no owner is stopped automatically. |
| C20 | Clipboard command with absent/denied/unsupported adapter | Existing capability error; temporary service released; no GUI or history-as-clipboard fallback. |
| C21 | Cold sender with eligible peer initially connecting | Bounded connection opportunity; one dispatch; no stop at service-ready or queued admission. |
| C22 | Offline targets, pending approval, sync disabled or no backend | Finite completion; existing local commit/disposition meaning preserved; no fabricated send or trust bypass. |
| C23 | Partial/multi-target send, slow file and send timeout | Accurate per-command terminal outcome; unrelated work excluded; resources retained until terminal/cancellation barrier. |
| C24 | Commit succeeds but response is lost or service replaced | Commit may exist; no automatic replay/new request key or false rollback/durable-query promise. |
| C25 | Watch starts another CLI concurrently, then cancels | Watch does not hold the startup lock; only its lease is released. |
| C26 | Pairing pending, approval, expiry and replacement | Distinct outcomes; session remains available for its adopted window; unrelated peer events do not finish it. |
| C27 | Code/ticket printed under temporary versus persistent ownership | Correct foreground/availability policy; output alone is not mistaken for completed temporary usage. |
| C28 | Invitation A replaced by B; A exits | B's state survives A's cleanup; generation/ownership checks apply. |
| C29 | Temporary log override expires, resets or is replaced | Adopted window honored; old cleanup cannot remove a new override; persistent setter behavior remains explicit. |
| C30 | stdin text/file pipelines and JSON output | Ownership channel consumes no command input; startup messages do not enter stdout; output closes before release. |
| C31 | Command error, Ctrl+C and already-cancelled context | Independent cleanup budget; no leaked last-user child; committed results remain distinct from cleanup failure. |
| C32 | Graceful stop fails and forced cleanup is considered | Only the same verified temporary generation with zero users is eligible; no borrowed/new-generation kill. |
| C33 | Lease/admission capacity pressure and repeated invocation | Finite rejection, no growing durable bookkeeping, normal capacity restored after release. |
| C34 | Enabled telemetry across temporary restarts or analytics disabled | Identity/disable semantics preserved; per-process starts are not installation counts; no lease/profile/credential leakage. |
| C35 | High-latency startup, blocked output consumer or incomplete response stream | No premature success/release; deadlines and backpressure follow the frozen contract. |
| C36 | Identical binary pair under supported process platforms | Applicable process tests and compile/dependency checks pass; missing native/platform acceptance is NOT_RUN, not inferred. |

Use explicit fixture barriers for admission, operation completion, connection delay and replacement races. If a failure needs an artificial listener, adapter or transport, label exactly what it proves. Do not claim fake capability/clipboard reports establish native OS operation or fake network availability establishes a real tailcat path.

## 5. Reproducible artifacts

Each phase retains a private sanitized directory:

```text
manifest.json             source SHA/tree digest, clean/dirty identity, dependencies, build flags, toolchain and binary hashes
commands.json             exact commands, environment key names, prerequisites and exit codes
command-catalog.json      leaf/alias/conditional requirement and completion coverage
scenarios.json            expected/actual values with PASS/FAIL/NOT_RUN and failure reasons
timeline.jsonl            monotonic fixture events using fixture-only client/profile/generation ordinals
operations.json           reduced local commit/send/apply results and resource-release assertions
cleanup.json              child termination, socket removal, lock reacquisition and surviving-owner checks
resource-summary.json     unchanged baseline/candidate measurements and preset thresholds
replay.sh                 deterministic local entry point, including binary construction/prerequisites
SHA256SUMS                recursive retained-artifact checksums
```

Retain real generation/lease mappings only inside the active fixture, then reduce them to ordinal labels. Do not retain actual credentials, pairing invitations, analytics identifiers, private profile/socket paths, raw transport logs, real clipboard content or node databases. Synthetic file integrity assertions may retain deterministic fixture sizes and hashes, clearly separated from production telemetry.

The artifact must identify the actual binaries used for each scenario, including fixture tags and race instrumentation. Verify the recorded source hashes against the frozen/committed source. Checksum and syntax checks do not replace executing replay; record whether replay actually ran. Preserve failed baseline/resource results instead of overwriting them with a later pass.

Retain manifests and reduced evidence, not source bundles, executables or native screenshots by default. Hosted/native evidence and publication receipts, if later authorized, belong in separate layers.

## 6. Documentation validation and current receipt

Validate this planning-only change with:

```sh
go run ./tools/docscheck
```

Do not bump architecture verification metadata for unimplemented behavior. Do not initialize a repository or fabricate verification commits if source identity is unavailable. During implementation, commit verified scoped source before the normal docscheck metadata bump, preserving unrelated workspace changes.

Receipt for the working-tree implementation, based on HEAD `1f0e6c815f` plus uncommitted changes. The source is not committed, so `verified` metadata is not bumped.

| Layer | Current state |
|---|---|
| User requirement and decision draft | Recorded in 0021 |
| Execution sequence, failure inventory and artifact contract | Documented by this plan |
| Adoption of detailed completion/foreground policy | Implemented as described in section 11 of 0021 |
| Lifecycle/operation DTOs, lease mechanism, limits and resource thresholds | Implemented (`internal/localapi/lease.go`, `internal/servicelaunch`, `app/service/cmd/unic/session.go`); thresholds were fixed after the implementation existed, not before |
| Process suite (`make service-lifecycle-e2e`) | PASS: 29 sub-scenarios (C01–C16, C18–C20 with C19b, C25, C27–C33, C35), plus C17 asserted inside C16. Final run after the last test change |
| Cases asserted by other suites | C18b: `TestStartupProcesses`. C21, C22, C26: `TestCLI` scripts `cold-start-send`, `cold-start-offline-peer`, `cold-start-pairing`, `cold-start-pairing-outcomes`. C24 (killed CLI): `cold-start-send-response-lost`. C34: `TestPostHogTemporaryRestarts`. `scenarios.json` lists them as NOT_RUN for this suite and names the other suite |
| Resource comparison (`make service-lifecycle-bench`, HEAD baseline) | PASS. Warm status median 5 ms (baseline) vs 6 ms; cold status median 64 ms; 16 concurrent cold commands 4.9 s with one `unicd` at most; no process or socket left after 30 repeats. Thresholds were fixed after the implementation existed |
| Regression | PASS after the last runtime change: `headless-e2e`, `posthog-e2e`, `desktop-startup-e2e`, `logs-export-e2e`, `TestCLI` (23 scripts) with `TestCatalog`, and `go test ./...` |
| `go test ./...` | PASS with `TMPDIR=/private/tmp`. With the default macOS `TMPDIR` it fails in `host`, `engine` and `v1migration` with `cannot open default file cache`, also on a clean HEAD worktree. The cause is the long temporary path, not this change. The Makefile targets set `TMPDIR` |
| NOT_RUN | C23 (partial and slow-file sends) and C36 (other platforms are compile-checked only) |
| Native GUI/clipboard/login and hosted/publication actions | NOT_RUN; separate authorization required |

Defects found while closing the plan:

1. Fixed. Concurrent cold commands failed (3 of 16) when a retiring generation closed its connections. The probe and the lease request returned `EOF` or "not running" before any business request was admitted. `session.go` now retries these control-plane errors within the 45 s acquisition budget.
2. Decided, no code change. A freshly started `unicd` creates its socket several seconds before it answers requests (about 4 s on a new profile). A CLI that probes in this window gets a probe timeout after 1 s and exits non-zero with `inspect existing unicd ... context deadline exceeded`. It does not retry, replace the process or unlink the socket. This matches the contract and C14. The old CLI waited, so the behavior differs from HEAD.
3. Fixed. `unic migrate` failed in its verification step with `cannot open default file cache` when the target path contains a symbolic link, such as `/tmp`. The second engine open used the caller's path, not the resolved one. This also happens on clean HEAD. C19b covers it. The fixture builder moved to `internal/v1migration/v1fixture` so the e2e suite can reuse it.
4. Open, outside this plan. A cold-start `submit` can send nothing. The command waits 10 s for paired devices to connect, then finds no connected device and records `no_targets`. A running service is not affected.
   - Cause, confirmed by measurement. A process that restarts with the stored client key reconnects slowly. The time is spent in `tailcat.Client.DialTCPPort`, not in the identity handshake. The peer's magicsock keeps trusting the direct UDP address of the previous process for 6.5 s (`trustUDPAddrDuration`) and sends the WireGuard handshake reply to a closed port. WireGuard retries every 5 s. In a probe against tailcat v0.7.0 alone, a restart within 4 s of the previous process took 5.5 to 11 s. A restart after 6 s or more took about 1 s. A client with a new key always took 0.8 to 2 s. A cold-started `unicd` took 2.6 to 17 s with the real peers.
   - Tried and rejected: a client key that is new for each process. It reconnects in 0.8 s, but the peer's tailcat server keeps two goroutines and about 40 KB for every key it has seen. It has no eviction. It grows with every CLI invocation on another device.
   - Tried and not adopted: a tailcat fork that resets the peer's path state when a client restarts. A boot ID in the meow ping separates a restart from a resend. In an isolated probe it brings every restart to about 1 s (original: 5.5 s). In the full product topology (three nodes, each one both server and client) it made restarts 2 to 4 s when it worked, but 4 of 12 runs did not connect within 40 s. The original had 0 failures in 12 runs. The cause of the remaining failures is not understood. The patch and the probe are kept in `.cache/tailcat-fork/`.
   - A restarted device is affected in the other direction as well: a long-running peer keeps a stale path to the restarted device's server for the same 6.5 s.
   No test pins the current behavior.

Section 2 boxes are checked only where a passing scenario or the code supports the item. Each open item states its reason. Do not archive 0021 before every item is closed and the source is committed.
