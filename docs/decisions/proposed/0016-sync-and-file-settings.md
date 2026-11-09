# 0016: Sync and file settings

- Status: Sync and quota CLI/TOML implemented. Separate file retention and cleanup are superseded by [0017](0017-history-auto-cleanup.md).
- Current behavior: [desktop runtime](../../architecture/desktop-runtime.md).
- Date: 2026-11-09
- Input: The settings screenshot and the subsequent scope review.
- Research and execution plan: [0016 execution plan](../../exec/0016-sync-and-file-settings.md).

## 1. Scope

The user requested nine settings. The user removed the immediate-transfer threshold.
The default maximum file size is 100 MB. Sync frequency is out of scope.
Files that pass admission transfer in full. This proposal adds no metadata-only delivery or on-demand download.

| ID | Setting | Default |
|---|---|---|
| S1 | Enable sync | On |
| S2 | Automatic sync | On |
| S3 | Sync on history restore | Off |
| S4 | Enable file sync | On |
| S5 | File storage location | Application-managed directory |
| S6 | Maximum file size | 100 MB |
| S7 | Per-device cache quota | 500 MB |
| S8 | Separate file retention | Removed; unified in 0017 |
| S9 | Separate file cleanup switch | Removed; unified in 0017 |

The original nine-setting scope is revised by 0017. S7 remains; S8 and S9 are removed.
The user confirmed the storage boundary: quota and cleanup cover only the default managed file directory.
A custom receive directory is user-managed. The application must not delete files there through cache or history lifecycle operations.
This draft proposes the remaining acceptance rules below for review.

## 2. User stories and acceptance criteria

### S1: Pause content sync

As a user, I want to pause content sync so that this device neither sends nor accepts new content.

1. When I turn sync off, the application keeps device connections and trust records.
2. The application continues to capture local history.
3. The application blocks automatic sends, manual sends, and restore sends.
4. The application cancels content transfers and blocks pending remote clipboard writes.
5. I can still restore an available history item to the local clipboard.
6. When I turn sync on, the application sends only new changes. It does not replay copies made during the pause.

### S2: Choose automatic or manual sending

As a user, I want to stop automatic sending while I continue to receive content and send selected content manually.

1. When automatic sync is off, a new local copy still enters local history.
2. The application does not send that copy automatically.
3. Manual sends and remote receives remain available when S1 permits them.
4. When I turn automatic sync on, the application does not send earlier local copies.
5. This setting controls clipboard observation. It does not disable user-triggered Actions or S3.

### S3: Share a restored history item

As a user, I want to choose whether a history restore also updates my other devices.

1. With the default setting, restore changes only the local clipboard.
2. When the setting is on, the application sends the restored content only after a successful local apply report.
3. A failed or conflicting local apply does not send content.
4. One restore request produces at most one send. Clipboard observation does not echo that send.
5. Restore does not create a duplicate local history item or rerun captured processors.
6. S1, S4, S6, backend policy, and device trust still govern the send.
7. S2 does not suppress a restore send. Restore is a user action.
8. A network failure does not undo a successful local restore. The application reports the local result and the send result separately.
9. No online target means local success and no queued retry after reconnect.
10. Applying an Action result does not trigger a second send through this setting.
11. The original processor sync prohibition requires an explicit permission rule before implementation. See the execution plan.

### S4: Exclude files from sync

As a user, I want to disable file sync while I continue to sync text and images.

1. When file sync is off, the application sends no file payload and accepts no new file payload.
2. The application keeps existing local history and cached files.
3. Manual file sends and restore file sends cannot bypass the setting.
4. Backend file policies cannot override this global restriction.
5. The application does not treat a file-manager text representation as a substitute file transfer.

### S5: Select a file storage location

As a user, I want to choose where the application stores received files.

1. The application shows the default managed location when I have not selected a directory.
2. I can select an existing writable directory or restore the default.
3. The default directory is application-managed. A custom receive directory is user-managed and grants no cleanup authority.
4. A location change affects future received files after a daemon restart. The interface shows that requirement.
5. Existing files keep their storage class and recorded location. Only default-cache files can become eligible for cache eviction or expiry.
6. The application never deletes custom received files, unrelated files, local source files, or user-exported copies.
7. A missing or unwritable location produces a visible failure. The application does not silently choose another location.
8. History deletion, history expiry, lease release, restart, and quota pressure must not delete custom received files.
9. Switching back to the default does not enroll earlier custom received files in cache cleanup.
10. The page shows that quota, retention, and automatic cleanup do not apply to the custom directory.

### S6: Bound each file transfer

As a user, I want a 100 MB default file limit so that one file cannot cause an unexpectedly large transfer.

1. The application allows a file of exactly the configured size. The application rejects a file one byte larger.
2. The limit governs both sending and receiving. Manual and restore sends use the same limit.
3. If one file exceeds the limit, the application rejects the whole file batch. It does not publish a partial batch.
4. A rejected automatic send does not remove the local source or its local history reference.
5. The application rejects a receive before publication and removes incomplete owned data.
6. The interface reports the limit failure without logging the filename or path.

### S7: Bound this device's file cache

As a user, I want a 500 MB quota for the default managed file directory so that the application bounds its file cache.

1. The quota is local to this device. The quota does not give each sender a separate allowance.
2. The quota counts only default-cache file payloads and their transfer reservations. History references to these payloads do not expand the directory boundary.
3. Shared payloads count once. Custom received files, local source references, text, images, and exported copies do not count.
4. Before a default-cache receive, the application reserves the whole batch and uses unified history deletion to reclaim eligible managed files when automatic cleanup permits it.
5. The application does not evict payloads held by an active read, send, apply, or clipboard lease.
6. If the quota cannot fit a default-cache batch, the application rejects that receive without publishing partial history or clipboard content.
7. Reclamation deletes eligible history and releases its managed files. Tag exclusions apply to both.
8. The quota remains an admission bound when unified automatic cleanup is off. Reject the new batch instead of deleting history.
9. The application accepts custom-directory receives independently of the default-cache quota. File-size and transfer resource limits still apply.
10. The application must not copy custom received files into the default cache merely to apply quota or cleanup.

### S8 and S9: Superseded

Separate file retention and startup cleanup are removed.
[0017](0017-history-auto-cleanup.md) owns one cleanup policy for all history types and their managed files.
No file-retention setting, independent file-cleanup switch, or payload-only eviction remains.
Ownership checks, active holders, quota reservations, and interrupted-deletion recovery remain necessary.

### S10: Save settings reliably

As a user, I want the settings page to show saved values and failures accurately.
This is a cross-cutting story, not a tenth configuration item.

1. The page contains the remaining sync/file settings. Unified history cleanup controls belong to 0017.
2. The page contains no sync-frequency or immediate-transfer-threshold row.
3. A successful save survives daemon and GUI restart.
4. An invalid value leaves the last valid configuration active.
5. A conflicting save asks me to reload current values. It does not overwrite another edit.
6. Dependent controls show why sync is unavailable without resetting their saved values.
7. Keyboard focus, accessible names, narrow layouts, and folder-picker cancellation remain usable.

## 3. Proposed decisions and rationale

- Keep policy and cache ownership in Engine. A GUI cannot enforce receive admission or headless resource limits.
- Keep TOML persistence, the folder picker, system clipboard access, and user-selected receive destinations in the desktop host.
- Supply private cache storage with `Options.CacheDir`. Engine retains common quota, expiry, and ownership rules.
- Use a received-file stream callback for host-managed saves. Reuse `PersistentFileSource` for returned files.
- Keep host references opaque. An Android host can supply document references and streams without requiring a filesystem receive directory.
- Separate `FileReference` from `LocalFilePath`. Only the host interprets a platform reference.
- Reuse SQLite, the existing content leases, the blob holder pattern, and the UI Kit. Add no settings framework.
- Use the unified history/file lifecycle from 0017. Retain physical files only while a valid reference or active holder requires them.
- Use one cache identity for a received payload. History and the clipboard share that payload instead of keeping separately charged copies.
- Limit deletion authority to proven owned content inside the default managed file directory.
- Record default-cache and custom-received storage classes separately. A path change must not change a file's storage class.
- Store custom received files as persistent external references in history. History lifetime grants no authority to delete the referenced file.
- Resolve and check the default directory boundary before deletion. Reject symlink escapes and directory replacement.
- Retain the current restart boundary for the storage location. This is a supported behavior, not an interim migration mechanism.
- Keep the existing backend automatic-size policy as a separate backend capability. Do not expose it as an immediate-transfer setting.
- Do not add remote recovery, offline queues, or automatic retries after reconnect.

## 4. Details awaiting review

These choices are recommendations, not additional user approvals already received.

| Topic | Proposed rule |
|---|---|
| Unit | Preserve the existing parser: the UI label MB means 1,048,576 bytes. Show the definition in help text. |
| Configurable ranges | File size: 1 through 2048 MB. Cache: 1 through 8192 MB. Retention: 1 through 876000 hours. Reject overflow and non-integers. |
| Quota smaller than a batch | For default-cache receives, reject a batch that cannot fit and show the required space. Custom receives ignore this quota. |
| Reduced quota | Apply only to default-cache payloads. Keep protected payloads, stop new default-cache reservations, and evict eligible payloads. |
| File limit and local history | Make the user setting a transfer limit. Keep protocol hard limits on content import and local history. |
| Cleanup schedule | Startup only, as described in the screenshot. No periodic age cleanup in this scope. |
| Directory changes | Require daemon restart. Preserve each file's storage class. Never enroll custom directories in a cleanup registry. |
| Restore retry | Use an idempotent restore request. Retrying the same request must not send twice or overwrite a newer clipboard. |
| Mixed clipboard content | If a clipboard operation contains files, reject the whole file batch when the file policy blocks files or one file is too large. |

## 5. Landing conditions

Update the architecture and contracts in the implementation change.
Run their required verified-marker updates only after behavior exists.
Archive this decision only after the execution plan has passed its acceptance gates.
