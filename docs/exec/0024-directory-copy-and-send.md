# Execution plan: Directory copy and explicit send

- Status: Implemented; local process acceptance passed. Native UI and real-device acceptance remain pending.
- Date: 2026-11-12
- Decision: [0024: Directory copy and explicit send](../decisions/proposed/0024-directory-copy-and-send.md).
- Source baseline: `1d542f4ca7412972ae4c2cbbac46c211ae19395c`, branch `session/cebfb630`, with the uncommitted 0024 documentation changes.
- Code commit: `be9b3d81a4d9033dd26b55f08305cb7982aa8429`.
- Scope: Deferred directory selection, explicit one-shot send, optional automatic directory send, tree receive/apply, and ownership-safe cleanup.
- Authorization boundary: The user subsequently authorized implementation. Native application launch, deployment, and real-device execution still require separate authorization.

## 1. Required outcome

Copying a selection containing a directory records only its top-level references by default. It does not traverse descendants, allocate a snapshot, or send any part of the selection. Explicit `send` prepares the current source contents and sends one complete selection. The receiver can apply the top-level files and directories without exposing an archive to the user.

`sync.directories.automatic` defaults to `false`. Enabling it permits preparation once per new copy action. Editing the source alone never initiates a transfer. Explicit send bypasses automatic restrictions only. Existing authorization, file policy, transfer limits, and storage admission remain active.

## 2. Code map to verify before implementation

| Area | Existing entry points | Work |
|---|---|---|
| Clipboard paths and observation | `app/desktop/internal/board/board.go`, `internal/clip/clip.go`, `internal/clipboardintegration/clipboard.go` | Distinguish path kinds and copy actions without recursive polling. Keep apply serialization and echo suppression. |
| Local source access | `internal/localcontent/content.go`, `internal/clipboardintegration/files.go` | Retain private source references and validate source identity during preparation. |
| Engine content and history | `engine/content_types.go`, `engine/content.go`, `engine/history.go`, `internal/store/history.go` | Represent trees, deferred references, immutable snapshots, and history states without treating a reference as frozen bytes. |
| Storage and receive | `engine/file_cache.go`, `engine/receive.go`, `app/service/internal/host/received_files.go` | Reserve storage, stage trees, publish a complete batch, and retain ownership through leases. |
| Send and policy | `engine/submission.go`, `engine/backend.go`, `engine/peersend.go`, `internal/localconfig/` | Apply directory and mixed-selection policy before expensive preparation. |
| Transport and plugins | `internal/wire/wire.go`, `engine/pluginbackend.go`, `internal/pluginproc/` | Carry bounded manifests and file streams across supported backend paths. |
| Service, CLI, desktop | `app/service/internal/host/`, `app/service/cmd/unic/`, `internal/localapi/`, `app/desktop/internal/desktopdemo/` | Expose selection send, progress, cancellation, and settings through existing paths. |
| Process acceptance | `e2e/cli/`, `internal/testboard/dir.go` | Reuse real processes, paired nodes, and isolated directory clipboards. |

Verify actual call sites and applicable platform instructions before editing. These paths identify starting points, not a commitment to introduce a new abstraction in each directory. Inspect the existing mygo clipboard change facilities and current dependencies first. Prefer maintained platform facilities and existing libraries. Do not add an archive dependency or rewrite transport framing.

## 3. Contract gate before production code

Resolve and record the following in the affected contracts. Do not start a partial production implementation with these semantics undecided.

1. Define the selection, root, entry, payload, and source-reference DTOs. Specify relative-path syntax, ordering, duplicate detection, payload binding, integrity checks, and empty-directory representation. Keep source absolute paths out of public DTOs and transport.
2. Choose bounded manifest encoding. Keep manifest payloads separate from the bounded control envelope when needed. Specify maximum manifest bytes, entries, depth, total selection bytes, per-file bytes, and temporary snapshot storage. Record exact values and overflow behavior. Reuse existing limits where they have the same meaning; do not add a user setting for every safety bound.
3. Define deferred-reference history states and their persistence. Specify source identity, unavailable references, explicit send of a source versus resend of a snapshot, and whether a completed snapshot is retained. Never mutate committed content bytes in place or count references as owned payload bytes.
4. Define overlapping roots, duplicate selected roots, and equal root names. Preserve top-level selection semantics or reject an ambiguous selection before sending. Do not silently duplicate or discard entries.
5. Define portable path validation and receiver filesystem checks, including reserved names, case/normalization collisions, and file/directory conflicts. Never silently rename descendants.
6. Define source permissions, links, junctions, hard links, special files, metadata preservation, concurrent mutation detection, and root replacement behavior. Preserve contents and empty directories. Do not imply ACL, extended-attribute, ownership, or atomic filesystem-snapshot guarantees without an explicit contract.
7. Define preparation ownership, cancellation, failure codes, crash recovery, batch publication, and custom-destination staging. Specify how temporary and retained bytes share admission without double charging or uncharged duplication.
8. Define per-platform copy-event detection and echo handling. Separate local copy identity from cross-device content identity. Specify fallback limitations if mygo does not expose a suitable platform change marker. Do not claim repeated-copy support from name/size hashing alone.
9. Define CLI and desktop send entry points, selected targets, operation progress, and cancellation over the existing service path. Distinguish automatic dispatch, explicit source send, and snapshot resend. Specify optional temporary-transfer behavior explicitly; do not silently advertise directory support on a path that cannot carry it.

Update the [Engine contract](../contracts/engine-contract.md), [history contract](../contracts/history-contract.md), [pipeline contract](../contracts/pipeline-contract.md), [backend contract](../contracts/sync-backend-contract.md), and [local service contract](../contracts/local-service-contract.md). Update plugin/RPC/settings contracts where their DTOs change. Keep wire version 1 and add no migration or compatibility path.

## 4. Failure inventory and acceptance preparation

Write process E2E scenarios and fixture expectations before production implementation. Do not write unit tests after implementing the behavior. If an isolated probe is necessary, enumerate its failure modes before writing probe code and record that its evidence is narrower than process acceptance.

- Copying a directory traverses descendants, opens file bodies, or allocates snapshot bytes while automatic directory sync is off.
- Mixed selections send regular files while withholding directories.
- Recopying the same directory is lost, or remote apply is mistaken for a new local copy.
- A root is replaced, a member changes during preparation, or different targets receive different bytes.
- Permission denial, unreadable descendants, links, or special files produce silent omissions.
- An empty tree disappears, zero-byte members fail, or selected roots merge unexpectedly.
- Host references, descendant paths, or pairing material leak into logs or ordinary DTOs.
- Malformed metadata escapes the receive root, aliases another entry, or bypasses size accounting.
- Scan, manifest parsing, or snapshot preparation exceeds memory, entry, depth, or storage limits.
- Cancellation, client loss, timeout, disconnect, or crash leaves reservations, leases, or private payloads held indefinitely.
- Partial receive publishes usable history, replaces the clipboard, or overwrites existing user content.
- Cleanup removes a source tree, a published custom receive, or content still held by a clipboard/send lease.
- A late receive replaces a newer local copy, or stopping/restarting a service resends an old selection.

Use deterministic synthetic trees and bounded process synchronization. Add narrowly scoped test-only diagnostics only where black-box behavior cannot establish a property, such as absence of descendant reads or snapshot allocation. Exclude diagnostic hooks from release builds. A lack of network sends alone does not prove lack of traversal.

## 5. Implementation order and gates

### P0: Contracts and executable scenarios

Complete section 3. Add fixtures and initially failing E2E scenarios for the minimal path and default no-work policy. Extend existing harness helpers only where they cannot represent trees or repeated copy events. Keep the existing working file path intact.

Exit: exact semantics and limits are documented; failures demonstrate missing directory behavior rather than harness errors.

### P1: Minimal explicit-send vertical slice

Implement the smallest complete path: explicit selection of one directory, source validation, bounded background scan, private snapshot, manifest plus file streams, trusted peer transfer, complete default-cache publication, and directory-clipboard application. Include nested and empty directories. Use existing operation and lease ownership. No eager scan on clipboard reads.

Exit: two real paired processes transfer a deterministic tree; the received inventory, empty directories, and every file hash match; cancellation/failure reclaims the slice's private staging. Ordinary file send still passes. Keep this path working while adding later stages.

### P2: Deferred copy, history, and full policy

Connect clipboard selection capture to the deferred-reference model. Add source-reference history and explicit send through the service/CLI. Implement the default-off setting and hot updates through existing configuration. Apply directory policy to the complete mixed selection before preparation. Validate all targets and relevant policy before costly work, then recheck policy at dispatch/publication barriers.

Add enabled automatic directory send and repeated-copy detection. Editing a source without a new copy must not trigger preparation. A user-triggered new send prepares new source bytes; all targets of one operation share one immutable snapshot. Keep unrelated regular-file behavior.

Exit: default no-traversal/no-snapshot/no-send, explicit send with automatic sync off, enabled automatic send, history semantics, and repeated-copy scenarios pass through real processes.

### P3: Receive ownership and failure completion

Complete mixed/multiple-root handling, custom destinations, full manifest validation, per-file and aggregate limits, source mutation, multi-target results, and policy/lifecycle races. Extend existing cleanup and recovery to entire owned trees. Protect active leases and leave published custom trees user-managed. Complete supported backend/plugin mappings without backend-specific directory policy.

Exit: failure/security/ownership scenarios pass; no partial history or clipboard publication; no source/custom-content deletion; no leaked reservations or private staging after bounded cleanup and restart.

### P4: Desktop integration

Connect the real service send operation to directory-reference entries and selected targets. Use the existing UI Kit and settings composition. Show source-reference semantics and scanning/preparation/sending stages. Expose cancellation. Avoid percentages while totals are unknown. Separate source send from snapshot resend if both are exposed. Preserve unrelated panels and their state.

Exit: service-backed behavior is accepted through nonvisual process checks. Native visual and clipboard results remain separate and pending until explicitly authorized and run.

### P5: Full acceptance and documentation landing

Run the scoped matrix and ordinary-content regressions. Compile affected host targets. Run native clipboard/cross-application paste and real-device checks only under separate authorization. Record each platform and layer as PASS, FAIL, or NOT_RUN. A build is not platform runtime acceptance.

Land verified behavior in the [pipeline architecture](../architecture/pipeline.md), [desktop runtime](../architecture/desktop-runtime.md), affected contracts, configuration documentation, and application instructions. Complete the documentation lifecycle only after required evidence exists. Keep the decision proposed while required implementation or acceptance remains incomplete.

## 6. Process E2E matrix

| ID | Scenario | Required observable result |
|---|---|---|
| D01 | Default directory copy | Top-level reference exists; no descendant reads, snapshot bytes, transfer, or remote history. |
| D02 | Explicit send, automatic off | Entire tree arrives once; inventories and hashes match. |
| D03 | Mixed file/directory selection | No automatic partial send; explicit send preserves all selected roots. |
| D04 | Nested/empty/hidden/zero-byte entries | Complete expected tree; no implicit ignore filtering. |
| D05 | Automatic enabled and hot updates | Each new copy sends once under policy; toggling the setting does not replay an old selection. |
| D06 | Repeated copy and source edits | Recopy is detected; edits alone do not send; a later explicit source send reads updated content. |
| D07 | Root identity and preparation mutation | Missing/replaced roots and detected changes fail clearly; no partial remote publication. |
| D08 | Links, junctions, special entries, permissions | Explicit complete-batch failure; no followed links or silent omission. Platform-specific cases are identified. |
| D09 | Multiple, equal-name, and overlapping roots | Contract-defined selection behavior; no accidental merge or descendant rename. |
| D10 | Malformed manifest and path attacks | Reject traversal, absolute/duplicate/conflicting paths, invalid payload bindings, and integrity failures; outside sentinel hashes stay unchanged. |
| D11 | Receiver filename constraints | Reject invalid names and case/normalization collisions before publication. |
| D12 | Bounds and policy | Enforce exact boundary values and overflows; explicit send still obeys hard limits, sync master, file/backend policy, and trust. |
| D13 | Cancellation and disconnect | Scan/preparation/send terminate; eventual owned-resource release; source tree unchanged. |
| D14 | Crash and restart | Reclaim incomplete owned staging and reservations; retain valid history; do not replay deferred references. |
| D15 | Default/custom ownership and active holders | History cleanup releases owned trees only after holders finish; source and published custom hashes remain unchanged. |
| D16 | Receive/apply race and echo | Newer local clipboard wins; remote directory apply does not broadcast back. |
| D17 | Multiple targets and policy changes | Successful targets receive identical snapshot bytes; report per-target failure; recheck revocation and lifecycle barriers. |
| D18 | Deferred history and snapshot history | Restart preserves declared semantics; reference send uses current source; retained snapshot resend uses frozen bytes. |
| D19 | Resource responsiveness | Large bounded trees stay within declared budgets; polling/apply remain responsive; progress and cancellation remain usable. |
| D20 | Ordinary-content regression | Existing text, HTML, image, regular-file, explicit send, restore, and cleanup behavior stays valid. |

Use the real production protocol with an adversarial peer fixture for malformed manifests. Do not replace the receive pipeline with a mocked parser. Collect denied-read and allocation evidence for D01 without relying solely on timing or unchanged network counters.

## 7. Commands and reproducible evidence

The following are planned checks, not results:

```sh
# Update the existing txtar catalogue after adding scenario scripts.
go test ./e2e/cli -run TestCatalog -update
# Run the real-process CLI suite, including the new directory scenarios.
TMPDIR=/private/tmp UNIC_E2E=1 go test -race ./e2e/cli -count=1 -v
# Run the repository's existing required checks; do not add unit tests here.
go test -race ./...
go build -o /private/tmp/unic-desktop-0024 ./app/desktop/cmd/unic-desktop
go run ./tools/docscheck
git diff --check
```

Use a frozen source identity and matching binaries for acceptance. Record any test-only build tag separately. Narrow scenario selectors and artifact options must be documented once implemented; do not publish invented runnable commands. Reproduce surprising failures against an independent original-baseline snapshot before calling them baseline defects.

Every acceptance run must emit a small verifiable artifact bundle containing:

- A machine-readable result report with case IDs, PASS/FAIL/NOT_RUN, exact assertions, and failure reasons.
- Source revision and scoped source digest, binary hashes, Go version, platform, commands, and exit codes.
- Deterministic fixture recipe, root mapping, expected/received relative-path inventories, explicit empty directories, file sizes, and SHA-256 hashes.
- Sanitized settings and bounded stage/resource observations, including D01 no-work evidence and cleanup verification.
- A replay command or script and checksums for retained artifacts.

Use private temporary profiles and synthetic content. Stop all owned processes and remove binaries, snapshots, profiles, and raw private logs on success and failure. Retain only the small receipt and reproducible fixture specification by default. Report cleanup failure and preserve the minimum recovery inventory needed to finish cleanup. Do not package personal paths, addresses, codes, credentials, or user clipboard content.

Native acceptance must separately prove copy detection, repeated copy, directory paste in another application, current-clipboard protection, and responsiveness on each claimed platform. Local paired processes do not establish cross-machine or native clipboard behavior.

## 8. Completion checklist

- [x] Product semantics accepted and decision registered.
- [x] Execution plan written.
- [x] Contract gate completed with exact DTOs, bounds, and platform semantics.
- [x] Failure scenarios written before production implementation.
- [x] Minimal explicit-send slice accepted with a replayable artifact.
- [x] Deferred history, default-off policy, mixed selections, and repeated copy accepted.
- [x] Receive validation, quotas, cancellation, recovery, and cleanup accepted.
- [x] CLI and desktop send/settings/progress integrated.
- [x] Local process results and ordinary-content regressions recorded with bounded coverage below.
- [ ] Required platform/native and real-device evidence recorded, with explicit remaining limits.
- [x] Architecture and contracts landed; documentation verified.
- [ ] Decision archived after the remaining required platform acceptance.

The sections above preserve the planned gates. The following receipt records actual implementation and validation. Native and real-device checks are not implied by process results.


## 9. Implementation receipt

Implementation uses bounded directory manifests and existing file streams, not archives. Deferred references are private host sources. Engine prepares immutable snapshots only for explicit sends or enabled automatic directory sync. Mixed selections follow the directory policy. New local copies supersede unfinished automatic directory preparation; explicit sends keep independent lifetimes.

The CLI accepts directory arguments with paired-device `--to`, and supports `history send`. Desktop history displays root names, distinguishes references from snapshots, and exposes send targets, preparation stages and cancellation. The directory automatic setting uses the existing TOML/service/UI path and defaults off. Linux settings disclose the identical-copy limitation.

Receive validates manifest structure before member payloads and verifies hashes before publication. Default-cache clipboard trees use leased views with root-confined linking or charged copying. Custom batches publish by rename and remain user-managed. A private exact-path journal recovers interrupted custom staging without scanning a user destination.

The implementation also fixes service request hashing to include history IDs and device selections. Explicit cancellation reaches pre-Engine admission and owned submission contexts. Operation completion releases those contexts. Caller RPC lifetimes remain independent of background restore sends.

## 10. Accepted evidence

| Layer | Result | Evidence |
|---|---|---|
| Frozen, race-enabled paired CLI processes | PASS: 17 scenarios, including all eight directory scenarios and nine ordinary-content regressions | [Accepted receipt](baselines/directory-copy-send-0024/accepted/receipt.json), [checksums](baselines/directory-copy-send-0024/accepted/SHA256SUMS) |
| Additional source freeze | PASS: eight directory scenarios plus history restore | [Source receipt](baselines/directory-copy-send-0024/current-source/receipt.json) |
| Binary identity across the two process runs | PASS: both `unic` and `unicd` SHA-256 values match exactly | Both process receipts; builds use `-trimpath`, `-race`, `-buildvcs=false`, and `unic_e2e` |
| Production headless programs | PASS: production binaries, paired processes, adapter lifecycle, existing content types and UI dependency boundary | [Production manifest](baselines/directory-copy-send-0024/production-headless/manifest.json) |
| Existing repository race checks | PASS | [Full check receipt](baselines/directory-copy-send-0024/checks/full-race.json) |
| Existing Desktop nonvisual checks | PASS | [Desktop check receipt](baselines/directory-copy-send-0024/checks/desktop-nonvisual.json) |
| macOS Desktop build, Windows/Linux service builds, Windows clipboard adapter compile | PASS; compilation only | [Build receipt](baselines/directory-copy-send-0024/platform-builds/receipt.json) |
| Native clipboard and cross-application directory paste | NOT_RUN | Separate authorization and platform execution required |
| Real devices and Windows/Linux process execution | NOT_RUN | Local paired macOS processes do not establish these layers |

The process receipts bind their initial HEAD plus frozen working-tree changes through source inventories and build-input digests. Private source copies, binaries, profiles and raw logs are removed by the runner. The final Desktop root-name projection was added after the CLI source freeze; it cannot affect the UI-free CLI binaries. The separate compilation receipt binds the final Desktop source and binary. Its source inventory remained unchanged during compilation. This is build evidence, not native interaction acceptance.

Replay process acceptance from the repository with `python3 tools/directorye2e/run.py --output /absolute/new/receipt`. Replay compile-only checks on macOS with `python3 tools/directorye2e/compile.py --output /absolute/new/build-receipt`. Use a writable Go build cache and canonical temporary directory where required. Existing receipt directories are rejected. Fixtures and assertions live in `e2e/cli/testdata/directory-*.txtar`; inventories include empty directories and file SHA-256 values. Separate equal-name roots have separate inventory files.

## 11. Coverage against the planned matrix

| Matrix IDs | Actual evidence | Limit |
|---|---|---|
| D01-D06 | Default scan/snapshot counters stay zero; explicit and mixed sends; opt-in automatic send; repeated same-path copy; edits alone do not send | Native change markers are compile-checked only |
| D07-D09 | Deterministic member mutation, source validation, symlink rejection, reserved names, overlapping and equal-name roots | Exhaustive permission, junction and root-replacement races remain untested on other platforms |
| D10-D11 | Real peer traversal/absolute/duplicate/orphan/size/hash/reserved-name/oversized-manifest attacks are rejected | All Unicode normalization and target-filesystem permutations are not exhausted |
| D12-D14 | File limits, excessive entries/depth, automatic-off explicit send, cancellation, owned-staging crash recovery, restart without replay | Forced disk-full, unlink failure and disconnect at every byte boundary are not exhaustively injected |
| D15-D16 | Custom-tree deletion protection; directory history restore; current-clipboard recheck; newer text progresses during held directory preparation | Native focus/cross-application competition is NOT_RUN; custom-journal crash boundaries are not exhaustively injected |
| D17-D19 | Two receivers get the same selection; durable source history and snapshot paths; bounded scans; cancellation and responsive newer-copy processing | Directory-specific mid-transfer revocation stress and heap/latency benchmarking remain follow-up evidence |
| D20 | Existing text/HTML/PNG/file, file-policy, custom-receive, cleanup/quota, restore and target-selection scenarios pass | Other platform runtime acceptance remains separate |

These rows describe bounded evidence. They do not claim every subcase from the original failure inventory was executed. The decision remains proposed until the remaining required platform acceptance is resolved.

## 12. Regression diagnosis and correction

Intermediate frozen runs exposed a real history-restore regression: the restored item applied locally, but its queued transfer failed when the short adapter RPC ended. The new cancellation propagation had incorrectly inherited every caller context. Restricting propagation to Engine-owned submission lifetimes fixed the regression. History restore subsequently passed both the focused directory matrix and the full frozen regression run. Earlier failures are not classified as baseline defects.

A separate held-preparation scenario verifies that observing a directory does not block a newer text copy. The newer copy cancels obsolete automatic preparation, prevents late directory publication and releases owned staging. Explicit sends remain independent.

Initial sandbox socket denials were resolved with isolated-process execution permission. A check run without canonical `TMPDIR` hit the existing managed-root symlink protection; the canonical rerun passed. Documentation checks pass. The four updated architecture pages are verified against the code commit. Four unrelated architecture staleness warnings remain (identity/trust, observability, real-machine testing, and releases).

## 13. Remaining native checklist

1. Use the code revision recorded with this receipt and matching binaries.
2. With directory automatic sync off, copy a nested directory. Verify the source-reference title and explicit send action. Verify no scan starts until send.
3. Send to another device. Paste into a file manager. Compare root names, empty directories and member hashes.
4. Enable directory automatic sync. Copy the same directory twice on macOS/Windows. Verify one preparation per observed copy and no echo.
5. Copy newer local content during receive preparation. Verify the old application does not overwrite it. Cancel scanning/preparation/sending and check cleanup.
6. Check source-reference versus snapshot wording, progress labels, cancellation, keyboard access and the directory setting in supported skins.

Do not archive 0024 or mark native/real-device acceptance complete before these results exist. No release, deployment or publication was performed.

## 14. PR integration with the updated production UI

PR integration merges v2 commit `b4fff3b8f57d512274322de0ed0317e52a912c72`. Directory history, settings and send state now live in `desktopui`, following the upstream separation of production rendering, demo fixtures and shared translations. The upstream built-in classifier treats directory references and snapshots as `builtin:files`. The directory process scenario checks this tag on both capture and receive.

The complete race check passes after integration. The production-boundary, data-binding and send-device-menu cases pass without native windows. Their source identities and JSON assertions are retained in [Desktop integration receipts](baselines/directory-copy-send-0024/pr-desktop-integration/README.md). Frozen directory process results are recorded separately in [the integration process report](baselines/directory-copy-send-0024/pr-integration/report.md). Earlier receipts remain evidence for their original source revisions. Native clipboard, cross-application paste and real-device acceptance remain NOT_RUN.

## 15. PR 2240 lint correction

The complete failed Lint job log for run `43214064110`, job `131168698571`, reports 14 unchecked error returns and eight import-group violations. Local reproduction with the pinned `golangci-lint v2.15.0` reports the same 22 issues. The separate formatting check also covers the Windows clipboard marker and tagged daemon fixture.

The correction propagates history-send output errors, explicitly discards errors from read-only handle cleanup and failed-start rollback, and applies the configured import formatter. It preserves existing write, sync and close error handling for output files. No lint exclusions or workflow changes are added.

Local `golangci-lint run` reports `0 issues`. `golangci-lint fmt --diff --diff-colored=false` produces no output. Directory-send and history-restore process regressions pass. Production headless acceptance also passes; its manifest is retained with the receipt. Process regression evidence is recorded in [the lint-fix report](baselines/directory-copy-send-0024/ci-lint-fix/report.md). Native clipboard and real-device acceptance remain NOT_RUN.
