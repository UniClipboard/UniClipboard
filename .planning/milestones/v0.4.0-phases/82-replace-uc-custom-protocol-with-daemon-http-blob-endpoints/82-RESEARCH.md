# Phase 82 Research: Replace uc:// Custom Protocol with Daemon HTTP Blob Endpoints

**Date:** 2026-04-01
**Status:** Complete (Updated with full analysis)

---

## Research Summary

The `uc://` custom protocol serves binary blob and thumbnail data through the Tauri GUI process's `AppRuntime`, which owns an `InMemoryEncryptionSessionPort`. However, auto-unlock only targets the daemon's encryption session. This creates a session split: the daemon's encryption session is unlocked, but the Tauri GUI process's encryption session is not. The result is "encryption session not ready - cannot decrypt blob" errors when the frontend tries to display images via `uc://` URLs.

The fix is straightforward: add binary-serving HTTP endpoints to the daemon (`GET /clipboard/blobs/:blob_id` and `GET /clipboard/thumbnails/:rep_id`), update the URL generation in use cases to produce daemon HTTP URLs instead of `uc://` URLs, and update the frontend to use `daemonClient.request()` with `?auth=Session <token>` for `<img src>` tags.

**Critical Finding:** The daemon's auth middleware at Phase 75 already supports `?auth=Session <token>` as a query parameter fallback (for browser fetches that cannot set headers). This eliminates the authentication complexity for `<img src>` tags.

---

## Current Architecture

### uc:// Protocol Registration

**File:** `/Volumes/ExternalSSD/myprojects/uniclipboard-desktop/src-tauri/src/main.rs`

Registration at line 347:

```rust
.register_asynchronous_uri_scheme_protocol("uc", move |ctx, request, responder| {
    let app_handle = ctx.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let response = resolve_uc_request(app_handle, request).await;
        responder.respond(response);
    });
})
```

Routing at `resolve_uc_request()` → `resolve_uc_blob_request()` or `resolve_uc_thumbnail_request()`.

Both handlers access `AppRuntime` via `app_handle.try_state::<Arc<AppRuntime>>()` and call `runtime.usecases().resolve_blob_resource()` or `runtime.usecases().resolve_thumbnail_resource()`. These use cases use `AppRuntime`'s own `BlobStorePort` which wraps an `EncryptedBlobStore` backed by `AppRuntime`'s `InMemoryEncryptionSessionPort` — NOT the daemon's.

**File:** `/Volumes/ExternalSSD/myprojects/uniclipboard-desktop/src-tauri/crates/uc-tauri/src/protocol.rs`

`UcRoute` enum with `Blob { blob_id }` and `Thumbnail { representation_id }`.

URL format handling:

- macOS/Linux: `uc://blob/<id>` — host = resource type
- Windows: `http://uc.localhost/blob/<id>` — path segment = resource type
- Frontend convertFileSrc: `uc://localhost/blob/<id>` — path segment = resource type

### URL Generation in Use Cases

**File:** `/Volumes/ExternalSSD/myprojects/uniclipboard-desktop/src-tauri/crates/uc-app/src/usecases/clipboard/get_entry_resource.rs`

Line 104: `url: Some(format!("uc://blob/{}", blob_id_clone))`

**File:** `/Volumes/ExternalSSD/myprojects/uniclipboard-desktop/src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs`

Line 267: `Some(format!("uc://thumbnail/{}", preview_rep_id))`
Line 466: `Some(format!("uc://thumbnail/{}", preview_rep_id))` (duplicate in `execute_single`)

### Frontend URL Resolution

**File:** `/Volumes/ExternalSSD/myprojects/uniclipboard-desktop/src/lib/protocol.ts`

`resolveUcUrl(ucUrl)` converts `uc://blob/<id>` → `uc://localhost/blob/<id>` (macOS/Linux) or `http://uc.localhost/blob/<id>` (Windows).

**Called in:**

- `src/api/clipboardItems.ts` line 357 — `fetchClipboardResourceText()` uses `resolveUcUrl(resource.url)` for blob fetch
- `src/components/clipboard/ClipboardItem.tsx` line 202 — `resolveUcUrl(originalImageUrl)` for `<img src>`
- `src/components/clipboard/ClipboardPreview.tsx` line 142 — `resolveUcUrl(imageUrl)` for `<img src>`
- `src/preview-panel/PreviewPanel.tsx` line 60 — `resolveUcUrl(rawUrl)` for image display

### Blob Resolution Use Cases

`ResolveBlobResourceUseCase` (`resolve_blob_resource.rs`): lookups `blob_id` → representation → `BlobStorePort.get(blob_id)` → decrypted bytes.

`ResolveThumbnailResourceUseCase` (`resolve_thumbnail_resource.rs`): lookups `rep_id` → `ThumbnailMetadata` → `thumbnail_blob_id` → `BlobStorePort.get(thumbnail_blob_id)` → decrypted bytes.

Both are accessible via `CoreUseCases::resolve_blob_resource()` and `CoreUseCases::resolve_thumbnail_resource()` in `uc-app/src/usecases/mod.rs`.

---

## Daemon HTTP Capabilities

### Existing Architecture

**File:** `/Volumes/ExternalSSD/myprojects/uniclipboard-desktop/src-tauri/crates/uc-daemon/src/api/routes.rs`

Router tiers:

- `router_l1`: public (health check only)
- `router_l2_plus`: authenticated (all domain routes)

All L2+ routes protected by `auth_extractor_middleware` + `rate_limit_middleware` + CORS.

### CRITICAL: Auth Middleware Already Supports `?auth=` Query Parameter

**File:** `/Volumes/ExternalSSD/myprojects/uniclipboard-desktop/src-tauri/crates/uc-daemon/src/security/middleware.rs`

Lines 85-91 show the middleware already accepts the session JWT from either:

1. `Authorization: Session <token>` header, OR
2. `?auth=Session%20<token>` query parameter

**File:** `/Volumes/ExternalSSD/myprojects/uniclipboard-desktop/src/api/daemon/client.ts`

`DaemonClient.sendRequest()` at line 209-211 **already** sets the `?auth=Session ${token}` query param on every request URL:

```typescript
if (this.session?.token) {
  url.searchParams.set('auth', `Session ${this.session.token}`)
}
```

This means `daemonClient.request('/clipboard/blobs/{id}')` already works with the daemon's auth middleware with zero changes. For `<img src>` tags specifically, we need a method to get the pre-built URL (with auth) as a string. This is a `blobUrl(path)` helper on `DaemonClient`.

### CORS Support

**File:** `/Volumes/ExternalSSD/myprojects/uniclipboard-desktop/src-tauri/crates/uc-daemon/src/api/server.rs`

`cors_middleware` allows: `tauri://localhost`, `http://tauri.localhost`, `http://localhost:*`, `http://127.0.0.1:*`, `http://[::1]:*`.

The Tauri webview origin is `tauri://localhost` (production) or `http://localhost:1420` (dev), both are allowed. Binary responses from new blob endpoints will include `Access-Control-Allow-Origin` correctly.

### What Needs Adding

Two new daemon endpoints for binary blob serving:

1. `GET /clipboard/blobs/:blob_id` — calls `ResolveBlobResourceUseCase` → returns raw binary with `Content-Type` header
2. `GET /clipboard/thumbnails/:rep_id` — calls `ResolveThumbnailResourceUseCase` → returns raw binary with `Content-Type` header

These return `axum::response::Response` with binary body + `Content-Type` header, not JSON envelopes.

---

## Frontend Consumption Patterns

### URL Flow

1. `GET /clipboard/entries` or Tauri `get_clipboard_entries` returns `EntryProjectionDto` containing `thumbnailUrl: "uc://thumbnail/{rep_id}"`.
2. Frontend `ClipboardItem.tsx` and `ClipboardPreview.tsx` call `getClipboardEntryResource(entryId)`.
3. The resource endpoint (`GET /clipboard/entries/:id/resource`) returns `EntryResourceDto` with `url: "uc://blob/{blob_id}"`.
4. `getResourceImageUrl(resource)` returns `resource.url` (the `uc://` string).
5. Frontend calls `resolveUcUrl(url)` to get platform-specific URL for `<img src>`.

### What Changes

After migration, the URL generation in use cases produces daemon HTTP URLs. The `thumbnail_url` in `EntryProjectionDto` becomes `http://127.0.0.1:{port}/clipboard/thumbnails/{rep_id}?auth=Session+{token}` and `url` in `EntryResourceDto` becomes `http://127.0.0.1:{port}/clipboard/blobs/{blob_id}?auth=Session+{token}`.

However, the session token is not available at URL-generation time in the Rust backend. The token must be appended on the frontend side. The cleanest approach:

**Option A (Recommended):** Keep the URL as a template without token in Rust (e.g., `http://127.0.0.1:{port}/clipboard/blobs/{blob_id}`), and have the frontend `daemonClient` append `?auth=Session+{token}` when constructing `<img src>` URLs. A new helper function `daemonClient.blobUrl(path)` returns the full URL with auth appended.

**Option B:** Pass the daemon base URL to the URL-generation layer in use cases and return full URLs. This requires threading daemon port/URL through the use case layer — architecturally complex, not recommended.

### Files That Need Frontend Changes

| File                                            | What Changes                                                                                       |
| ----------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `src/lib/protocol.ts`                           | `resolveUcUrl` updated to accept daemon HTTP URLs OR deleted                                       |
| `src/api/daemon/client.ts`                      | Add `blobUrl(path): string` helper that returns URL with `?auth=Session+{token}`                   |
| `src/api/daemon/clipboard.ts`                   | `ClipboardEntryResource.url` receives daemon HTTP URL (no auth token)                              |
| `src/api/clipboardItems.ts`                     | `fetchClipboardResourceText()` uses `daemonClient.blobUrl(resource.url)` instead of `resolveUcUrl` |
| `src/components/clipboard/ClipboardItem.tsx`    | Replace `resolveUcUrl(originalImageUrl)` with `daemonClient.blobUrl(originalImageUrl)`             |
| `src/components/clipboard/ClipboardPreview.tsx` | Replace `resolveUcUrl(imageUrl)` with `daemonClient.blobUrl(imageUrl)`                             |
| `src/preview-panel/PreviewPanel.tsx`            | Replace `resolveUcUrl(rawUrl)` with `daemonClient.blobUrl(rawUrl)`                                 |

---

## Auth Strategy Analysis

### The Problem

`<img src="...">` cannot set `Authorization` headers — only JavaScript fetch can. The daemon requires a valid JWT session token for all L2+ routes.

### Options Analyzed

#### Option 1: Query Parameter Token (Recommended — Already Implemented)

Use `?auth=Session+{token}` in the URL.

**Pros:**

- Already supported by `auth_extractor_middleware` (Phase 75, lines 85-91 in `middleware.rs`)
- No changes to daemon security middleware needed
- Works natively with `<img src="...">` without JavaScript fetch
- Consistent with how `daemonClient.sendRequest()` already works (line 210 in `client.ts`)

**Cons:**

- Token visible in browser Network tab and server logs
- Token could leak via `Referer` header on cross-origin sub-resources (mitigated: daemon is localhost-only)

**Risk Level:** Low. Token only works from processes in the PID whitelist on localhost. Even if intercepted, attacker must be on the same machine with the right PID.

#### Option 2: Blob URL Approach (fetch + createObjectURL)

Fetch binary via `daemonClient.request()` with proper headers, then `URL.createObjectURL(blob)`.

**Pros:** No token in URL, cleaner from a security perspective.

**Cons:**

- Requires memory management (`URL.revokeObjectURL()` on component unmount)
- Cannot be used in `<img src>` directly without a two-step process
- Slower (extra JS step) and more complex
- Already partially done in `ClipboardItem.tsx` via `getClipboardEntryResource` + `getResourceImageUrl` pattern — but not consistently applied

#### Option 3: Cookie-Based Auth

Store session JWT in a `HttpOnly` cookie.

**Cons:** Requires cookie management, CSRF protection, and changes to daemon server setup. Significantly more complex. Not recommended.

#### Option 4: L1 (Unauthenticated) Blob Endpoints

Put blob endpoints in `router_l1` without auth.

**Cons:** Any process on the machine could fetch blob data without authentication. Violates security model. Not recommended.

### Recommended: Option 1 with `daemonClient.blobUrl()` Helper

Add a `blobUrl(path: string): string` method to `DaemonClient` that returns the full daemon URL with `?auth=Session+{token}` appended. Frontend image rendering uses this for `<img src>`. The token is the session JWT (short-lived, 300s TTL), not the long-lived bearer token.

---

## Migration Impact

### Rust Backend Changes

#### New Daemon Endpoints (`src-tauri/crates/uc-daemon/src/api/`)

New file: `src-tauri/crates/uc-daemon/src/api/blob.rs`

- `GET /clipboard/blobs/:blob_id` — calls `CoreUseCases::resolve_blob_resource()`
- `GET /clipboard/thumbnails/:rep_id` — calls `CoreUseCases::resolve_thumbnail_resource()`
- Returns `axum::response::Response` with `Bytes` body and `Content-Type`

Update `routes.rs`: merge `crate::api::blob::router()` in `router_l2_plus`.

Update `daemon_api_strings.rs` in `uc-core`: add `http_route::CLIPBOARD_BLOBS` and `http_route::CLIPBOARD_THUMBNAILS` constants.

#### Use Case URL Generation

**`get_entry_resource.rs`:** Change `url: Some(format!("uc://blob/{}", blob_id_clone))` to `url: Some(format!("/clipboard/blobs/{}", blob_id_clone))` (relative path, frontend prepends base URL).

**`list_entry_projections.rs`:** Change `Some(format!("uc://thumbnail/{}", preview_rep_id))` to `Some(format!("/clipboard/thumbnails/{}", preview_rep_id))` (both occurrences in `execute()` and `execute_single()`).

Note: Use relative paths (no scheme or host). Frontend `daemonClient.blobUrl(path)` prepends `config.baseUrl` and appends `?auth=Session+{token}`.

#### Test Updates

`get_entry_resource.rs` test at line 302: assert changes from `"uc://blob/blob-1"` to `"/clipboard/blobs/blob-1"`.

`list_entry_projections.rs` test at line 908-912: assert changes from `"uc://thumbnail/rep-1"` to `"/clipboard/thumbnails/rep-1"`.

### Frontend Changes

#### New helper in `src/api/daemon/client.ts`

Add a `blobUrl(path)` public method to the `DaemonClient` class. It reuses the same URL construction as `sendRequest()` but returns a string instead of making a fetch:

```ts
/**
 * Build a full daemon blob URL with session auth query param.
 * Suitable for use in <img src> without JavaScript fetch.
 * Returns null if client is not initialized or session is unavailable.
 */
blobUrl(path: string): string | null {
  if (!this.config || !this.session?.token) return null
  const url = new URL(`${this.config.baseUrl}${path}`)
  url.searchParams.set('auth', `Session ${this.session.token}`)
  return url.toString()
}
```

This method mirrors `sendRequest()` (lines 203-222 of `client.ts`) but is synchronous and returns the URL string instead of doing a fetch. It returns `null` when the client is not initialized or the session is unavailable, allowing callers to handle the "daemon not ready" state with a loading/placeholder image.

#### `src/lib/protocol.ts`

`resolveUcUrl` becomes unused. Can be deleted entirely or kept as a no-op stub during transition. All callers switch to `daemonClient.blobUrl(url)`.

#### `src/api/clipboardItems.ts`

`fetchClipboardResourceText()` — replace `resolveUcUrl(resource.url)` with `daemonClient.blobUrl(resource.url!)` (with null guard).

`getResourceImageUrl()` — remains unchanged structurally, but `resource.url` now contains a daemon relative path. The function returns `resource.url` unchanged. Callers that use the result for `<img src>` must call `daemonClient.blobUrl(url)`.

#### Component Changes

| Component              | Line | Before                           | After                                    |
| ---------------------- | ---- | -------------------------------- | ---------------------------------------- |
| `ClipboardItem.tsx`    | ~202 | `resolveUcUrl(originalImageUrl)` | `daemonClient.blobUrl(originalImageUrl)` |
| `ClipboardPreview.tsx` | ~142 | `resolveUcUrl(imageUrl)`         | `daemonClient.blobUrl(imageUrl)`         |
| `PreviewPanel.tsx`     | ~60  | `resolveUcUrl(rawUrl)`           | `daemonClient.blobUrl(rawUrl)`           |

Note: `blobUrl()` returns `null` if client not initialized or no session. These components already handle null image URLs with loading states.

### What Can Be Removed

After migration:

1. **`src/lib/protocol.ts`** — `resolveUcUrl` function and file (if no other consumers)
2. **`src/lib/__tests__/protocol.test.ts`** — tests for deleted function
3. **`src-tauri/crates/uc-tauri/src/protocol.rs`** — `UcRoute`, `UcRequestError`, `parse_uc_request` (only used by `main.rs` uc:// handler)
4. **`src-tauri/src/main.rs`** — `register_asynchronous_uri_scheme_protocol("uc", ...)` block, `resolve_uc_request`, `resolve_uc_blob_request`, `resolve_uc_thumbnail_request`, `set_cors_headers`, `build_response`, `text_response`, `is_allowed_cors_origin` (the main.rs CORS helpers are separate from daemon CORS)
5. **`src-tauri/crates/uc-tauri/Cargo.toml`** — verify no other dependency on `protocol.rs`

The `AppRuntime` use case accessors `usecases().resolve_blob_resource()` and `usecases().resolve_thumbnail_resource()` also become unused from Tauri commands. They remain available in `CoreUseCases` for the daemon — no change needed there.

---

## CORS Implications

The daemon listens on `127.0.0.1:{port}` (loopback only). The Tauri webview origin is:

- Production: `tauri://localhost`
- Dev: `http://localhost:1420`

Both are in `is_allowed_cors_origin()` in `server.rs`. Binary blob responses will include `Access-Control-Allow-Origin: tauri://localhost` (or the dev origin), which allows the webview to display images loaded from `http://127.0.0.1:{port}/clipboard/blobs/...`.

No CORS changes needed.

---

## Risks and Considerations

### 1. Session Token Expiry in `<img src>` URLs

Session tokens have 300s TTL. If a `<img src>` URL is built with `daemonClient.blobUrl()` and the session expires before the browser fetches the resource, the request returns 401 and the image fails to load.

**Mitigation:** `daemonClient` has a 240s keep-alive refresh timer. If the image is loaded within 240s of the session being obtained (typical), it will succeed. For edge cases, the `blobUrl()` could check if the token is near-expiry and trigger a refresh, but this adds complexity. For Phase 82, the 60s grace window (300s TTL - 240s refresh) is acceptable.

### 2. Token Visibility in Browser Network Tab

The session JWT appears in the URL's query string in browser DevTools Network tab. This is a developer UX concern, not a production security risk (all communication is localhost). Acceptable for Phase 82.

### 3. Backend URL Format Change — Wire Contract Break

Changing `thumbnail_url` from `uc://thumbnail/{id}` to `/clipboard/thumbnails/{id}` and `url` from `uc://blob/{id}` to `/clipboard/blobs/{id}` is a breaking change in the wire format. This affects:

- Any Tauri commands that return `EntryProjectionDto` (thumbnail_url field)
- `GET /clipboard/entries` daemon response
- `GET /clipboard/entries/:id/resource` daemon response

All frontend consumers must be updated atomically. Since this is a fully owned codebase (no external consumers), this is safe.

### 4. `getResourceImageUrl` Needs Review

`getResourceImageUrl()` in `clipboardItems.ts` returns `resource.url` directly for `<img src>`. After migration, callers must not pass this to `resolveUcUrl()` — they should call `daemonClient.blobUrl(resource.url)`. The function may need documentation update or be replaced by a daemon-aware version.

### 5. Test Updates Required

Unit tests in `get_entry_resource.rs` and `list_entry_projections.rs` assert specific URL formats. They must be updated to expect the new relative path format.

Frontend tests in `src/api/__tests__/clipboardItems.test.ts` and `src/lib/__tests__/protocol.test.ts` need updates.

### 6. Daemon Must Be Running

After this migration, blob/thumbnail serving requires the daemon to be running. If the daemon is down, images will fail to load (401 or connection refused). Currently the `uc://` handler works even without a daemon. This tightens the daemon-dependency coupling.

**Mitigation:** This is acceptable since Phase 57+ established the daemon as the primary clipboard watcher. The GUI already degrades gracefully when the daemon is unreachable (clipboard list shows empty state). Image display failing when daemon is down is consistent with this degraded state.

---

## Recommended Approach

### Summary

1. **Add two daemon endpoints** in new `src-tauri/crates/uc-daemon/src/api/blob.rs`:
   - `GET /clipboard/blobs/:blob_id` (L2, authenticated)
   - `GET /clipboard/thumbnails/:rep_id` (L2, authenticated)
     Both return raw binary `Bytes` with `Content-Type` header.

2. **Change URL generation** in use cases to emit relative daemon paths:
   - `get_entry_resource.rs`: `"/clipboard/blobs/{blob_id}"` instead of `"uc://blob/{blob_id}"`
   - `list_entry_projections.rs` (both occurrences): `"/clipboard/thumbnails/{rep_id}"` instead of `"uc://thumbnail/{rep_id}"`

3. **Add `blobUrl(path)` helper to `DaemonClient`** that returns `{baseUrl}{path}?auth=Session+{token}` — suitable for `<img src>` tags.

4. **Update frontend image rendering** in `ClipboardItem.tsx`, `ClipboardPreview.tsx`, `PreviewPanel.tsx` to call `daemonClient.blobUrl(url)` instead of `resolveUcUrl(url)`.

5. **Update `fetchClipboardResourceText()`** to use `daemonClient.blobUrl()` instead of `resolveUcUrl()`.

6. **Remove dead code**: `uc://` protocol handler in `main.rs`, `protocol.ts`, `protocol.rs` (uc-tauri), CORS helpers in `main.rs`.

7. **Update tests** for the URL format change.

### Implementation Order

Plan 1 (Backend): New blob.rs endpoints + URL format change in use cases + test updates.
Plan 2 (Frontend): `daemonClient.blobUrl()` helper + update all image/text resource consumers + remove protocol.ts.
Plan 3 (Cleanup): Remove `uc://` protocol handler from main.rs, remove protocol.rs from uc-tauri.

---

## RESEARCH COMPLETE
