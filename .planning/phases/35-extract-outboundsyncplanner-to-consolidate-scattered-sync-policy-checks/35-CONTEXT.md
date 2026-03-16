# Phase 35: Extract OutboundSyncPlanner to consolidate scattered sync policy checks - Context

**Gathered:** 2026-03-16
**Status:** Ready for planning

<domain>
## Phase Boundary

Extract an `OutboundSyncPlanner` domain service that consolidates all outbound sync eligibility decisions (currently scattered across 3 stages in `on_clipboard_changed()`) into a single decision point. The planner produces an `OutboundSyncPlan` that the runtime dispatches without further logic. This is a pure internal refactoring — no user-facing behavior changes.

</domain>

<decisions>
## Implementation Decisions

### Types and location

- Define `OutboundSyncPlan`, `ClipboardSyncIntent`, `FileSyncIntent` in `uc-app`
- `OutboundSyncPlan` has optional `clipboard` field and `files` vec
- `ClipboardSyncIntent` carries the snapshot and file transfer mappings
- `FileSyncIntent` carries path, transfer_id, and filename per eligible file

### Planner design

- `OutboundSyncPlanner::plan(snapshot, origin, settings) -> OutboundSyncPlan` consolidates all eligibility logic:
  - Load settings once (not per-check)
  - Classify content type (once, before peer loop — Phase 25 decision)
  - Extract file paths (only for LocalCapture + file_sync_enabled)
  - Filter files by max_file_size
  - Determine clipboard sync eligibility (skip when all files excluded)
- Runtime becomes thin dispatch: plan → spawn clipboard sync if plan.clipboard.is_some() → spawn file sync for plan.files

### Redundant check removal

- Gradually remove Stage 2/3 defensive checks in `SyncOutboundClipboardUseCase` and `SyncOutboundFileUseCase` since the Plan already guarantees correctness
- Keep removal within this phase scope per Issue #279

### Testing approach

- Planner is unit-testable: mock settings + snapshot → assert plan output
- Cover edge cases: oversized files, file_sync disabled, mixed file sizes, all files excluded, non-LocalCapture origin

### Claude's Discretion

- Whether `plan()` is async (due to settings load) or takes pre-loaded settings as parameter
- Internal module organization within uc-app (new file vs existing module)
- Exact method signatures and builder patterns
- How much Stage 2/3 cleanup to include vs defer

</decisions>

<specifics>
## Specific Ideas

- Issue #279 provides the canonical design: https://github.com/UniClipboard/UniClipboard/issues/279
- The "all_files_excluded" guard and "file size pre-check" in runtime.rs lines 1210-1315 are the specific symptoms that motivated this refactoring
- Planner should make clipboard metadata and file transfer decisions atomically in the same place to prevent the class of bugs where copying an oversized file still syncs its thumbnail

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `classify_snapshot()` in `uc-core/src/settings/content_type_filter.rs`: Already classifies snapshots once — planner should call this
- `apply_sync_policy()` on `SyncOutboundClipboardUseCase`: Per-peer filtering with content type checks — logic to consolidate
- `apply_file_sync_policy()` in `uc-app/src/usecases/file_sync/sync_policy.rs`: Shared file sync peer filtering — logic to consolidate
- `extract_file_paths_from_snapshot()` in `runtime.rs`: File path extraction from clipboard snapshot
- `resolve_sync_settings()` in `uc-core/src/network/paired_device.rs`: Effective per-device settings resolution

### Established Patterns

- Use cases in `uc-app/src/usecases/` take Arc<dyn Port> dependencies via constructor
- Settings loaded via `SettingsPort::load().await` (async)
- Sync policy functions are standalone `pub async fn` in dedicated modules
- Tests use mock structs implementing port traits

### Integration Points

- `on_clipboard_changed()` in `runtime.rs` (lines 1140-1362): The caller that will use the planner
- `SyncOutboundClipboardUseCase::execute()`: Currently receives file_transfers from runtime — will receive from plan instead
- `SyncOutboundFileUseCase::execute()`: Currently re-checks file_sync_enabled and max_file_size — redundant after planner

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 35-extract-outboundsyncplanner-to-consolidate-scattered-sync-policy-checks_
_Context gathered: 2026-03-16_
