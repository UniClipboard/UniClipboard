---
paths:
  - engine/directory.go
  - internal/directory
  - internal/localcontent/directory.go
  - engine/pipeline.go
  - engine/types.go
  - engine/peersend.go
  - engine/pluginbackend.go
  - engine/product_events.go
  - engine/transfer.go
  - engine/submission.go
  - engine/backend.go
  - engine/receive.go
  - engine/clipboard.go
  - engine/action.go
  - engine/history.go
  - engine/history_search.go
  - engine/history_search_types.go
  - engine/history_cleanup.go
  - engine/file_cache.go
  - engine/restore.go
  - engine/history_sync.go
  - engine/history_types.go
  - engine/history_import.go
  - engine/plugins.go
  - engine/observers.go
  - plugin/plugin.go
  - internal/store/history.go
  - internal/store/search.go
  - internal/store/store.go
  - internal/plugins/builtintext
  - internal/plugins/builtinclassify
  - plugin/tags.go
verified: e3ff9f4792d16e7499e8266568edf6a9ec873b30
---
# Clipboard pipeline and local history

Engine owns processing, commit, synchronization, local history and user-triggered Actions.

## Current behavior

The stage chain is Capture/Receive → Processing → Commit → Sync → Use → End.

- **Entry points**: `ObserveClipboard` uses origin `local_system` and a reserved observation request key. Baseline, echo, unchanged and empty observations do not enter processing. Explicit `CommitContent`/`SubmitContent` and `RunAction` use origins `explicit` and `action`. These entries share `startSubmissionLocked` and `runSubmission`. Remote receipt uses `delivered` without a submission record.
- **Processing**: Registered built-in/Go processors run in assembly order, followed by configured Lua processors. Local entries emit `clipboard.captured`; receipt emits `clipboard.received`. Processors can stage tags, persistence, sync and expiry through `processingContext`. They cannot change content. Each call has a 200 ms budget including its shared per-instance serial gate. Errors, timeout, panic or an invalid mutation discard that call's changes. A failed `on_error=block_sync` processor disables sync for all later processors. Persistence and sync default to true. Exact limits are in the [plugin contract](../contracts/plugin-contract.md).
- **Commit**: With persistence enabled, validate size, reserve storage and persist blobs before publishing history and deduplication. Capacity reclamation follows unified cleanup eligibility. Bounded age/count cleanup preserves the newly committed record. Notifications and sync dispatch follow successful commit. Sending failure does not undo history.
- **Sync**: `dispatchProcessed` checks global and item switches, automatic-sync settings, backend type policy, automatic file thresholds and transfer limits. Only explicit submissions bypass the automatic file threshold. Backends run in ID order and share one Operation. History stores per-backend sync summaries when a backend queues or fails. Restart marks pending summaries interrupted without resending.
- **Device selection**: `CommitContent.DeviceIDs=nil` selects all eligible targets. A nonempty selection is sorted before request-key construction. Empty, repeated, unknown or revoked IDs return `invalid_argument`. Peer-session backends filter their live targets by the selection. Non-peer backends skip selected-device submissions. Validation loads device records; actual sending still requires an eligible live peer session.
- **Receive**: Admission checks part sizes, policy, deduplication and storage before body delivery. Processing sees only accepted parts. An empty filtered result is dropped without successful deduplication. Received content is never forwarded. Reserve a Received slot before writing history and successful deduplication. The final publication rechecks lifecycle, sync policy and source authorization. Failure removes the new history and deduplication record.
- **Temporary receive**: `AdmitTransfer` reserves the shared receive path under backend `unic.transfer` and pseudo-channel `transfer`. It creates no stored device or channel. Ephemeral receipts bypass device/channel authorization, but still require active lifecycle, enabled sync, receive policy and storage admission. They use origin `external`, run received processors and do not request automatic clipboard application.
- **Use**: `BeginApply` and `BeginApplyHistory` apply content without a new commit. History tickets freeze the stored origin classification for `history_applied`; the event requires successful application with `UserRestore=true`. Desktop can call `SyncRestoredHistory` after explicit restore when `sync.on_restore` is enabled. Action-result application remains local. See [desktop runtime](desktop-runtime.md).
- **Actions**: User-triggered `ListActions`/`RunAction` operate on text history. A plugin action must produce one nonempty UTF-8 text result. The new `action` record forces persistence and passes captured processors and normal sync decisions. The source remains unchanged. Action execution shares the processor instance gate and has a 10-second budget. Request IDs share the submission namespace. Plugin failures return opaque errors. The host applies the new history item. `builtin.text` provides upper/lower/trim/plain and email/URL tagging. Service routes are `GET /v1/actions` and `POST /v1/history/{id}/actions/{key}`.
- **History**: SQLite records and private blobs belong to Engine. Host-managed files remain external references. Stable IDs, pagination, edits, leases, expiry and apply follow the [history contract](../contracts/history-contract.md). Cleanup defaults to enabled, 180 days, unlimited eligible records and no excluded tags. The startup history byte budget is 4 GiB. Live history settings affect existing records. Tag exclusions protect records and managed files. Explicit item expiry is independent. The service exposes `/v1/history*` and `unic history`.
- **Notifications**: Observers receive content-free events and cannot mutate content. Each observer has a 64-entry queue that drops the newest event when full and a 200 ms callback budget. Delivery is lossy. Successful capacity eviction emits `clipboard.deleted` once per deleted history ID.

## Built-in content tags

The service assembles `builtin.classify` before `builtin.text` by default.
The ordinary processor adds `builtin:text`, `builtin:image` or `builtin:files` from the Engine item type on capture, explicit commit, Action results and receipt.
Text containing an HTTP(S) URL also receives `builtin:link`; inspection is limited to the first 64 KiB.
It changes no content and performs no network or file-body reads.
Existing history is not backfilled. Disabling `plugins.instances."builtin.classify".enabled` takes effect after service restart and stops classification of new items; existing tags remain.
The existing `builtin.text` email/URL hints and text Actions remain independent.
Tag IDs remain language-independent through storage, filtering and cleanup settings. Desktop localizes only the four built-in labels, protects their management operations and always lists them, including when history is empty. Custom tag labels remain unchanged. Built-in sidebar visibility survives refresh within the current desktop session.

## Invariants

- Processors and observers cannot change content. User-triggered Actions create new content.
- A request ID commits at most once. Reserved `obs:` and `recv:` prefixes belong to Engine. Cached failures remain idempotent; changed parameters return conflict.
- Network dispatch follows successful commit. Send failure never rolls back history or reruns processing.
- Disabled sync, no backend or no target does not prevent local persistence when the processor permits it.
- Stop/suspend and commit are ordered by `commitGate`; a completed lifecycle barrier prevents late commit.
- Committed content is immutable. Tags and expiry remain editable. Delete/expiry hides records immediately while existing leases and operations remain valid.
- Receive publication keeps history, successful deduplication and the Received slot consistent. A nonpersistent receipt still stores deduplication.
- Zero-byte representations are invalid. A nonempty set of zero-byte regular files is valid.
- History and Received hold independent content references. History and `MaxStoredBytes` share storage accounting; external file references account for zero owned bytes.
- Events, logs, errors and submission results contain no content, paths, filenames or raw plugin errors.
- Changed or moved external files return `source_changed`. Engine must not replace or delete user source files.

## Sync settings and file ownership

`sync.automatic=false` suppresses clipboard-origin dispatch after local commit.
`sync.files` restricts file policy in both directions.
`limits.max_file_bytes` defaults to 100 MB and bounds transfer; source references can still enter local history.
History preserves processor sync prohibition for later restore sends.
Received files use the default-cache ledger or a user-managed external reference.
The per-device cache quota applies only to default-cache payloads.
Unified age/count cleanup and capacity reclamation delete eligible records and release their managed files, with the same tag exclusions.
There is no separate file-retention or payload-only eviction path.
History deletion and expiry preserve custom received files.
See [desktop runtime](desktop-runtime.md) for host storage boundaries.

## Receive boundaries and diagnostics

Every adapter supplies part sizes before shared admission. External channels supply actual byte lengths.
Admission requires the declared total to equal all part sizes without overflow.
Engine holds the profile lock before removing temporary `unic-content-*` and `unic-recv` directories from InboxDir at startup.
Cleanup uses an opened directory root and does not follow symlinks. Other entries and retained cache payloads remain intact.
Cache-backed files survive receipt dismissal while history or a lease owns them.
Import uses the protocol file cap; transfer uses the configured file cap.
A backend with files disabled rejects a file-bearing batch without text fallback.

Engine emits fixed-field diagnostics independently of UI subscriptions.
Commit, queue, send, receive and apply outcomes are separate measurements.
Idempotent waits do not repeat commit metrics. Send success does not prove remote receive or apply.
Callback failure does not change business results. The host owns bounded asynchronous output.
See [observability](observability.md) and its [contract](../contracts/observability-contract.md).

## Code map

| Path | Responsibility |
|---|---|
| `engine/plugins.go`, `pipeline.go` | Processor/Action registration, staged policy, gate, timeout and rollback |
| `engine/submission.go`, `types.go` | Submission API, device selection, idempotency and results |
| `engine/backend.go`, `peersend.go`, `pluginbackend.go` | Backend dispatch, target filtering and Operation aggregation |
| `engine/receive.go`, `transfer.go` | Receive admission/publication, temporary-transfer adapter and Received ownership |
| `engine/clipboard.go`, `product_events.go` | Observation/apply protocol and finite product events |
| `engine/action.go`, `internal/plugins/builtintext` | User-triggered text actions and built-in processor |
| `engine/history.go`, `history_types.go`, `history_sync.go`, `restore.go` | History API, source classification, sync summaries and restore sending |
| `engine/history_cleanup.go`, `file_cache.go`, `history_import.go` | Cleanup eligibility, managed-file ownership and import |
| `engine/observers.go` | Bounded read-only notification dispatch |
| `internal/store/history.go` | History and deduplication storage |

Service routes live in `app/service/internal/server/`; CLI handling lives in `app/service/cmd/unic/history.go`. Shared clients and DTOs live in `internal/localapi/`.

## Specifications

- [Pipeline contract](../contracts/pipeline-contract.md), [history contract](../contracts/history-contract.md), [Action contract](../contracts/clipboard-action-contract.md).
- [Engine contract](../contracts/engine-contract.md), [SDK extension contract](../contracts/sdk-extension-contract.md).
- [Plugin contract](../contracts/plugin-contract.md), [sync backend contract](../contracts/sync-backend-contract.md).

## Gaps

- GitHub plugin installation (`unic plugin install`) is unavailable.
- Two-host CLI acceptance covers the recorded source snapshot. Native clipboard acceptance remains pending. See the [physical-host receipt](../exec/baselines/0004-2026-11-10-cli-twohost.md).
- The desktop GUI Action menu awaits the mygo/UI Kit interface design.
- Nonempty `CommitRequest.WritebackGuard` returns `unsupported`; automatic writeback is cancelled.

## Explicit history search

Engine persists derived search documents and readiness state with each history record in the same transaction.
Both normal pipeline commits and offline imports use this path. Missing derived state is repaired from persisted representations and basenames on open.
Documents contain committed text or an HTML-text fallback and individual file basenames. Search does not open file bodies, walk directories, fetch URLs or perform OCR.
A case-sensitive FTS5 trigram index accelerates normalized substring matching; canonical `instr` checks decide membership. One- and two-rune terms use exact substring scans.
Delete cascades remove search documents and their FTS postings. Query-time expiry prevents stale hits before physical cleanup.
A separate bounded SQLite reader serves search/count/catalogue work without holding the existing writer connection.
Index quota or extraction failure preserves history and reports an incomplete index for content-dependent queries. Ordinary metadata browsing remains available.
The [search contract](../contracts/history-search-contract.md) defines the public methods and resource limits. The [execution receipt](../exec/desktop-search-implementation.md) separates process evidence from native UI acceptance.

## Directory references and one-shot transfer

Copying a selection containing a directory captures only top-level private source references by default. It does not enumerate descendants, create a byte snapshot, or dispatch a regular-file subset. `sync.directories.automatic` defaults to false. Source edits never initiate a transfer. macOS and Windows adapters supply local copy markers to distinguish another copy of the same directory; Linux currently cannot distinguish identical repeated selections and requires explicit send for that case.

Explicit send prepares current directory content in a bounded background operation. The captured reference and completed immutable snapshot have separate history kinds, `directory_reference` and `directory`. Every target of one operation reads the same snapshot. Directory capture returns the transitional observation disposition `preparing` while Engine completes local processing and baseline reconciliation. Ordinary capture remains unchanged.

The tree is a bounded manifest representation plus ordinary file streams. Empty directories and hidden files are preserved. Links, special entries, overlapping roots, unsafe names and unsupported trees fail the batch. File policy governs the entire selection, including an empty directory. The receiver validates manifest structure before member payloads and verifies member hashes before history/Received publication. Source references never enter a backend. Exact bounds and interfaces are in the [directory contract](../contracts/directory-contract.md).

Directory snapshots use ordinary history, content leases and cache accounting. Default-cache clipboard views reuse payload bytes through root-confined hard links where supported; additional copies require a storage reservation. The final holder releases each temporary view. Published custom trees belong to the user. Explicit history send preserves an existing processor sync prohibition, prepares references anew, and resends snapshots without reading the original source.

A newer local copy cancels unfinished automatic directory preparation so that an old slow tree cannot arrive after the newer copy. Explicit source sends continue independently of clipboard changes. Background transmission uses the submission-owned lifetime when present; history-restore RPC completion does not cancel its queued transmission.

Directory references and snapshots receive the existing `builtin:files` classification. They do not introduce a separate built-in tag.
