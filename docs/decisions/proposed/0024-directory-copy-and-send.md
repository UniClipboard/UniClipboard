# 0024: Directory copy and explicit send

- Status: Implemented; local process acceptance passed. Native and real-device acceptance pending.
- Date: 2026-11-12
- Execution plan: [0024 execution plan](../../exec/0024-directory-copy-and-send.md).
- Requirement: Send a directory once per copy action, with directory automatic sync disabled by default and explicit `send` available.
- Current behavior: [Pipeline architecture](../../architecture/pipeline.md), [desktop runtime](../../architecture/desktop-runtime.md), and [Engine contract](../../contracts/engine-contract.md).

## 1. Context

The desktop clipboard adapter can obtain selected paths. The local content importer accepts only regular files. Engine content and wire metadata do not represent directory trees.

Reading a directory can require a large traversal, many file opens, and substantial temporary storage. Disabling automatic network dispatch alone would still leave that work on every copy. Capture must therefore separate a cheap selection reference from preparation of transferable content.

## 2. Decision

Directory transfer is a one-shot clipboard operation. It does not establish a watched directory or propagate later edits, renames, or deletions. A later transfer requires another user action.

Add `sync.directories.automatic`, default `false`, through the existing configuration and settings path. Do not add a separate configuration layer.

| Action | Required behavior |
|---|---|
| Copy regular files only | Preserve existing file behavior. |
| Copy a selection containing a directory, automatic directory sync off | Record top-level selection references locally. Do not traverse, snapshot, or send the selection. |
| Explicit `send` of that selection | Scan, validate, prepare a private snapshot, and send the complete selection. |
| Copy a selection containing a directory, automatic directory sync on | Prepare and send once for that copy action, subject to automatic policies and limits. |
| Edit a previously copied source directory | Do not initiate another transfer. |
| Copy the same directory again | Recognize a new copy action, even if the top-level name and size are unchanged. |

A mixed selection of regular files and directories follows the directory policy as one batch. Do not automatically send only its regular files.

Explicit `send` bypasses automatic-dispatch restrictions, including the directory switch and automatic size threshold. It does not bypass the sync master switch, file permission, backend policy, device trust, transfer limits, or storage admission. Keep existing regular-file send semantics.

## 3. Deferred references and snapshot semantics

A copied directory reference records the top-level selection, not its descendants or frozen bytes. It must remain distinguishable from committed immutable content. A history entry that exposes this reference must identify it as a source reference. Do not present it as a copy-time snapshot or export source paths in ordinary history DTOs, events, or logs.

Explicit `send` reads the source during preparation. It cannot promise the bytes that existed at copy time. Check that references still resolve before scanning. A missing or replaced source must fail clearly instead of silently selecting another path.

Prepare one immutable private snapshot for each send operation. All selected targets read that snapshot. Subsequent sends from the deferred reference prepare new snapshots; they may contain newer source content. Re-sending a retained immutable snapshot sends its existing bytes and must be distinguishable from sending the source reference again.

Detect source changes during preparation and fail with `source_changed` when detected. Do not claim a filesystem-wide atomic snapshot of a directory that other programs are modifying. Reuse source identity checks and bounded copying; do not require platform filesystem snapshot facilities.

Snapshot bytes count toward storage admission, including temporary preparation. Keep memory bounded with streamed file reads and disk-backed storage. Cancellation, failure, and operation completion release owned temporary resources. A retained history snapshot follows the existing history ownership and cleanup rules.

## 4. Directory representation

Use a directory manifest plus file streams. Do not disguise a directory as an ordinary archive file.

The manifest identifies top-level selected items and their descendant entries. Each entry has a relative path and an explicit file or directory kind. File entries declare their byte sizes and bind to their payloads. Explicit directory entries preserve empty directories. Root identities distinguish independently selected roots with equal display names.

Transmit names and relative paths only. Source absolute paths and host references stay local. Keep relative paths out of routine telemetry and errors, as with filenames today.

Reuse existing content leases, pipeline processing, per-target operations, transport framing, storage accounting, and receive admission. Extend their data models where required. Keep protocol version 1. This unreleased project needs no compatibility reader or migration branch.

The manifest is content metadata, not an unbounded control message. Bound its encoded size and entry count. Its exact encoding and payload mapping belong in the contracts before implementation.

This follows the relative-path transfer pattern used by [LocalSend](https://github.com/localsend/localsend/blob/main/app/lib/provider/selection/selected_sending_files_provider.dart), with explicit directory entries to preserve empty directories.

## 5. Tree rules and receive publication

| Item or condition | Decision |
|---|---|
| Nested directories, empty directories, zero-byte files | Preserve. |
| Hidden files | Include. Do not implicitly apply `.gitignore`. |
| Symbolic links and directory junctions | Do not follow. Reject the selection with an explicit unsupported result until link transfer is supported. |
| Hard links | Copy as independent regular files. Do not preserve shared inode identity. |
| Devices, FIFOs, sockets, and other special entries | Reject the selection. Do not silently omit them. |
| Absolute paths, parent traversal, duplicate paths, or file/directory conflicts | Reject before publication. |
| Names invalid on the receiver, including case or normalization collisions | Reject. Do not silently rename descendants. |
| Existing destination with the same top-level name | Use an isolated receive-batch container. Never merge into or overwrite an existing tree. |
| A failed member of the selection | Fail the batch. Do not publish a partial successful selection. |

Never follow a received or concurrently substituted link outside the owned receive root. Use the existing root-confined filesystem approach. Treat sender metadata as untrusted even for a trusted device.

Link support varies across platforms. [Syncthing](https://docs.syncthing.net/users/faq.html#what-things-are-synced) also separates link transfer from following links. This decision chooses explicit rejection rather than accidental traversal or silent omission.

Admit the bounded manifest before accepting payloads. Verify received lengths and integrity before publishing history or Received. Stage content privately and publish only after the complete selection is ready. Failure must not expose a usable partial history item or replace the clipboard.

For default-cache receives, retain the complete tree under Engine ownership and protect it with leases. For custom destinations, preserve the existing user-managed ownership rule: never delete published custom content through history cleanup. Failed private staging is application-owned and must be reclaimed; do not sweep unrelated or published user content.

Apply the received selection by writing its local top-level file and directory paths to the platform clipboard. Keep the existing apply ticket, clipboard lease, current-observation check, and echo suppression. A newer local copy must not be overwritten by a late receive.

## 6. Bounds and user interaction

Apply the existing per-file transfer limit to every member. Also bound total selection bytes, entry count, directory depth, and manifest size. A zero-byte tree still consumes entries and metadata budget. Automatic size thresholds apply to the total file bytes of the entire selection.

Scan and snapshot outside the clipboard polling path and UI thread. Keep capture and receive application responsive. Check cancellation throughout traversal, preparation, and transfer. Report scanning, preparation, and sending as distinct stages. Do not report a percentage while the total is unknown.

Use the platform clipboard change sequence where available to detect another copy of the same directory. Do not use a recursive scan as the polling change detector. Keep this local observation marker separate from the cross-device content identity used for echo suppression. Platform fallback behavior must be specified and accepted explicitly; matching names and sizes alone does not establish a new copy action.

## 7. Responsibilities and alternatives

- Platform hosts read selections, access source paths, resolve host references, and write received paths to the system clipboard.
- Engine owns the directory content model, snapshot lifecycle, history semantics, policy, quotas, leases, and cleanup.
- Backends carry manifests and file streams without implementing separate directory business rules.
- The desktop exposes explicit `send`, stage progress, cancellation, and actionable failure states.

Reject eager traversal on every copy because it defeats the low-cost default. Reject a transport-only automatic switch because it still permits expensive preparation. Reject persistent directory watching because the requested action is a clipboard copy. Reject implicit archive conversion because it changes the content type and introduces extraction and duplicate-storage rules without removing the need for a directory model.

## 8. Contract and acceptance gate

Before implementation, specify exact DTOs, manifest encoding, resource-bound values, source-reference history states, overlapping root selection rules, portable filename validation, and platform copy-event detection. Update the Engine, history, pipeline, backend, and plugin contracts as affected. These details must preserve the decisions above.

Use real multi-process end-to-end scenarios as the acceptance mechanism. Cover directory-only and mixed selections, default no-traversal/no-send behavior, explicit send, enabled automatic send, repeated copies, empty directories, nested files, source changes, links, destination collisions, invalid paths, resource limits, cancellation, disconnects, cleanup, and ordinary-file regression.

Each run must produce a repeatable receipt bound to the source revision and binary hashes. Record the expected and received relative-path inventories, empty-directory lists, per-file hashes, policy assertions, and cleanup outcomes using synthetic fixtures. Do not retain personal source paths or content.

Native clipboard capture and cross-application directory paste require separate platform acceptance. A directory-backed test clipboard does not prove either behavior. No native application launch or cross-device run is authorized by this documentation change.

Implementation progress belongs in `docs/exec/`. After acceptance, land verified behavior in the pipeline and desktop runtime architecture pages and the affected contracts, then archive this decision. The execution receipt records implementation, accepted local process evidence, and remaining platform limits.
