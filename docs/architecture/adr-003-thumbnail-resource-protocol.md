# ADR-003: Thumbnail Resource Protocol for Preview Thumbnails

## 状态

已被 Engine 的按条目读取缩略图契约取代。当前 Desktop HTTP 契约见
[按历史条目读取缩略图](entry-thumbnail-api.md)。下文保留原决策，不代表当前实现。

## Context

Thumbnail bytes are generated and stored as blobs, but the thumbnail blob id is only
persisted in `clipboard_representation_thumbnail`. The existing `uc://blob/{blob_id}`
handler resolves through `resolve_blob_resource`, which depends on
`clipboard_snapshot_representation` and fails to find thumbnail blob ids. As a result,
`thumbnail_url` values like `uc://blob/{thumbnail_blob_id}` cannot be resolved, even
though the thumbnail blob bytes exist. Meanwhile, expand flows work because they use
preview representation `blob_id` and `get_clipboard_entry_resource`.

We need a dedicated access path for thumbnail bytes that does not depend on
representation lookups by thumbnail blob id, while keeping list previews lightweight.

## Decision

Introduce a thumbnail-specific resource protocol:

- Add a `ResolveThumbnailResourceUseCase` that looks up thumbnail metadata by
  `representation_id` through `ThumbnailRepositoryPort`, then reads bytes via
  `BlobStorePort`.
- Extend the `uc://` protocol handler to route `uc://thumbnail/{representation_id}`
  to the thumbnail resolver use case.
- Emit `thumbnail_url` as `uc://thumbnail/{preview_rep_id}` in list projections,
  only when thumbnail metadata exists.
- Keep expand flows unchanged: `get_clipboard_entry_resource` continues to return
  `uc://blob/{preview_blob_id}` for full content.

Errors in thumbnail resolution are logged and surfaced to upper layers, with no silent
failures.

## Consequences

- Thumbnail URLs no longer depend on representation lookups by thumbnail blob id,
  avoiding `Representation not found` errors for thumbnails.
- Thumbnail reads go through ports (`ThumbnailRepositoryPort`, `BlobStorePort`),
  preserving hexagonal boundaries and encryption handling.
- List views remain lightweight: thumbnails are fetched only when needed via the
  dedicated protocol.
- `uc://blob` semantics remain unchanged for full content, reducing regression risk.
