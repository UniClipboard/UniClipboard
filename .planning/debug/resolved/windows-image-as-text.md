---
status: resolved
trigger: 'windows-image-as-text: Image copied on macOS syncs to Windows as text content'
created: 2026-03-08T00:00:00Z
updated: 2026-03-08T00:02:00Z
---

## Current Focus

hypothesis: CONFIRMED AND FIXED
test: cargo check passes, awaiting human verification on real devices
expecting: Image copied on macOS should display as image on Windows after sync
next_action: User verifies fix on actual macOS -> Windows sync

## Symptoms

expected: Image copied on macOS should display as image on Windows after sync
actual: Windows shows "Text Content (xxx)" in preview list, garbled text content when clicked
errors: No explicit errors - data syncs successfully but is misinterpreted as text
reproduction: Copy an image on macOS, sync to Windows device
started: Consistent. macOS-macOS works. Windows-macOS works. Only macOS-Windows fails for images.

## Eliminated

- hypothesis: Transport/sync protocol issue
  evidence: V3 binary protocol preserves both format_id and mime correctly (clipboard_payload_v3.rs)
  timestamp: 2026-03-08T00:00:30Z

- hypothesis: Frontend content type detection issue
  evidence: Frontend isImageType() correctly checks for "image/" prefix - would work if backend sent correct content_type
  timestamp: 2026-03-08T00:00:40Z

## Evidence

- timestamp: 2026-03-08T00:00:10Z
  checked: macOS clipboard capture in common.rs
  found: macOS fast path captures images as format_id="image", mime="image/tiff" (TIFF raw buffer)
  implication: TIFF data with image/tiff MIME is sent over the wire

- timestamp: 2026-03-08T00:00:20Z
  checked: write*snapshot in common.rs (line 409)
  found: Only "image/png" was explicitly matched. "image/tiff" fell to catch-all * branch which calls ctx.set_buffer("image", raw_bytes)
  implication: On Windows, TIFF data was written as custom format "image" - Windows doesn't recognize it as image data

- timestamp: 2026-03-08T00:00:30Z
  checked: Windows read_snapshot after write
  found: After writing TIFF as custom buffer, Windows clipboard watcher re-reads clipboard. ContentFormat::Image is false, CF_DIB fallback also fails. Raw buffer read picks it up as format_id="image" with mime=None
  implication: Re-captured entry has no MIME type -> content_type="unknown" -> frontend shows as text

- timestamp: 2026-03-08T00:00:40Z
  checked: macOS-macOS path
  found: macOS write_snapshot also only handles image/png explicitly. However macOS read_snapshot has TIFF-aware fast path that can re-read TIFF data, so it works on macOS
  implication: Bug is platform-specific to Windows receiving non-PNG image data

- timestamp: 2026-03-08T00:01:00Z
  checked: cargo check after fix
  found: Full cargo check passes with 0 errors (only pre-existing warnings)
  implication: Fix compiles correctly

## Resolution

root_cause: write_snapshot in common.rs only handled image/png in its match statement. macOS captures images as image/tiff via its fast path. When this TIFF data is synced to Windows, write_snapshot fell through to set_buffer("image", ...) which Windows doesn't recognize as image clipboard data. The clipboard watcher then re-reads it without MIME info, causing it to be displayed as text.
fix: (1) Broadened the image/png match arm to handle ALL image/\* MIME types using a match guard `Some(mime) if mime.starts_with("image/")`. RustImageData::from_bytes uses the image crate internally which supports TIFF, JPEG, BMP, etc. (2) Added format_id fallback mappings for "image"->"image/png", "public.tiff"->"image/tiff", "public.jpeg"->"image/jpeg" for robustness when MIME is not explicitly set.
verification: cargo check passes; human verified on real macOS -> Windows sync (2026-03-09)
files_changed:

- src-tauri/crates/uc-platform/src/clipboard/common.rs
