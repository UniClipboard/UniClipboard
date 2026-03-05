# Phase 6: Fix Dashboard Image Display - Research

**Researched:** 2026-03-05
**Domain:** Tauri 2 custom protocol + frontend image rendering
**Confidence:** HIGH

## Summary

Images are captured successfully by the clipboard system (Phase 5 confirmed PNG capture working on Windows), but they are not visible in the dashboard. The problem description states this is "not a thumbnail issue" -- meaning the expanded (full-size) image also fails to display.

The data pipeline for image display has two paths: (1) thumbnail display via `uc://thumbnail/{rep_id}` and (2) full image display via `uc://blob/{blob_id}`. Both rely on Tauri 2's custom URI scheme protocol (`register_asynchronous_uri_scheme_protocol("uc", ...)`). The Rust handler correctly resolves these routes, reads from the blob store, and returns image bytes with proper MIME types and CORS headers.

**The most likely root cause is a cross-platform URL format mismatch.** Tauri 2 custom URI scheme protocols use different URL formats on different platforms: on macOS/Linux, the webview expects `uc://localhost/{path}`, while on Windows, it expects `http://uc.localhost/{path}`. The backend currently generates URLs in `uc://thumbnail/{rep_id}` format (no `localhost` host), and the frontend uses these URLs directly in `<img src>` attributes. This may work on some platforms but fail on others, or fail on all platforms depending on the exact webview behavior.

**Primary recommendation:** Use Tauri's `convertFileSrc()` API from `@tauri-apps/api/core` to generate platform-correct URLs for custom protocol resources, or build a helper that normalizes `uc://` URLs to the platform-appropriate format.

## Standard Stack

### Core

| Library           | Version   | Purpose                                              | Why Standard                                |
| ----------------- | --------- | ---------------------------------------------------- | ------------------------------------------- |
| `@tauri-apps/api` | (project) | `convertFileSrc()` for cross-platform URL generation | Official Tauri API for custom protocol URLs |

### Supporting

No additional libraries needed. This is a fix to existing code.

## Architecture Patterns

### Current Data Flow (Image Display)

```
Backend capture -> representation (image/png) -> background_blob_worker
  -> blob materialization (encrypted on disk)
  -> thumbnail generation (webp) -> thumbnail blob -> thumbnail metadata in DB

Frontend list:
  get_clipboard_entries -> list_entry_projections use case
  -> thumbnail_url: "uc://thumbnail/{rep_id}" (if thumbnail metadata exists)
  -> Frontend renders <img src="uc://thumbnail/{rep_id}" />

Frontend expand:
  get_clipboard_entry_resource -> get_entry_resource use case
  -> url: "uc://blob/{blob_id}"
  -> Frontend renders <img src="uc://blob/{blob_id}" />

URL resolution:
  Tauri custom protocol handler (main.rs)
  -> parse_uc_request() -> UcRoute::Thumbnail or UcRoute::Blob
  -> resolve_uc_thumbnail_request() or resolve_uc_blob_request()
  -> blob_store.get() -> decrypt + decompress -> raw image bytes
  -> HTTP response with Content-Type and CORS headers
```

### Recommended Fix Pattern

```
1. Backend: Generate URLs in platform-neutral format (keep uc://thumbnail/{id})
2. Frontend: Create a helper function that converts uc:// URLs to
   platform-correct format using convertFileSrc or window.__TAURI_INTERNALS__
3. Apply the helper in ClipboardItem.tsx for both thumbnail and expanded views
```

### Pattern: Platform-Aware URL Helper

```typescript
// src/lib/protocol.ts
import { convertFileSrc } from '@tauri-apps/api/core'

/**
 * Convert a uc:// protocol URL to a platform-correct URL.
 * On macOS/Linux: uc://localhost/thumbnail/{id}
 * On Windows: http://uc.localhost/thumbnail/{id}
 */
export function resolveUcUrl(ucUrl: string): string {
  // Extract the path after "uc://"
  // e.g., "uc://thumbnail/rep-1" -> "thumbnail/rep-1"
  //        "uc://blob/blob-1" -> "blob/blob-1"
  const match = ucUrl.match(/^uc:\/\/(.+)$/)
  if (!match) return ucUrl
  const path = match[1]
  // convertFileSrc(path, 'uc') generates the correct URL per platform
  return convertFileSrc(path, 'uc')
}
```

### Anti-Patterns to Avoid

- **Hardcoding `uc://` URLs directly in `<img src>`**: Fails on Windows where WebView2 uses `http://uc.localhost/` format
- **Platform detection with `navigator.platform`**: Fragile; use Tauri's built-in `convertFileSrc` instead
- **Changing backend URL generation per platform**: The backend should remain platform-agnostic; the frontend should adapt

## Don't Hand-Roll

| Problem                             | Don't Build                               | Use Instead                                    | Why                                               |
| ----------------------------------- | ----------------------------------------- | ---------------------------------------------- | ------------------------------------------------- |
| Cross-platform custom protocol URLs | Manual platform detection + URL rewriting | `convertFileSrc()` from `@tauri-apps/api/core` | Tauri handles all platform differences internally |

## Common Pitfalls

### Pitfall 1: Custom URI Scheme URL Format Differs by Platform

**What goes wrong:** `<img src="uc://thumbnail/rep-1" />` works on macOS but fails silently on Windows
**Why it happens:** Windows WebView2 doesn't support custom URI schemes natively. Tauri emulates them using `http://{scheme}.localhost/` format.
**How to avoid:** Always use `convertFileSrc()` to generate URLs for custom protocol resources
**Warning signs:** Images load in dev mode on macOS but not on Windows, or vice versa

### Pitfall 2: Missing localhost in Custom Scheme URLs

**What goes wrong:** `uc://thumbnail/rep-1` may not be properly routed even on macOS
**Why it happens:** Tauri custom protocols expect the format `scheme://localhost/path`, not `scheme://path`
**How to avoid:** Ensure URLs follow `uc://localhost/thumbnail/rep-1` format, or better yet use `convertFileSrc()`
**Warning signs:** Network tab shows 404 or the request never reaches the Rust handler

### Pitfall 3: Thumbnail Not Yet Generated When Dashboard Renders

**What goes wrong:** Dashboard fetches entries before the background blob worker has generated thumbnails
**Why it happens:** Thumbnail generation is async -- the background worker processes after the capture event
**How to avoid:** Handle `null` thumbnail gracefully with a placeholder; consider polling or event-driven refresh after blob processing completes
**Warning signs:** Images appear as loading placeholder permanently; thumbnail_url is `None` in the response

### Pitfall 4: Blob Store Returns Encrypted Data

**What goes wrong:** Image bytes returned by the protocol handler are still encrypted/compressed
**Why it happens:** `EncryptedBlobStore` wraps `FilesystemBlobStore` and the `get()` method must decrypt + decompress
**How to avoid:** Verify the blob store wiring in AppRuntime -- the protocol handler should use the same blob store that handles decryption
**Warning signs:** Response has correct Content-Type but browser can't decode the image

### Pitfall 5: CORS Blocking in Dev Mode

**What goes wrong:** Custom protocol requests blocked by CORS policy
**Why it happens:** In dev mode, frontend runs on `http://localhost:1420`, and custom protocol responses need CORS headers
**How to avoid:** The existing `set_cors_headers` function handles this, but verify the Origin header matches
**Warning signs:** Console shows CORS errors; Network tab shows blocked requests

## Code Examples

### Current URL Generation (Backend - list_entry_projections.rs)

```rust
// Source: src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs
let thumbnail_url = if is_image {
    match self.thumbnail_repo.get_by_representation_id(&selection.selection.preview_rep_id).await {
        Ok(Some(_metadata)) => Some(format!("uc://thumbnail/{}", preview_rep_id)),
        Ok(None) => None,
        Err(err) => { /* logged, returns None */ None }
    }
} else { None };
```

### Current URL Generation (Backend - get_entry_resource.rs)

```rust
// Source: src-tauri/crates/uc-app/src/usecases/clipboard/get_entry_resource.rs
Ok(EntryResourceResult {
    entry_id: entry.entry_id.to_string(),
    blob_id: blob_id.clone(),
    mime_type: mime_type_str.map(String::from),
    size_bytes: preview_rep.size_bytes,
    url: format!("uc://blob/{}", blob_id),
})
```

### Current Frontend Image Rendering (ClipboardItem.tsx)

```typescript
// Source: src/components/clipboard/ClipboardItem.tsx
case 'image': {
    const thumbnailUrl = (content as ClipboardImageItem | null)?.thumbnail ?? null
    const imageUrl = isExpanded && detailImageUrl ? detailImageUrl : thumbnailUrl
    return (
        <div className="flex justify-center bg-black/20 rounded-lg overflow-hidden py-4">
            {imageUrl ? (
                <img src={imageUrl} /* ... */ />
            ) : (
                <div>/* placeholder */</div>
            )}
        </div>
    )
}
```

### Fix: Platform-Aware URL Helper

```typescript
// Source: new file src/lib/protocol.ts
import { convertFileSrc } from '@tauri-apps/api/core'

/**
 * Convert a uc:// protocol URL to a webview-loadable URL.
 *
 * Tauri custom protocol URLs differ by platform:
 * - macOS/Linux: uc://localhost/{path}
 * - Windows: http://uc.localhost/{path}
 *
 * convertFileSrc handles the conversion automatically.
 */
export function resolveUcUrl(ucUrl: string): string {
  const match = ucUrl.match(/^uc:\/\/(.+)$/)
  if (!match) return ucUrl
  // convertFileSrc("thumbnail/rep-1", "uc") produces the correct URL
  return convertFileSrc(match[1], 'uc')
}
```

### Fix: Apply Helper in ClipboardItem.tsx

```typescript
import { resolveUcUrl } from '@/lib/protocol'

// In image rendering:
const thumbnailUrl = (content as ClipboardImageItem | null)?.thumbnail ?? null
const resolvedThumbnail = thumbnailUrl ? resolveUcUrl(thumbnailUrl) : null
const imageUrl = isExpanded && detailImageUrl ? resolveUcUrl(detailImageUrl) : resolvedThumbnail
```

## State of the Art

| Old Approach                          | Current Approach                                 | When Changed         | Impact                                 |
| ------------------------------------- | ------------------------------------------------ | -------------------- | -------------------------------------- |
| Raw `uc://` URLs in img src           | Use `convertFileSrc()` for platform-correct URLs | Tauri 2.0 (Oct 2024) | Cross-platform custom protocol support |
| `https://` for Windows custom schemes | `http://` for Windows custom schemes             | Tauri 2.0            | URL format change from v1 to v2        |

## Debugging Checklist

Before implementing the fix, the planner should verify these diagnostic steps:

1. **Check browser DevTools Network tab** -- Are requests to `uc://...` being made? What status code?
2. **Check browser DevTools Console** -- Any CORS or CSP errors?
3. **Try `fetch("uc://thumbnail/{rep_id}")` in DevTools Console** -- Does it return data?
4. **Check backend logs** -- Is `resolve_uc_thumbnail_request` being called? Any errors?
5. **Verify thumbnail exists in DB** -- Is `thumbnail_url` populated in the entries response?
6. **Test with `convertFileSrc("thumbnail/rep-1", "uc")` in console** -- What URL does it produce?

## Open Questions

1. **Is this Windows-only or cross-platform?**
   - What we know: Phase 5 verified image capture on Windows, noted "dashboard display issue as separate concern"
   - What's unclear: Whether the same issue occurs on macOS/Linux
   - Recommendation: The fix (using `convertFileSrc`) is platform-agnostic and should be applied regardless

2. **Is thumbnail generation actually completing?**
   - What we know: The background blob worker generates thumbnails after blob materialization
   - What's unclear: Whether the async thumbnail generation completes before the frontend lists entries
   - Recommendation: Add diagnostic logging/check; handle `null` thumbnail_url gracefully

3. **Does `convertFileSrc` work with custom scheme routes (not file paths)?**
   - What we know: `convertFileSrc` is designed for file paths with the `asset` protocol
   - What's unclear: Whether it works correctly with route-based URLs like `thumbnail/rep-1`
   - Recommendation: Test this first; if it doesn't work, build a platform-aware URL helper using `window.__TAURI_INTERNALS__.convertFileSrc` logic directly

## Sources

### Primary (HIGH confidence)

- Project source code: `src/components/clipboard/ClipboardItem.tsx`, `src/api/clipboardItems.ts`
- Project source code: `src-tauri/crates/uc-tauri/src/protocol.rs`, `src-tauri/src/main.rs`
- Project source code: `src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/`
- Project source code: `src-tauri/crates/uc-infra/src/clipboard/thumbnail_generator.rs`

### Secondary (MEDIUM confidence)

- [Tauri Issue #9875: Unable to load custom URI scheme](https://github.com/tauri-apps/tauri/issues/9875) - Confirmed Windows URL format issue
- [Tauri Discussion #5506: Custom protocol URL format differences](https://github.com/orgs/tauri-apps/discussions/5506) - Platform URL format table
- [Tauri Discussion #10868: register_uri_scheme_protocol platform differences](https://github.com/orgs/tauri-apps/discussions/10868)
- [Tauri 2.0 Release Notes](https://v2.tauri.app/blog/tauri-20/) - `http` scheme for Windows in v2

### Tertiary (LOW confidence)

- `convertFileSrc` behavior with non-file-path arguments (route-based paths) -- needs verification

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - Using existing Tauri API, no new dependencies
- Architecture: HIGH - Clear data flow traced through entire pipeline
- Root cause: MEDIUM - URL format is most likely cause but needs verification via debugging
- Pitfalls: HIGH - Well-documented Tauri cross-platform issue

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable domain, Tauri 2 custom protocol API is settled)
