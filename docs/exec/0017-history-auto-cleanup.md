# Execution plan: Automatic history cleanup

- Status: Implemented; CLI acceptance passed (16 cases). Native visual acceptance pending.
- Date: 2026-11-09
- Requirements: [0017 decision draft](../decisions/proposed/0017-history-auto-cleanup.md).
- Scope: Automatic cleanup, retention age, item count, tag exclusions, and replacement of separate file cleanup with unified managed-file/history deletion.
- Source baseline: `aaba41ae7117a07b63d9eb9785c538af32fd3a25`, branch `v2`, with task changes in the working tree.
- Evidence boundary: Real-process CLI acceptance and existing non-native checks. No native application was launched.

## 1. Initial evidence before implementation

| Area | Source | Consequence |
|---|---|---|
| Public history options | `engine/history_types.go`, `engine/options.go` | `HistoryOptions` is fixed at `New`; zero `MaxItems` selects 10,000, not unlimited. |
| Desktop configuration | `app/desktop/internal/config/config.go` | Existing fields include `history.max_items`, `history.max_bytes`, and `history.default_ttl_days`; reuse the configuration path. |
| History ownership | [History contract](../contracts/history-contract.md) | Deletion releases history references, preserves active holders, and must not unlink host-managed files. |
| Expiry and eviction | `engine/history.go`, `internal/store/history.go` | Inspect all expiry, count, byte-budget, and shared-budget deletion paths before adding protection. |
| Tag exclusions | [History contract](../contracts/history-contract.md) | Existing history metadata and update APIs support tags. Reuse them for cleanup matching; add no favorite field. |
| File cache | [Desktop runtime](../architecture/desktop-runtime.md) | Current eviction preserves history metadata. Replace this behavior with coupled file/history deletion while preserving ownership boundaries. |

This plan does not assume that an existing GUI or configuration option already implements the requested behavior.

## 2. Implementation sequence

1. Resolve the review points in the decision draft. Inspect the current settings save path and maintained product patterns before choosing control options.
2. Update the contracts for tag-based cleanup exclusions, live policy changes, unlimited count, coupled file/history deletion, removal of separate file-cleanup settings, and storage pressure.
3. Deliver the smallest complete path through Engine, daemon configuration, and CLI. Start with one managed file and its bound history: delete both through the CLI and prove the result across a daemon restart. Persist and enforce the excluded-tag list on the same path.
4. Replace the standalone file-age worker and startup cleanup branch with unified age/count cleanup. Remove `file_cache.retention_hours` and `file_cache.auto_cleanup` across TOML, Engine settings, API DTOs, defaults, validation, templates, CLI/GUI, and obsolete E2E expectations. Add no compatibility path. Keep ownership tracking and recovery of unreferenced files.
5. Route capacity reclamation through the same record deletion and tag rules. Keep `file_cache.max_bytes` as an admission bound. When cleanup is off or insufficient, reject new content without deleting protected data. Cover shared payloads and concurrent tag updates. Reuse the existing maintenance path and bounded transactions.
6. Add the four settings with the existing UI Kit. Reuse existing history tag editing. Add a multi-select control for tags excluded from cleanup.
7. Update architecture and configuration documentation. Run documentation checks and bump only architecture pages verified against the implementation.
8. Complete process-level acceptance. Hand native interaction and visual acceptance to the user unless explicitly authorized.

Do not replace the working history path while later stages remain incomplete.
Do not add unit tests after implementation. Use the existing real-process E2E harness.

## 3. Failure modes to cover before implementation

- Disabling cleanup still deletes records through an unintended count or age path.
- An old per-record default deadline conflicts with the new policy or prevents tag exclusions.
- Unlimited count accidentally becomes the existing 10,000-record default.
- A record with an excluded tag is removed by byte-budget or shared-budget eviction.
- Protected records exhaust storage and the application deletes them to admit new content.
- A concurrent tag or policy update loses a race with cleanup.
- A policy change only affects new records or does not survive restart.
- A large cleanup blocks capture, holds a long transaction, or exceeds bounded memory.
- Cleanup invalidates a live read, send, or apply lease.
- Cleanup deletes a source file or a custom received file.
- A database failure publishes a deletion event without a committed deletion.
- Wall-clock changes cause boundary errors or repeated deletion attempts.
- File cleanup deletes a payload but leaves its bound history record.
- History deletion leaves an unreferenced managed file without a recoverable cleanup task.
- A record containing multiple files becomes partially available after automatic cleanup.
- An old file-age worker or payload-only quota eviction survives and bypasses unified cleanup or tag exclusions.
- Shared payload deletion destroys content still referenced by excluded history.
- A crash between logical deletion and physical reclamation leaks files or restores deleted records.
- Removed file-cleanup keys or controls remain exposed or active.
- Capacity reclamation silently deletes records while unified automatic cleanup is disabled.

## 4. End-to-end acceptance

Use isolated `UNIC_HOME` directories, real `unic`/`unicd` processes, and the existing directory clipboard.
Use the real history import path for dated fixtures when supported. Avoid long sleeps and direct database edits.
Write the scenarios before implementing the behavior.

| Scenario | Observable result |
|---|---|
| Cleanup off | Age and user count rules do not delete records; saved rules survive restart. |
| Age boundary | Eligible old records disappear; newer records remain; restore does not refresh creation time. |
| Count and unlimited | Oldest eligible records disappear under a limit; unlimited does not select the old default cap. |
| Tag exclusions | A record with any selected tag survives age/count cleanup and restart; removing its last matching tag makes it eligible. |
| Tag matching | Multiple selections use OR matching; an empty list excludes nothing; unrelated tags do not match. |
| Existing records | A stricter saved policy cleans eligible existing records without restart. |
| Explicit expiry | A per-item deadline follows the reviewed expiry rule independently of the cleanup switch. |
| Storage pressure | Protected history remains intact; admission reports resource exhaustion when required. |
| Concurrent changes | A committed tag or policy change governs subsequent deletion commits. |
| Content ownership | Existing leases finish; custom received files and source files retain their hashes. |
| Coupled deletion | Unified cleanup deletes eligible history and releases its managed files. Neither path leaves a record pointing to a cleanup-deleted payload. |
| Tag exclusions across triggers | Excluded records and their managed files survive unified age/count cleanup and permitted capacity reclamation. |
| Multiple files and shared payloads | Cleanup removes the whole eligible record; protected references remain readable; shared bytes are reclaimed only after the final holder releases them. |
| Crash recovery | Restart completes pending physical reclamation without reviving deleted records or deleting valid files. |
| Single cleanup switch | Turning automatic cleanup off stops age/count deletion and capacity-driven history deletion; insufficient capacity rejects admission. |
| Removed settings | Separate file-cleanup keys and controls are absent from configuration output, templates, SDK/API settings, and GUI; the old file-age cleanup no longer runs. |
| Recovery | Restart preserves the policy, record tags and the excluded-tag list, history revision, and committed deletions. |

Retain a reproducible artifact per acceptance run:

- Source identity when available, Go version, platform, exact commands, and exit codes.
- Sanitized settings and fixture recipe, including timestamps and expected survivor IDs.
- Before/after CLI results, process logs, and file hashes for ownership checks.
- A machine-readable assertion report and a replay command.

Keep artifacts outside user data. Exclude clipboard secrets, pairing material, addresses, and credentials.
Record native visual acceptance separately from CLI E2E results.

## 5. Closure

- [x] Policy scope resolved with the user.
- [x] Separate file-cleanup logic and settings removed.
- [x] Contracts, configuration, and affected 0016 scope updated.
- [x] CLI and Engine path accepted with a replayable artifact.
- [x] GUI controls connected to the daemon save path.
- [ ] User visual acceptance recorded.
- [x] Architecture verified and documentation checks passed.

Implementation and acceptance results are recorded below. Native visual acceptance remains pending.

## 6. Implementation result

- Added live `Settings.History` and TOML `history.auto_cleanup`, `retention_days`, `max_items`, and `excluded_tags`.
- Defaults: enabled, 180 days, unlimited count, no excluded tags. The default per-device file-cache quota remains 500 MB.
- Removed `file_cache.retention_hours`, `file_cache.auto_cleanup`, and the payload-only eviction path. Unknown old keys are rejected; no compatibility layer was added.
- Added oldest-first, tag-filtered cleanup in `engine/history_cleanup.go`. Policy/tag changes and deletion serialize under the history mutex.
- Cache ownership and reservations remain in `engine/file_cache.go`. Unreferenced payload reclamation preserves active holders and keeps failed-deletion bytes charged.
- Shared payload reclamation checks every referencing record for excluded tags. Protected references prevent quota-driven reclamation.
- Fixed the explicit clipboard-write lease path: acquire the new snapshot before releasing a replaced file lease. The old rebase path hid this release from the poller.
- Added `GET /v1/history/tags`. The desktop Settings button opens four cleanup controls and the cache quota control. Saves use existing revision-checked configuration writes.
- Preserved unrelated UI Kit/layout changes already present in the working tree. Application deployment was not performed. The user subsequently authorized committing this task.

## 7. Validation and limits

Race-enabled `unic` and `unicd` builds pass. All existing tests defined in `engine/history_test.go` pass with the race detector.
Existing CLI history tests, plugin discovery, and the in-process empty-history layout check pass.
The layout check uses `ui.NewTester`; it is not native visual acceptance.
Config, store, and local API package checks pass.

A broader existing-test run still reports nine Engine failures and one host failure.
An isolated archive of the baseline HEAD reproduces each of these failures:

- Engine backend expectations: `TestBackendsGetIndependentViewsAndThresholds`, `TestBackendPoliciesAreEffectiveValidatedAndHotUpdated`, `TestHistorySyncSummaryFollowsTheOperation`.
- Engine import/staging expectations: `TestUserLimitsApplyToImport`, `TestNewRemovesOnlyItsOwnLeftovers`.
- Engine receive expectations: `TestReceiveBetweenEngines`, `TestReceiverHonoursItsOwnSettings`, `TestReceivedFilesAreRemovedWithTheirContent`, `TestCloseAndStartupClearTheReceiveDirectory`.
- Host receive cleanup expectation: `TestFileSync`.

These failures are not presented as passing acceptance. They remain outside this task's implementation scope.
On macOS, checks use canonical `TMPDIR=/private/tmp`; the default symlinked temporary path fails existing managed-root validation.

`go run ./tools/docscheck` passes with two pre-existing staleness warnings outside this task.
The first `-bump` attempt was rejected while source paths were uncommitted.
After the user requested a commit, the task was committed as `5f4e8a3ab28a5867eea2f43880bbe0aba5bb0a6c`.
The pipeline and desktop-runtime `verified` fields now point to that commit. Unrelated UI changes remain outside the commit.
`git diff --check` passes.

Not run: native interaction/visual acceptance, Windows/Linux native execution, forced disk-full/unlink failures,
crash injection at every filesystem/database boundary, and exhaustive concurrent clock/tag mutation permutations.
The restart, SIGKILL, clipboard-holder, quota, and symlink scenarios provide bounded evidence only.

## 8. Manual native acceptance

1. Start the matching daemon and desktop client. Open Settings from the footer button.
2. Change retention, count, and excluded tags. Verify saved values survive reopening the page.
3. Verify cleanup-dependent controls disable when cleanup is off. Verify the cache-quota control remains enabled.
4. Verify a tagged managed file and its history survive automatic cleanup; remove the exclusion and confirm eligible history/file deletion after all holders release the file.
5. Verify keyboard focus and light/dark layouts on the target platform.

## 9. Final CLI acceptance receipt

All 16 CLI E2E cases pass using matching race-enabled binaries and the race-enabled harness.
The final full run completed in 237.422 seconds. Child-process logs contained no race reports.
The shared-file quota scenario includes two history references to the same payload and an excluded tag.
The retained-payload count remains one, and removing the exclusion permits coupled record/file deletion.

Artifact directory: `/tmp/unic-cleanup-final-source-evidence`.
Aggregate receipt: `/tmp/unic-cleanup-final-source-evidence/summary.json`.
Each case retains its manifest, cache inventories, fixture/configuration hashes, and replay command.
All recorded source-file hashes match the final workspace files. All cases record the same binary hashes.

```sh
go build -race -o /private/tmp/unic-cleanup-final-bin/unic ./app/desktop/cmd/unic
go build -race -o /private/tmp/unic-cleanup-final-bin/unicd ./app/desktop/cmd/unicd
UNIC_E2E=1 UNIC_E2E_BIN=/private/tmp/unic-cleanup-final-bin UNIC_E2E_ARTIFACTS=/tmp/unic-cleanup-final-source-evidence go test -race ./e2e/cli -run TestCLI -count=1 -parallel 4 -v
```

All 24 existing checks in `engine/history_test.go` also pass with the race detector.
The native checklist in section 8 remains for user acceptance.

## 10. Scoped commit verification

The feature commit is `5f4e8a3ab28a5867eea2f43880bbe0aba5bb0a6c`.
Only the task's 44 files/hunks were staged. Shared README and GUI files were staged partially;
the native-layout demo and unrelated scroll/UI Kit changes remain unstaged.

A full index snapshot built both race-enabled binaries independently of the unstaged changes.
Three real-process cases passed against those binaries: `history-auto-cleanup`, `history-cache-quota`,
and `file-cache-retention` (50.846 seconds total). Every staged task file matched that tested snapshot before commit.
The previously recorded 16-case run covered the shared working tree; this additional check verifies the isolated commit.

Architecture verification metadata and this receipt are recorded in a follow-up documentation commit.
No push or application deployment was requested or performed.
