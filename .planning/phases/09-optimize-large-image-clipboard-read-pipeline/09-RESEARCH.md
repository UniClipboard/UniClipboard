# Phase 9: Optimize Large Image Clipboard Read Pipeline - Research

**Researched:** 2026-03-06
**Domain:** macOS/Windows clipboard image capture optimization (Rust, clipboard-rs, image crate)
**Confidence:** HIGH

## Summary

The clipboard image capture pipeline has three identified bottlenecks causing ~3s latency and ~71MB peak memory for a single image copy. All three originate in `CommonClipboardImpl::read_snapshot()` in `uc-platform/src/clipboard/common.rs`.

**Bottleneck 1 (TIFF->PNG transcode):** On macOS, `clipboard-rs::get_image()` reads TIFF data from NSPasteboard, decodes it via `image::load_from_memory()` into a `DynamicImage`, then `to_png()` re-encodes it to PNG. For a 34MB TIFF (typical screenshot), this decode+encode cycle takes ~3 seconds. The transcode happens synchronously on the clipboard watcher thread, blocking all subsequent clipboard events.

**Bottleneck 2 (duplicate TIFF reads):** macOS NSPasteboard reports TIFF data under two format names: `public.tiff` and `NeXT TIFF v4.0 pasteboard type`. These are aliases for identical data. After the high-level `get_image()` call produces a PNG representation (format_id="image"), the raw fallback loop at lines 175-205 reads both TIFF aliases via `get_buffer()`, creating two additional 34MB `Vec<u8>` buffers. The `seen` set only tracks high-level format_ids ("text", "rtf", "html", "image", "files"), not the raw pasteboard type strings, so the aliases are not deduplicated.

**Bottleneck 3 (peak memory):** The combined allocations are: PNG output from transcode (~3-5MB) + raw TIFF buffer 1 (34MB) + raw TIFF buffer 2 (34MB) = ~71MB for a single image capture. All buffers are held simultaneously in the `reps` Vec until the snapshot is returned.

**Primary recommendation:** Skip the `get_image()` + `to_png()` transcode entirely on macOS. Instead, read raw TIFF once via `get_buffer("public.tiff")`, store it as `image/tiff` with format_id `public.tiff`, and deduplicate known TIFF aliases. Defer PNG conversion to the spool background worker if PNG is needed downstream. This eliminates the 3s blocking transcode and reduces peak memory from ~71MB to ~34MB.

<phase_requirements>

## Phase Requirements

| ID             | Description                                      | Research Support                                                                                                                                                                    |
| -------------- | ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TIFF-DEDUP     | Deduplicate TIFF aliases on macOS clipboard      | Known aliases `public.tiff` and `NeXT TIFF v4.0 pasteboard type` identified in clipboard-rs source and Apple docs; filter list in `should_skip_raw_format()` pattern already exists |
| SKIP-TRANSCODE | Skip/defer PNG transcode when raw TIFF available | `get_image()` path in clipboard-rs does TIFF->DynamicImage->PNG; can be replaced with direct `get_buffer("public.tiff")` read; PNG conversion deferred to spool worker              |
| REDUCE-MEMORY  | Reduce peak memory during image capture          | Eliminating duplicate TIFF reads and deferred transcode reduces peak from ~71MB to ~34MB; further reduction possible with early-drop of raw buffers                                 |

</phase_requirements>

## Standard Stack

### Core

| Library      | Version                       | Purpose                 | Why Standard                                                                             |
| ------------ | ----------------------------- | ----------------------- | ---------------------------------------------------------------------------------------- |
| clipboard-rs | 0.3.3                         | System clipboard access | Already in use; provides `get_buffer()`, `available_formats()`, `has()`                  |
| image        | (transitive via clipboard-rs) | Image decode/encode     | Used by clipboard-rs for TIFF->PNG; will be used in spool worker for deferred conversion |

### Supporting

| Library | Version    | Purpose                  | When to Use                                      |
| ------- | ---------- | ------------------------ | ------------------------------------------------ |
| tracing | (existing) | Structured logging/spans | Instrument new code paths with spans             |
| blake3  | (existing) | Content hashing          | Already used for snapshot/representation hashing |

### Alternatives Considered

| Instead of                      | Could Use              | Tradeoff                                                         |
| ------------------------------- | ---------------------- | ---------------------------------------------------------------- |
| Store raw TIFF                  | Convert to PNG eagerly | Current approach; 3s blocking, high memory                       |
| image crate PNG encode          | libpng/oxipng          | Marginal speed improvement, adds dependency                      |
| Skip all raw formats for images | Keep raw formats       | Lose fidelity for niche apps that need specific pasteboard types |

## Architecture Patterns

### Current Pipeline (Before)

```
on_clipboard_change()
  -> read_snapshot()                          [blocking, clipboard watcher thread]
      -> ctx.has(ContentFormat::Image) = true
      -> ctx.get_image()                      [reads TIFF from pasteboard, decodes to DynamicImage]
      -> img.to_png()                         [~3s: re-encodes DynamicImage to PNG bytes]
      -> reps.push(image/png, png_bytes)      [~3-5MB]
      -> raw fallback loop:
          -> get_buffer("public.tiff")        [34MB copy #1]
          -> get_buffer("NeXT TIFF v4.0 pasteboard type")  [34MB copy #2]
      -> return snapshot with all reps        [~71MB total]
```

### Optimized Pipeline (After)

```
on_clipboard_change()
  -> read_snapshot()                          [fast, clipboard watcher thread]
      -> ctx.has(ContentFormat::Image) = true
      -> ctx.get_buffer("public.tiff")        [read raw TIFF once, 34MB]
      -> reps.push(image/tiff, tiff_bytes)    [34MB, no transcode]
      -> raw fallback loop:
          -> skip "public.tiff" (already read)
          -> skip "NeXT TIFF v4.0 pasteboard type" (known alias)
          -> skip other known image aliases
      -> return snapshot with reps            [~34MB total]
  -> CaptureClipboardUseCase
      -> normalize -> Staged (large non-text)
      -> spool_queue.enqueue(tiff_bytes)
      -> spool worker: TIFF->PNG conversion   [async, non-blocking]
```

### Pattern 1: TIFF Alias Deduplication

**What:** Maintain a set of known macOS TIFF aliases to skip in the raw fallback loop
**When to use:** After reading image via the optimized path
**Example:**

```rust
// Source: clipboard-rs macOS source + Apple pasteboard documentation
#[cfg(target_os = "macos")]
const TIFF_ALIASES: &[&str] = &[
    "public.tiff",
    "NeXT TIFF v4.0 pasteboard type",
];

fn should_skip_raw_format(format_id: &str, image_already_read: bool) -> bool {
    // ... existing skips ...

    #[cfg(target_os = "macos")]
    if image_already_read {
        if TIFF_ALIASES.iter().any(|alias| format_id == *alias) {
            return true;
        }
    }

    false
}
```

### Pattern 2: Direct TIFF Read (Skip clipboard-rs get_image)

**What:** Read TIFF bytes directly via `get_buffer()` instead of going through `get_image()` + `to_png()`
**When to use:** On macOS when `ContentFormat::Image` is available
**Example:**

```rust
#[cfg(target_os = "macos")]
if ctx.has(ContentFormat::Image) {
    // Read raw TIFF directly — avoids decode+re-encode cycle
    match ctx.get_buffer("public.tiff") {
        Ok(tiff_bytes) => {
            debug!(
                format_id = "public.tiff",
                size_bytes = tiff_bytes.len(),
                "Read image as raw TIFF (skipping PNG transcode)"
            );
            reps.push(ObservedClipboardRepresentation {
                id: RepresentationId::new(),
                format_id: "image".into(),
                mime: Some(MimeType("image/tiff".to_string())),
                bytes: tiff_bytes,
            });
        }
        Err(_) => {
            // Fallback: try PNG via get_buffer, then clipboard-rs get_image()
            // ...
        }
    }
}
```

### Pattern 3: Deferred PNG Conversion in Spool Worker

**What:** Convert TIFF to PNG asynchronously in the background spool worker
**When to use:** When downstream consumers (dashboard display, sync) need PNG
**Example:**

```rust
// In spool worker, when processing a staged image/tiff representation:
fn convert_tiff_to_png(tiff_bytes: &[u8]) -> Result<Vec<u8>> {
    let image = image::load_from_memory(tiff_bytes)?;
    let mut png_bytes = Vec::new();
    image.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )?;
    Ok(png_bytes)
}
```

### Anti-Patterns to Avoid

- **Eagerly converting TIFF to PNG on watcher thread:** This is the current bottleneck. The watcher thread must remain fast.
- **Reading all raw formats blindly:** The raw fallback loop should be aware of image aliases to avoid duplicate reads.
- **Holding all buffers simultaneously:** If TIFF is read directly, the old `get_image()` path should not also run.

## Don't Hand-Roll

| Problem                    | Don't Build         | Use Instead                            | Why                                                              |
| -------------------------- | ------------------- | -------------------------------------- | ---------------------------------------------------------------- |
| TIFF decoding              | Custom TIFF parser  | `image` crate (already transitive dep) | TIFF is complex (multiple compression schemes, multi-page, etc.) |
| PNG encoding               | Custom PNG encoder  | `image` crate                          | Correct PNG encoding with proper filtering/compression           |
| Clipboard format detection | Custom UTI resolver | Hardcoded alias list                   | macOS TIFF aliases are stable and well-known                     |

## Common Pitfalls

### Pitfall 1: Clipboard Lock Duration

**What goes wrong:** Holding the clipboard `Mutex<ClipboardContext>` lock during slow operations blocks other clipboard operations.
**Why it happens:** `read_snapshot()` holds the lock for the entire duration, including any image processing.
**How to avoid:** Ensure the optimized path does not introduce any new slow operations inside the lock. The TIFF `get_buffer()` call is a fast memcpy from NSPasteboard, not a decode operation.
**Warning signs:** Clipboard change events being dropped or delayed.

### Pitfall 2: Format Ordering Assumptions

**What goes wrong:** Assuming `public.tiff` always exists when `ContentFormat::Image` is true.
**Why it happens:** Some apps (e.g., Pages v5+) put only `public.png` on the pasteboard, not TIFF.
**How to avoid:** Try `public.tiff` first, then fallback to `public.png` via `get_buffer("public.png")`, then final fallback to `get_image()` (the old slow path).
**Warning signs:** Missing image representations for specific source applications.

### Pitfall 3: Breaking Downstream MIME Expectations

**What goes wrong:** Downstream code (sync protocol, dashboard display) expects `image/png` MIME type but receives `image/tiff`.
**Why it happens:** The current pipeline always produces PNG; changing to TIFF requires updating all consumers.
**How to avoid:** Audit all code that checks for `image/png` MIME type. Key locations:

- `write_snapshot()` in common.rs (line 269: `Some("image/png")`)
- Frontend image display (may need to handle TIFF or receive converted PNG)
- V3 sync outbound encoder
- Dashboard blob URL resolver

### Pitfall 4: Windows/Linux Regression

**What goes wrong:** macOS-specific optimizations accidentally changing behavior on Windows/Linux.
**Why it happens:** `common.rs` is shared across all platforms.
**How to avoid:** Use `#[cfg(target_os = "macos")]` guards for macOS-specific paths. Windows already has its own DIB->PNG path in `platform/windows.rs`.

### Pitfall 5: Spool Worker Not Converting TIFF to PNG

**What goes wrong:** Image stored as TIFF blob but never converted; frontend can't display it.
**Why it happens:** Current spool worker just writes bytes to blob store without format conversion.
**How to avoid:** Either (a) convert TIFF->PNG in spool worker before blob store write, or (b) ensure frontend/backend can serve TIFF directly and browser can render it. Option (a) is simpler since browsers universally support PNG but TIFF support varies.

## Code Examples

### Critical: clipboard-rs get_image() on macOS (Source: clipboard-rs 0.3.3 source)

```rust
// clipboard-rs/src/platform/macos.rs lines 306-321
fn get_image(&self) -> Result<RustImageData> {
    autoreleasepool(|_| {
        // First try: read PNG directly from pasteboard
        let png_data = self.pasteboard.dataForType(unsafe { NSPasteboardTypePNG });
        if let Some(data) = png_data {
            return RustImageData::from_bytes(&data.to_vec());  // decode PNG -> DynamicImage
        };
        // Fallback: create NSImage, get TIFFRepresentation
        let ns_image = NSImage::initWithPasteboard(NSImage::alloc(), &self.pasteboard);
        if let Some(image) = ns_image {
            let tiff_data = image.TIFFRepresentation();
            if let Some(data) = tiff_data {
                return RustImageData::from_bytes(&data.to_vec());  // decode TIFF -> DynamicImage
            }
        };
        Err("no image data".into())
    })
}
```

**Key insight:** `get_image()` always decodes into `DynamicImage`. Then `to_png()` re-encodes. For TIFF input this is a full pixel-level decode+re-encode cycle. By reading raw bytes via `get_buffer("public.tiff")`, we skip both the decode and encode steps entirely.

### Critical: Current raw fallback loop (Source: common.rs lines 172-205)

```rust
// Raw fallback — reads ALL available formats not already covered
let seen: HashSet<String> = reps.iter().map(|r| r.format_id.to_string()).collect();

for format_id in available {
    if seen.contains(&format_id) { continue; }
    if should_skip_raw_format(&format_id) { continue; }
    match ctx.get_buffer(&format_id) {
        Ok(buf) => {
            reps.push(ObservedClipboardRepresentation { ... });
        }
        Err(err) => { warn!(...); }
    }
}
```

**Problem:** `seen` contains `"image"` (from the high-level path), but the available formats list contains `"public.tiff"` and `"NeXT TIFF v4.0 pasteboard type"` — these are NOT in `seen`, so both get read as separate raw buffers.

### Critical: ContentFormat::Image detection on macOS (Source: clipboard-rs macOS)

```rust
// clipboard-rs checks for BOTH PNG and TIFF
ContentFormat::Image => {
    let types = NSArray::from_retained_slice(&[
        unsafe { NSPasteboardTypePNG }.to_owned(),
        unsafe { NSPasteboardTypeTIFF }.to_owned(),
    ]);
    self.pasteboard.availableTypeFromArray(&types).is_some()
}
```

## State of the Art

| Old Approach         | Current Approach          | When Changed      | Impact                                 |
| -------------------- | ------------------------- | ----------------- | -------------------------------------- |
| No image support     | TIFF->PNG eager transcode | Phase 5 (2026-03) | 3s latency, 71MB peak memory per image |
| Read all raw formats | Read all raw formats      | Phase 5 (2026-03) | Duplicate TIFF reads                   |

**Key context:** The current image pipeline was built in Phase 5 to "get it working." Phase 9 optimizes it for production-quality performance.

## Open Questions

1. **Should we store TIFF or PNG in blob store?**
   - What we know: TIFF is raw from clipboard (no transcode needed), PNG is web-friendly.
   - What's unclear: Whether frontend needs PNG specifically or can handle TIFF.
   - Recommendation: Convert TIFF->PNG in spool worker. Store PNG in blob store. This keeps frontend simple and reduces stored blob size (PNG compression is typically 5-10x smaller than uncompressed TIFF).

2. **Are there other macOS clipboard image aliases beyond the two TIFF ones?**
   - What we know: `public.tiff` and `NeXT TIFF v4.0 pasteboard type` are confirmed aliases. Some apps also put `public.png` on the pasteboard.
   - What's unclear: Whether `com.apple.pict` or other legacy types appear in practice.
   - Recommendation: Log all skipped formats at debug level during the first implementation. Can add more aliases later based on real-world data from clipboard-probe.

3. **Should we keep the raw fallback loop for image clipboard events?**
   - What we know: Raw buffers for image-heavy clipboards are mostly TIFF aliases that provide no additional value.
   - What's unclear: Whether any apps put useful non-TIFF/non-PNG image data on the pasteboard.
   - Recommendation: Skip all known image format UTIs (`public.tiff`, `public.png`, `NeXT TIFF v4.0 pasteboard type`, etc.) when an image has already been captured via the optimized path. Keep raw loop for non-image formats.

## Sources

### Primary (HIGH confidence)

- clipboard-rs 0.3.3 source code at `/Users/mark/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipboard-rs-0.3.3/src/platform/macos.rs` — verified `get_image()` path, `has(ContentFormat::Image)` logic, and `available_formats()` behavior
- clipboard-rs 0.3.3 `common.rs` — verified `RustImageData::from_bytes()` and `to_png()` implementation
- Project source `uc-platform/src/clipboard/common.rs` — verified current `read_snapshot()` implementation and raw fallback loop
- Project source `uc-platform/src/clipboard/watcher.rs` — verified watcher calls `read_snapshot()` synchronously

### Secondary (MEDIUM confidence)

- Apple NSPasteboard documentation — TIFF as default image format, `NSPasteboardTypeTIFF` and `NSPasteboardTypePNG` constants
- W3C clipboard-apis issue #137 — browser pasteboard format ordering on macOS

### Tertiary (LOW confidence)

- Web search results on macOS TIFF alias names — community-reported aliases may not be exhaustive

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - no new dependencies needed, all existing libraries
- Architecture: HIGH - verified exact code paths in clipboard-rs source and project code
- Pitfalls: HIGH - derived from source code analysis, not speculation

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable domain, clipboard-rs unlikely to change)
