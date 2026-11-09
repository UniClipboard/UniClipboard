---
paths:
  - app/desktop/internal/config
  - app/desktop/internal/api
  - app/desktop/internal/host
  - app/desktop/cmd/unic/cli.go
  - app/desktop/cmd/unic/history.go
  - app/desktop/cmd/unicd/daemon.go
  - engine/history.go
  - engine/history_cleanup.go
  - engine/file_cache.go
  - engine/restore.go
  - engine/content.go
  - engine/content_types.go
  - engine/options.go
  - engine/types.go
  - engine/state.go
  - engine/peerproc.go
  - engine/receive.go
  - internal/store/file_cache.go
verified: 85dbf35a5643f15a4a54eef062f4888e62308eec
---
# Desktop configuration and received files

The desktop daemon loads `config.toml` and exposes the effective saved configuration over its local socket.
CLI changes and file edits use the same validation and Engine settings.

## Settings

| Key | Default | Effect |
|---|---|---|
| `sync.enabled` | `true` | Pause content sends and receives. Keep connections and local history. |
| `sync.automatic` | `true` | Send new observed copies. Local history, manual sends, receives, and Actions remain independent. |
| `sync.on_restore` | `false` | After a successful explicit history restore, dispatch the restored content once. |
| `sync.files` | `true` | Permit file sends and receives, subject to each backend's policy. |
| `inbox` | Empty | Empty selects the default file cache. An absolute custom directory selects user-managed receives after restart. |
| `limits.max_file_bytes` | `"100MB"` | Bound each file transfer in both directions. The boundary is inclusive. |
| `file_cache.max_bytes` | `"500MB"` | Bound default-cache file bytes and pending file reservations. |
| `history.auto_cleanup` | `true` | Enable unified history and managed-file cleanup. |
| `history.retention_days` | `180` | Age from record creation; 0 disables the age limit. Applies to existing records. |
| `history.max_items` | `0` | Limit eligible records; 0 means unlimited. Excluded records do not count. |
| `history.excluded_tags` | `[]` | Skip any record with an exact matching tag, including its managed files. |
| `history.max_bytes` | `"4GiB"` | Startup history storage budget; a change requires restart. |

The existing size parser uses powers of 1024: 100 MB is 104857600 bytes.
A file-size limit governs transfer, not local reference import or local history.
Protocol hard limits still govern content import.
No setting changes sync frequency. No setting creates metadata-only transfers or on-demand downloads.
The existing backend automatic-size filter remains a separate backend policy.

## Configuration writes

`GET /v1/config` returns the saved typed configuration, its raw-file hash revision, errors, and restart requirements.
`PUT /v1/config` accepts `{revision,toml}` and rejects stale or observed external edits with HTTP 409.
The daemon validates the whole document and replaces the file from a private temporary file in the same directory.
A failed parse preserves the last valid typed configuration.
The revision tracks the latest observed disk document, including an invalid document, so a CLI apply can repair the file.

The saved file and runtime Engine state do not share a transaction.
`apply_error` reports a saved configuration that the host could not apply.
`restart_required` reports fields that need a daemon restart, including `inbox`.
An external editor does not share the daemon write mutex; a later external save remains authoritative through the file watcher.

## Restore

`unic history use` accepts a history ID or `@N`.
The command returns the local result separately from the network disposition and operation ID.
The CLI can supply `--request-id` for retry deduplication and `--json` for machine-readable output.
The host keeps up to 1024 restore results for 30 minutes per daemon instance.
A full result cache rejects new requests instead of forgetting a result early.
A restart ends that deduplication window. The application does not promise durable exactly-once delivery.

Engine authorizes resend with the last successful history apply token.
`SyncRestoredHistory` dispatches existing content without a new commit or captured processor call.
The stored processor prohibition still blocks resend.
Applying an Action result uses the local-only apply path and does not trigger a second send.
Global sync, global file permission, backend policy, limits, and trust still apply.
A network failure cannot undo the local clipboard write.

## File ownership

The desktop host supplies `DataDir/inbox/file-cache` through `Options.CacheDir`.
Engine manages only the private cache that the host provides.
Other hosts can supply a different app-private directory without exposing a user-selected destination to Engine.
Its SQLite ledger records payload identity, size, completion time, and availability.
Engine journals ownership before creating each payload directory.
Default-cache history, Received, and content leases share the retained payload.
Unified cleanup removes eligible history records and releases their managed payloads.
Capacity reclamation uses the same deletion path and tag exclusions.
If automatic cleanup is off or eligible records cannot free enough bytes, default-cache admission fails.
`parts[].available=false` reports unavailable default-cache content.
Reading or restoring unavailable content returns `resource_expired`.

Custom receive files are user-managed.
The desktop host validates the configured destination and supplies `Options.StoreReceivedFile`.
Engine calls that callback with `ReceivedFile{Name, MIME, Size, Reader}` after processing the received content.
The callback consumes the stream and returns a readable `PersistentFileSource`.
The desktop host creates the file exclusively and owns its destination, permissions, and reference format.
Engine validates the source and stores its persistent reference without interpreting the reference.
A callback failure publishes no received item or history and does not fall back to the default cache.
The callback runs without Engine state, history, or content locks and must honor its context.
The default-cache quota does not count custom receives or reject them because the default cache is full.
Transfer limits, transient transfer resources, and disk-write errors still apply.
Custom finalization never silently overwrites an existing file.
The desktop host uses an opaque `desktop-received:` reference for custom received files.
`ResolveFileRef` resolves that reference after restart.
`FileReference` returns a checked host reference under a content lease.
`LocalFilePath` returns paths only for Engine-owned files and reports `unsupported` for host sources.
Only the desktop host decodes references for system clipboard writes.
Ordinary history responses, cache inventory, logs, and events contain no host references.
A partial custom destination from failed finalization remains for the user to inspect.

## Cleanup invariants

- Delete retained file content only when the default-cache ledger proves ownership and the open root confines the deletion.
- Use `os.Root` for cache publication and deletion. Refuse cleanup when the default-root identity changes.
- Reject a symlink as a managed root. Never follow a payload symlink into an external directory.
- Never scan a custom receive directory for old files, orphan files, or filename prefixes.
- Never unlink a custom received file on history deletion, history expiry, lease release, restart, or shutdown.
- Never enroll an earlier custom received file in cleanup after changing the active location.
- Keep payloads held by reads, sends, applies, or the clipboard.
- Preserve all startup clipboard candidates until the host reads the clipboard and establishes exact default-cache pins.
- Refuse new default-cache file admission until that first reconciliation completes.
- Run bounded age/count cleanup after startup reconciliation, policy changes, commits, and every 30 seconds while running. Each pass deletes at most 256 eligible records.
- Remove independent file-retention settings and payload-only eviction. One switch governs age/count cleanup and capacity-driven history deletion.
- Keep excluded records and their managed files. Manual deletion and explicit per-item expiry remain available independently.
- Delete logical records first. Reclaim physical payloads only after the final history reference and content/clipboard holder are gone.
- Keep failed-deletion bytes charged until physical deletion and ledger removal succeed.
- Do not run cache cleanup in offline migration mode.

The daemon no longer scans `clip-*` paths in the configured receive directory.
Temporary receive staging stays under the daemon's application storage.
Cleanup does not enumerate unknown default-cache directories merely because their names resemble generated IDs.

## Code map and validation

- `config/manager.go`: serialized saves, raw-file revision, watcher, and restart status.
- `cmd/unic/cli.go`: configuration commands and cache inventory.
- `cmd/unic/history.go`: explicit history restore and retry key.
- `engine/restore.go`: token-authorized resend without a new commit.
- `engine/file_cache.go`: private-cache ownership, quota reservation, pins, orphan reclamation, and host callback validation.
- `engine/history_cleanup.go`: unified age/count cleanup, tag exclusions, and capacity-driven record deletion.
- `host/received_files.go`: desktop destination validation, custom file writing, and reference encoding/resolution.
- `engine/content.go`: separate access to Engine-owned file paths and opaque host references.
- `engine/history_import.go`: the existing directory-sync helper used by cache finalization.
- `internal/store/file_cache.go`: durable ownership and availability transitions.
- [Pipeline](pipeline.md), [Engine contract](../contracts/engine-contract.md), and [history contract](../contracts/history-contract.md).

CLI E2E uses real paired daemons and directory clipboards.
See the [execution receipt](../exec/0016-sync-and-file-settings.md) for exact verification and remaining limits.

## Gaps

The settings GUI does not expose these controls yet.
Native clipboard and folder-picker interaction acceptance remains with the user.
Crash injection at every filesystem/database instruction and forced disk-full/unlink-failure acceptance are not complete.
Unknown temporary leftovers are not deleted through filename scans.

## Cleanup settings interface

The desktop Settings tab reads `GET /v1/config` and `GET /v1/history/tags`.
The tag endpoint returns the persisted tag-name list, including unassociated imported tags.
It contains no content or file references.
Settings save through the existing revision-checked `PUT /v1/config` path.
The page provides the unified switch, retention time, count limit, excluded-tag checkboxes, and per-device cache quota.
Configured values outside the preset lists remain selectable. The quota control remains enabled when cleanup is off.
Explicit clipboard writes release a replaced file lease after importing the new snapshot; otherwise rebasing would hide that release from polling.
Native visual acceptance remains separate from CLI acceptance.
