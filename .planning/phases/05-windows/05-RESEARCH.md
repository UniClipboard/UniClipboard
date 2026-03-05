# Phase 5: Windows Clipboard Image Capture - Research

**Researched:** 2026-03-05
**Domain:** Windows clipboard image reading, clipboard-rs/clipboard-win interop, BMP/DIB/PNG format conversion
**Confidence:** HIGH

## Summary

The Windows clipboard image capture failure is a well-understood problem. The current code delegates all image reading to `clipboard-rs` via `CommonClipboardImpl::read_snapshot()`, which calls `ctx.has(ContentFormat::Image)` and `ctx.get_image()`. While `clipboard-rs` **does** support Windows image reading (checking both PNG and CF_DIB formats), the project is pinned to **v0.3.1** (November 2025), which has known issues with Windows image handling that have been fixed in later releases (v0.3.2, v0.3.3). Additionally, the project already contains a native Windows fallback function `read_image_windows()` in `windows.rs` that uses `clipboard-win` to read CF_BITMAP data, but this function is **never called** -- it is dead code.

The fix strategy is two-pronged: (1) upgrade `clipboard-rs` to v0.3.3 to get all Windows image fixes, and (2) if `clipboard-rs` still fails for certain clipboard sources, add a Windows-specific fallback in `WindowsClipboard::read_snapshot()` that uses `clipboard-win` directly to read CF_DIB/CF_BITMAP data. The existing `read_image_windows()` function has a **critical bug** -- it returns raw RGBA pixel bytes via `to_rgba8().to_vec()` instead of PNG-encoded bytes, which would break the downstream pipeline that expects `image/png` data.

**Primary recommendation:** Upgrade `clipboard-rs` to 0.3.3, then add a Windows-specific fallback in `WindowsClipboard::read_snapshot()` that catches `clipboard-rs` image read failures and falls back to reading CF_DIB via `clipboard-win` with proper BMP-to-PNG encoding using the `image` crate.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Prioritize fixing `clipboard-rs` library integration on Windows first
- Investigate why `clipboard-rs` image reading fails on Windows (works on macOS)
- If `clipboard-rs` cannot be fixed: fall back to integrating the existing native Windows API functions (`read_image_windows()` in `windows.rs`) which are already written but not called
- Goal is to keep cross-platform consistency through `clipboard-rs` if possible
- Only image capture (reading from Windows clipboard) -- not write-back
- Must work for the most common scenarios: screenshots (Win+Shift+S, Print Screen), copying images from browsers, copying from image editors
- Text capture must remain unaffected

### Claude's Discretion

- Image format support scope (CF_DIB only vs also alpha channel/transparency)
- Whether to skip `clipboard-rs` investigation and go straight to native API if evidence points to a known upstream limitation
- Error handling strategy for clipboard lock contention on Windows
- Diagnostic logging level for image capture failures
- Testing approach given primary development on macOS

### Deferred Ideas (OUT OF SCOPE)

- Writing images back to Windows clipboard (inbound sync) -- future phase
- EMF/WMF vector format support -- not needed for MVP
- Windows clipboard history integration (Win+V) -- out of scope
  </user_constraints>

## Standard Stack

### Core

| Library       | Version                    | Purpose                             | Why Standard                                                               |
| ------------- | -------------------------- | ----------------------------------- | -------------------------------------------------------------------------- |
| clipboard-rs  | 0.3.3 (upgrade from 0.3.1) | Cross-platform clipboard read/write | Already used by project, has Windows PNG+DIB+DIBV5 image support           |
| clipboard-win | 5.4                        | Windows-native clipboard fallback   | Already a dependency, provides direct CF_DIB/CF_BITMAP access              |
| image         | 0.25                       | BMP/PNG image format conversion     | Already a dependency, has `BmpDecoder::new_without_file_header` for CF_DIB |

### Supporting

| Library | Version | Purpose                                    | When to Use                                                 |
| ------- | ------- | ------------------------------------------ | ----------------------------------------------------------- |
| winapi  | 0.3     | Windows API bindings (OpenClipboard, etc.) | Only if retry logic for clipboard lock contention is needed |

### Alternatives Considered

| Instead of                    | Could Use                                 | Tradeoff                                                                                                                                                                                   |
| ----------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| clipboard-rs                  | arboard (3.4, already in deps but unused) | arboard is by 1Password and well-maintained, but would require rewriting CommonClipboardImpl; not worth the migration cost                                                                 |
| clipboard-win formats::Bitmap | clipboard-win RawData(CF_DIB)             | RawData gives DIB without BMP file header; formats::Bitmap adds BMP file header. Either works with `image` crate -- RawData(CF_DIB) + `BmpDecoder::new_without_file_header` is more direct |

**Installation:**

```bash
# In src-tauri/crates/uc-platform/Cargo.toml, update:
# clipboard-rs = { version = "0.3.3", features = ["default"] }
# clipboard-win and image are already at correct versions
```

## Architecture Patterns

### Recommended Project Structure

No structural changes needed. All modifications stay within existing files:

```
src-tauri/crates/uc-platform/src/clipboard/
  common.rs         # No changes (shared cross-platform logic)
  platform/
    windows.rs      # PRIMARY CHANGE: Override read_snapshot with fallback
    macos.rs        # No changes
    linux.rs        # No changes
  watcher.rs        # No changes
```

### Pattern 1: Windows-Specific Fallback in read_snapshot

**What:** Override the image reading path only on Windows, falling back from clipboard-rs to clipboard-win when clipboard-rs fails to read an image.
**When to use:** When `clipboard-rs` `get_image()` returns an error or `has(ContentFormat::Image)` returns false despite an image being on the clipboard.
**Example:**

```rust
// In WindowsClipboard::read_snapshot()
// Source: Existing project pattern from write_snapshot fallback in windows.rs

fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
    let span = debug_span!("platform.windows.read_clipboard");
    span.in_scope(|| {
        let mut ctx = self.inner.lock().map_err(|poison| {
            error!("Failed to lock clipboard context (poisoned mutex)");
            anyhow::anyhow!("mutex poisoned: {}", poison)
        })?;

        // Try clipboard-rs first (cross-platform path)
        let mut snapshot = CommonClipboardImpl::read_snapshot(&mut ctx)?;

        // If no image representation found, try native Windows fallback
        let has_image = snapshot.representations.iter().any(|rep| {
            rep.mime.as_ref().is_some_and(|m| m.as_str().starts_with("image/"))
        });

        if !has_image {
            // Drop the mutex before calling clipboard-win (avoids double clipboard open)
            drop(ctx);

            match read_image_windows_as_png() {
                Ok(png_bytes) => {
                    debug!(size_bytes = png_bytes.len(), "Read image via Windows native fallback");
                    snapshot.representations.push(ObservedClipboardRepresentation {
                        id: RepresentationId::new(),
                        format_id: "image".into(),
                        mime: Some(MimeType("image/png".to_string())),
                        bytes: png_bytes,
                    });
                }
                Err(err) => {
                    // Not an error -- clipboard may genuinely have no image
                    debug!(error = %err, "Windows native image fallback unavailable");
                }
            }
        }

        Ok(snapshot)
    })
}
```

### Pattern 2: BMP-to-PNG Conversion Using image Crate

**What:** Read CF_DIB from clipboard and encode as PNG for downstream consistency.
**When to use:** When the native fallback path is triggered.
**Example:**

```rust
// Source: image crate docs (BmpDecoder::new_without_file_header)
// and clipboard-win docs (RawData for CF_DIB)

fn read_image_windows_as_png() -> Result<Vec<u8>> {
    use clipboard_win::{formats, get_clipboard};
    use image::codecs::bmp::BmpDecoder;
    use image::DynamicImage;
    use std::io::Cursor;

    // Try CF_DIB first (most common from screenshots)
    let dib_data: Vec<u8> = get_clipboard(formats::RawData(formats::CF_DIB))
        .map_err(|e| anyhow::anyhow!("No image on clipboard: {}", e))?;

    // CF_DIB has no BMP file header, use new_without_file_header
    let cursor = Cursor::new(&dib_data);
    let decoder = BmpDecoder::new_without_file_header(cursor)
        .map_err(|e| anyhow::anyhow!("Failed to decode DIB: {}", e))?;
    let image = DynamicImage::from_decoder(decoder)
        .map_err(|e| anyhow::anyhow!("Failed to load DIB image: {}", e))?;

    // Encode as PNG
    let mut png_bytes = Vec::new();
    image.write_to(
        &mut Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    ).map_err(|e| anyhow::anyhow!("Failed to encode PNG: {}", e))?;

    Ok(png_bytes)
}
```

### Pattern 3: Clipboard Lock Retry (from existing write_image_windows)

**What:** Retry clipboard operations when another process holds the lock.
**When to use:** For clipboard lock contention scenarios.
**Example:**

```rust
// Source: Existing code in windows.rs write_image_windows (lines 186-207)
// The write path already has retry logic (max 5 attempts, 10ms delay).
// For read operations via clipboard-rs, the library handles opening/closing
// internally, so explicit retry is NOT needed for the primary path.
// Only needed if using raw winapi clipboard access.
```

### Anti-Patterns to Avoid

- **Replacing CommonClipboardImpl entirely for Windows:** The cross-platform path works for text, RTF, HTML, files. Only image needs the fallback. Do not fork the entire read_snapshot logic.
- **Using formats::Bitmap instead of RawData(CF_DIB):** formats::Bitmap reads CF_BITMAP (device-dependent), which loses color fidelity. CF_DIB is the correct format for device-independent bitmap data.
- **Returning raw RGBA bytes instead of PNG:** The existing `read_image_windows()` function has this bug -- `to_rgba8().to_vec()` returns raw pixel data, not PNG. Downstream code expects PNG-encoded bytes.
- **Opening clipboard while clipboard-rs still holds it:** Must drop the ClipboardContext mutex guard before calling clipboard-win functions to avoid deadlock.

## Don't Hand-Roll

| Problem                      | Don't Build                     | Use Instead                                                                             | Why                                                                             |
| ---------------------------- | ------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| BMP/DIB decoding             | Manual BITMAPINFOHEADER parsing | `image::codecs::bmp::BmpDecoder::new_without_file_header`                               | Handles all DIB variants (V1, V4, V5), bit depths, compression modes            |
| PNG encoding                 | Custom PNG writer               | `image::DynamicImage::write_to(..., ImageFormat::Png)`                                  | Handles color type conversion, proper PNG chunk structure                       |
| Clipboard format detection   | Manual EnumClipboardFormats     | `clipboard-rs ctx.has(ContentFormat::Image)` + `clipboard_win::is_format_avail(CF_DIB)` | Handles format registration, synthesis detection                                |
| BMP file header construction | Manual 54-byte header           | `image` crate's built-in BMP encoder                                                    | The existing `get_bmp_header()` in windows.rs is unnecessary with `image` crate |

**Key insight:** The `image` crate (already a dependency at v0.25) handles all the complex BMP format variants. The existing hand-rolled BMP header code in `windows.rs` (`to_bitmap()`, `get_bmp_header()`, `set_bytes()`) is only needed for the write path and can be left as-is (write is out of scope).

## Common Pitfalls

### Pitfall 1: Clipboard Lock Contention

**What goes wrong:** Windows clipboard is a global resource. Only one process can open it at a time. If another process (clipboard managers like Ditto, uTools) holds the lock, reads fail.
**Why it happens:** clipboard-rs opens the clipboard internally during `get_image()`. If a clipboard manager is monitoring, it may hold the lock briefly.
**How to avoid:** clipboard-rs handles this internally. For the native fallback via clipboard-win, the `get_clipboard()` helper also handles open/close. No explicit retry needed for read path.
**Warning signs:** Intermittent "Failed to read image representation" errors in logs, especially when clipboard managers are running.

### Pitfall 2: CF_DIB Without BMP File Header

**What goes wrong:** CF_DIB clipboard data starts with BITMAPINFOHEADER (40+ bytes), NOT a BMP file header (14-byte BITMAPFILEHEADER). Passing CF_DIB data to `image::load_from_memory_with_format(&data, ImageFormat::Bmp)` fails because the BMP decoder expects the file header.
**Why it happens:** Windows clipboard stores device-independent bitmap data without the file header.
**How to avoid:** Use `BmpDecoder::new_without_file_header()` for CF_DIB data, or use `formats::Bitmap` which adds the file header automatically.
**Warning signs:** "Failed to decode BMP" errors -- this is exactly the bug in the existing `read_image_windows()` function if `formats::Bitmap` returns data that does not include a proper file header.

### Pitfall 3: Raw RGBA Bytes vs PNG Encoding

**What goes wrong:** The existing `read_image_windows()` returns `rgba_image.to_vec()` which is raw RGBA pixel data (width _ height _ 4 bytes). Downstream code expects PNG-encoded bytes (proper PNG file with headers, IDAT chunks, etc.).
**Why it happens:** `to_rgba8()` converts to an RGBA image buffer, and `.to_vec()` just dumps the raw pixel data.
**How to avoid:** Use `image.write_to(&mut cursor, ImageFormat::Png)` to get proper PNG-encoded bytes.
**Warning signs:** Thumbnail generation fails, image appears corrupt or has wrong dimensions.

### Pitfall 4: Alpha Channel / Premultiplied Alpha

**What goes wrong:** Some applications (notably Chrome) put premultiplied-alpha image data on the clipboard. Reading this with standard non-premultiplied alpha assumption produces semi-transparent pixels with wrong colors.
**Why it happens:** Windows bitmap alpha handling is historically inconsistent across applications.
**How to avoid:** For MVP, accept the alpha as-is. The `image` crate handles standard RGBA. Chrome's premultiplied alpha edge case is acceptable to ignore for now.
**Warning signs:** Semi-transparent areas appear darker than expected when pasted from Chrome.

### Pitfall 5: Mutex Guard Lifetime and clipboard-win Calls

**What goes wrong:** If the `ClipboardContext` mutex guard is still held when calling `clipboard-win` functions, both libraries try to open the clipboard simultaneously, causing a deadlock or error.
**Why it happens:** clipboard-rs opens the clipboard internally, and clipboard-win also opens it. Can't have both open.
**How to avoid:** Always `drop(ctx)` before calling any clipboard-win functions in the fallback path.
**Warning signs:** Hang in `read_snapshot()`, or "clipboard is already open" errors.

### Pitfall 6: Windows 11 CF_DIB-to-CF_DIBV5 Conversion Bug

**What goes wrong:** Windows 11 (23H2, 24H2, 25H2) has a known bug where the OS-level conversion from CF_DIB to CF_DIBV5 produces corrupted data with a 3-pixel shift in BI_BITFIELDS mode.
**Why it happens:** Microsoft bug in the synthesized format conversion.
**How to avoid:** Read CF_DIB directly (not CF_DIBV5) when possible. clipboard-rs already prioritizes PNG > DIBV5 > DIB, so the bug mainly affects direct DIBV5 reads.
**Warning signs:** Images with 3 superfluous red/green/blue pixels at the bottom-left corner.

## Code Examples

### Reading CF_DIB Without File Header

```rust
// Source: docs.rs/image/0.25.9/image/codecs/bmp/struct.BmpDecoder.html
use image::codecs::bmp::BmpDecoder;
use image::DynamicImage;
use std::io::Cursor;

let dib_data: Vec<u8> = /* from clipboard */;
let cursor = Cursor::new(&dib_data);
let decoder = BmpDecoder::new_without_file_header(cursor)?;
let image = DynamicImage::from_decoder(decoder)?;
```

### Encoding DynamicImage to PNG Bytes

```rust
// Source: image crate docs
use image::ImageFormat;
use std::io::Cursor;

let mut png_bytes = Vec::new();
image.write_to(
    &mut Cursor::new(&mut png_bytes),
    ImageFormat::Png,
)?;
// png_bytes now contains valid PNG file data
```

### Checking CF_DIB Availability via clipboard-win

```rust
// Source: docs.rs/clipboard-win/5.4.0/clipboard_win/formats
use clipboard_win::formats;

if clipboard_win::is_format_avail(formats::CF_DIB) {
    let data: Vec<u8> = clipboard_win::get_clipboard(
        formats::RawData(formats::CF_DIB)
    )?;
    // data contains BITMAPINFOHEADER + pixel data (no file header)
}
```

### clipboard-rs Windows Image Read Priority

```rust
// Source: https://github.com/ChurchTao/clipboard-rs/blob/master/src/platform/win.rs
// clipboard-rs get_image() on Windows uses this priority:
// 1. PNG registered format (if available) -- best quality, preserves alpha
// 2. CF_DIBV5 (if available) -- extended format with alpha support
// 3. CF_DIB (fallback) -- standard device-independent bitmap
```

## State of the Art

| Old Approach              | Current Approach                      | When Changed                         | Impact                               |
| ------------------------- | ------------------------------------- | ------------------------------------ | ------------------------------------ |
| CF_BITMAP only            | PNG > DIBV5 > DIB cascade             | clipboard-rs v0.3.0 (2025-07)        | Better alpha/transparency support    |
| Manual BMP header parsing | `BmpDecoder::new_without_file_header` | image crate 0.24+                    | Correct handling of all DIB variants |
| CF_DIB for everything     | Prefer registered PNG format          | Modern Windows apps (Office, Chrome) | Better cross-app compatibility       |

**Deprecated/outdated:**

- `read_image_windows()` in current codebase: Has the `to_rgba8().to_vec()` bug (returns raw pixels, not PNG). Must be fixed or replaced if used.
- `clipboard-rs v0.3.1`: Missing fixes from v0.3.2 (HTML parsing) and v0.3.3 (Windows HTML multi-set). No Windows image-specific fixes between versions, but upgrading is low-risk and ensures latest compatibility.

## Open Questions

1. **Does upgrading clipboard-rs to 0.3.3 alone fix the image capture?**
   - What we know: clipboard-rs v0.3.1 already has the PNG > DIBV5 > DIB cascade. The changelog between 0.3.1 and 0.3.3 shows no Windows image-specific fixes.
   - What's unclear: The exact error that occurs when `get_image()` fails on Windows. Without Windows test access, we cannot confirm whether the issue is in clipboard-rs or in the integration.
   - Recommendation: Upgrade and add diagnostic logging to capture the exact failure mode. Implement the native fallback regardless as defense-in-depth.

2. **What exact error does clipboard-rs produce when image reading fails on Windows?**
   - What we know: The `warn!(error = %err, "Failed to read image representation")` log in common.rs would show the error, but we need to see actual Windows logs.
   - What's unclear: Is it `has(ContentFormat::Image)` returning false, or `get_image()` failing? Is it for all image sources or only certain ones?
   - Recommendation: Add more granular logging to distinguish which step fails (has check vs get_image vs to_png conversion).

3. **Does clipboard-win require its own clipboard open/close, or does get_clipboard handle it?**
   - What we know: `clipboard_win::get_clipboard()` is a convenience function that handles open/close internally.
   - What's unclear: Whether there's a timing conflict between clipboard-rs releasing the clipboard and clipboard-win opening it.
   - Recommendation: Drop the clipboard-rs context mutex guard before calling clipboard-win fallback functions.

## Validation Architecture

### Test Framework

| Property           | Value                                                      |
| ------------------ | ---------------------------------------------------------- |
| Framework          | cargo test (Rust built-in)                                 |
| Config file        | src-tauri/Cargo.toml (workspace)                           |
| Quick run command  | `cd src-tauri && cargo test -p uc-platform -- --nocapture` |
| Full suite command | `cd src-tauri && cargo test`                               |

### Phase Requirements to Test Map

| Req ID     | Behavior                                             | Test Type   | Automated Command                                                    | File Exists?                |
| ---------- | ---------------------------------------------------- | ----------- | -------------------------------------------------------------------- | --------------------------- |
| WIN-IMG-01 | clipboard-rs upgrade does not break text capture     | unit        | `cd src-tauri && cargo test -p uc-platform -- watcher -x`            | Existing watcher tests      |
| WIN-IMG-02 | read_image_windows_as_png returns valid PNG bytes    | unit        | `cd src-tauri && cargo test -p uc-platform -- read_image_windows -x` | Wave 0                      |
| WIN-IMG-03 | PNG encoding from DynamicImage produces valid output | unit        | `cd src-tauri && cargo test -p uc-platform -- png_encode -x`         | Wave 0                      |
| WIN-IMG-04 | Fallback triggers when clipboard-rs has no image rep | unit (mock) | `cd src-tauri && cargo test -p uc-platform -- fallback -x`           | Wave 0                      |
| WIN-IMG-05 | End-to-end image capture from screenshots            | manual-only | Manual: Win+Shift+S, verify image in clipboard history               | N/A (requires Windows + UI) |
| WIN-IMG-06 | End-to-end image capture from browser copy           | manual-only | Manual: Right-click > Copy Image in browser                          | N/A (requires Windows + UI) |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-platform -- --nocapture`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-platform/src/clipboard/platform/windows.rs` -- unit tests for PNG encoding helper (can test on any platform since `image` crate is cross-platform)
- [ ] Cross-platform test for BMP-to-PNG conversion logic (extract conversion to a testable function, guard Windows-specific clipboard access behind `#[cfg(target_os = "windows")]`)

## Sources

### Primary (HIGH confidence)

- [clipboard-rs GitHub](https://github.com/ChurchTao/clipboard-rs) - Issue #8 (Windows get_image support), #18 (transparency fix), #35 (pixel shift), #55 (handle contention), #57 (RGBA conversion)
- [clipboard-rs source: win.rs](https://github.com/ChurchTao/clipboard-rs/blob/master/src/platform/win.rs) - Windows `has()` checks PNG + CF_DIB; `get_image()` cascades PNG > DIBV5 > DIB
- [clipboard-win docs](https://docs.rs/clipboard-win/latest/clipboard_win/formats/struct.Bitmap.html) - Bitmap and RawData format documentation
- [image crate BmpDecoder](https://docs.rs/image/latest/image/codecs/bmp/struct.BmpDecoder.html) - `new_without_file_header` for CF_DIB decoding
- [Microsoft: Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats) - Synthesized format conversion table (CF_BITMAP <-> CF_DIB <-> CF_DIBV5)
- [Microsoft: Standard Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/standard-clipboard-formats) - CF_DIB, CF_DIBV5 specifications
- Project source code: `src-tauri/crates/uc-platform/src/clipboard/` - All clipboard implementation files

### Secondary (MEDIUM confidence)

- [clipboard-rs DeepWiki](https://deepwiki.com/ChurchTao/clipboard-rs/1-overview) - Overview confirms Windows uses "CF_DIB / PNG" format identifiers
- [arboard GitHub](https://github.com/1Password/arboard) - Alternative crate by 1Password, already in project deps but unused
- [Windows 11 CF_DIB bug report](https://techcommunity.microsoft.com/discussions/windows11/serious-bug-in-windows-clipboard---images-get-corrupted/4466584) - Known Windows 11 DIB-to-DIBV5 conversion corruption

### Tertiary (LOW confidence)

- [Mozilla Bug 1717306](https://bugzilla.mozilla.org/show_bug.cgi?id=1717306) - Browser perspective on PNG clipboard format adoption on Windows

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - All libraries already in project dependencies, well-documented APIs
- Architecture: HIGH - Follows existing fallback pattern from `write_snapshot()` in windows.rs
- Pitfalls: HIGH - Multiple sources confirm clipboard lock contention, DIB header issues, alpha handling challenges
- Code examples: HIGH - Verified against official docs (image crate BmpDecoder, clipboard-win formats)

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable domain, clipboard APIs rarely change)
