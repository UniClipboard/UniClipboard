# Phase 5: 支持Windows平台的剪切板图片捕获 - Context

**Gathered:** 2026-03-05
**Status:** Ready for planning

<domain>
## Phase Boundary

Fix clipboard image capture on Windows. Currently, text capture works correctly on Windows and all clipboard features (text + image) work on macOS, but image capture fails silently on Windows. This phase makes image capture work reliably on Windows.

Scope: image **capture** (read from clipboard) only. Writing images back to Windows clipboard (for inbound sync from other devices) is out of scope — future phase.

</domain>

<decisions>
## Implementation Decisions

### Capture approach

- Prioritize fixing `clipboard-rs` library integration on Windows first
- Investigate why `clipboard-rs` image reading fails on Windows (works on macOS)
- If `clipboard-rs` cannot be fixed: fall back to integrating the existing native Windows API functions (`read_image_windows()` in `windows.rs`) which are already written but not called
- Goal is to keep cross-platform consistency through `clipboard-rs` if possible

### Scope

- Only image capture (reading from Windows clipboard) — not write-back
- Must work for the most common scenarios: screenshots (Win+Shift+S, Print Screen), copying images from browsers, copying from image editors
- Text capture must remain unaffected

### Claude's Discretion

- Image format support scope (CF_DIB only vs also alpha channel/transparency)
- Whether to skip `clipboard-rs` investigation and go straight to native API if evidence points to a known upstream limitation
- Error handling strategy for clipboard lock contention on Windows
- Diagnostic logging level for image capture failures
- Testing approach given primary development on macOS

</decisions>

<code_context>

## Existing Code Insights

### Reusable Assets

- `read_image_windows()` (`uc-platform/src/clipboard/platform/windows.rs:159-170`): Reads Windows clipboard as BMP, converts to PNG — already written, just needs integration
- `CommonClipboardImpl::read_snapshot()` (`uc-platform/src/clipboard/common.rs:137-162`): Current image capture path using `clipboard-rs` — this is what needs fixing
- `ClipboardWatcher` (`uc-platform/src/clipboard/watcher.rs`): Dedup logic already recognizes image representations by MIME type
- `ThumbnailGeneratorPort` (`uc-infra/src/clipboard/thumbnail_generator.rs`): WebP thumbnail generation — works with any PNG input

### Established Patterns

- Platform-specific code in `uc-platform/src/clipboard/platform/{macos,linux,windows}.rs`
- Common implementation in `common.rs` uses `clipboard-rs` crate for cross-platform clipboard access
- Images are always converted to PNG before storage
- `SystemClipboardSnapshot` with `ObservedClipboardRepresentation` (format_id: "image", mime: "image/png")

### Integration Points

- `SystemClipboardPort::read_snapshot()` — the trait method that returns clipboard content
- `clipboard-rs` crate dependency with `ContentFormat::Image` detection
- `clipboard-win` and `winapi` crates already in `uc-platform/Cargo.toml` as dependencies
- `image` crate already available for BMP→PNG conversion

</code_context>

<specifics>
## Specific Ideas

- The existing `read_image_windows()` function already handles BMP→PNG conversion using the `image` crate — if we fall back to native API, this code is ready to integrate
- `write_image_windows()` has retry logic for Windows clipboard lock contention (max 5 attempts, 10ms delay) — while write-back is out of scope, the retry pattern may be useful for read operations too
- The fix should be transparent to the rest of the system — upstream code just sees a `SystemClipboardSnapshot` with an image/png representation, same as macOS

</specifics>

<deferred>
## Deferred Ideas

- Writing images back to Windows clipboard (inbound sync) — future phase
- EMF/WMF vector format support — not needed for MVP
- Windows clipboard history integration (Win+V) — out of scope

</deferred>

---

_Phase: 05-windows_
_Context gathered: 2026-03-05_
