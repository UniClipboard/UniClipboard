# 0017: Automatic history cleanup

- Status: Implemented; CLI acceptance passed (16 cases). Native visual acceptance pending.
- Date: 2026-11-09
- Input: User-provided History Retention settings screenshot and subsequent corrections for tag exclusions and coupled file/history deletion.
- Execution plan: [0017 execution plan](../../exec/0017-history-auto-cleanup.md).
- Current behavior: [History contract](../../contracts/history-contract.md) and [pipeline architecture](../../architecture/pipeline.md).

## 1. Requested scope

Add four local history settings. The screenshot supplies the first three displayed values below.
The user replaced the fourth setting with a list of tags excluded from cleanup.
The user also requires removal of the separate file-cleanup logic and settings. This task replaces them with unified automatic cleanup.
The implementation uses On, 180 days, Unlimited, and an empty excluded-tag list as defaults.

| Setting | Screenshot value | Required behavior |
|---|---|---|
| Automatic history cleanup | On | Enable scheduled cleanup under the retention rules. |
| History retention time | 180 days | Remove history records older than the selected age. |
| Maximum history items | Unlimited | Limit retained records, or disable the user count limit. |
| Tags excluded from cleanup | Not specified | Skip records with any selected tag during automatic history cleanup. |

When cleanup is off, retain the selected rules and disable their controls.
Re-enabling cleanup applies the saved rules to existing and future records.
Deletion is local. It does not remove another device's history or change the system clipboard.
The interface offers 7/30/90/180/365 days or unlimited age, and 100/500/1000/5000/10000 items or unlimited count.
Existing configured values outside these presets remain selectable.

## 2. One lifecycle for managed files and their history

The user requires file content and its bound history to be deleted together.
For application-managed files, history deletion and file cleanup must use one deletion path.
History age/count rules govern records of all content types through this path.
Remove the standalone file-age cleanup path and its startup cleanup branch.
Remove `file_cache.retention_hours` and `file_cache.auto_cleanup` from configuration, SDK settings,
API responses, defaults, validation, templates, CLI/GUI controls, and documentation.
Do not retain aliases, compatibility readers, or a second file-cleanup switch.
Retain `file_cache.max_bytes` as a storage admission bound, not an independent payload-eviction policy.
Route required capacity reclamation through the unified record deletion path and tag exclusions.
If cleanup is disabled or no eligible record can free enough space, reject admission rather than bypassing the policy.
File cleanup must not leave a history record whose managed payload it has removed.
Deleting file history must release its managed payload for physical deletion when no holder remains.

This replaces the earlier payload-only eviction behavior described in [file settings](0016-sync-and-file-settings.md).
The separate settings and payload-only eviction path have been removed.
The related parts of 0016 now point to this decision.
Tag exclusions must protect both the record and its managed files from automatic cleanup.

Before this change, HistoryOptions supplied a 10,000-record limit, a byte budget, and DefaultTTL.
DefaultTTL only supplied a deadline at commit. That model could not apply age changes to existing records
or exclude tagged records. Live Settings.History now owns age/count cleanup; HistoryOptions retains the byte budget.

Use one Engine-owned cleanup policy and deletion path for history and managed files. The desktop host persists settings and exposes them
through the existing configuration path. The GUI uses the daemon API.
Reuse the existing scheduler, SQLite transactions, content references, and UI Kit.
Do not add a second cleanup worker or a new storage layer without a demonstrated need.

## 3. Implemented policy semantics

The history and Engine contracts now specify these rules.

1. Measure retention from the stored creation time. Reading or restoring a record does not reset its age.
2. Apply age cleanup first. Then enforce the count limit by removing the oldest eligible records.
3. Exclude records with any configured tag from age cleanup, count cleanup, and the count being limited.
4. Reuse existing history tags. Match tag names exactly using the existing tag normalization rules. An empty exclusion list excludes no records. Do not add favorite metadata or a reserved tag.
   Support multiple selected tags. A record needs only one matching tag to be excluded.
5. Recheck record tags and the exclusion list at the deletion commit point. Publish normal history change events after commit.
6. Keep explicit per-item expiry separate. Turning off automatic cleanup does not cancel a deadline set by a user or processor.
7. Keep storage safety budgets separate. Unlimited means no user count limit; it does not mean unlimited storage.
8. Storage pressure must not silently bypass tag exclusions. If protected records prevent admission, reject the new content with `resource_exhausted`.
9. Manual deletion remains available for excluded records. Tag exclusions apply to automatic history and managed-file deletion.
10. Existing content holders remain valid after record deletion. Delete owned payloads only after all references are released.
11. Never delete source files or files in a custom receive directory through history cleanup.
12. Unified cleanup and any permitted capacity reclamation must delete the bound history records in the same logical deletion transaction. A history record with multiple managed files must not be left partially available by cleanup.
    Release all of its managed-file references. Reclaim physical files only when no valid reference remains.
    A shared payload referenced by an excluded history record is not eligible for automatic eviction.
    Active reads, sends, and applies may delay physical reclamation; never count their bytes as already freed.
13. Apply policy changes without restart. Run a bounded cleanup pass at startup, after a policy change, and during normal maintenance.

The interface must explain that explicit expiry and storage budgets remain active when automatic cleanup is off.
It must explain that automatic cleanup removes managed files and their bound history together.
The custom-directory and source-file ownership boundary remains unchanged.

## 4. Resolved scope

- Keep the per-device cache quota. Default: 500 MB.
- Remove independent file-age and file-cleanup settings.
- Use one automatic-cleanup switch for age/count deletion and capacity-driven history deletion.
- Exclude records with any selected tag from automatic cleanup and the count limit.
- Keep explicit expiry and manual deletion independent of tag exclusions.
- Reject new file admission if cleanup is off or protected/held content prevents sufficient reclamation.

## 5. Completion boundary

The implementation removes the superseded file-cleanup logic and settings and updates the history and Engine contracts, file-cache lifecycle, configuration documentation,
and pipeline architecture. Keep protocol version 1.
No migration or compatibility layer is required for this unreleased project.
Completion requires the end-to-end evidence described in the execution plan.
