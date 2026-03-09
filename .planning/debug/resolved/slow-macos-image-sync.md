---
status: resolved
trigger: 'Copying images from macOS to Windows is noticeably slow (several seconds for moderate images), while Windows to macOS is fast.'
created: 2026-03-09T00:00:00Z
updated: 2026-03-09T00:00:00Z
---

## Current Focus

hypothesis: CONFIRMED - macOS TIFF fast path sends raw uncompressed TIFF (~18MB) without conversion
test: Traced full pipeline: capture -> encode -> encrypt -> transport -> decrypt -> decode -> write
expecting: Raw TIFF flows through entire pipeline untouched
next_action: Awaiting human verification of fix

## Symptoms

expected: Image sync from macOS to Windows should be fast (similar speed to Windows->macOS)
actual: macOS->Windows image sync takes several seconds for moderate images, while reverse direction is fast
errors: No errors, purely a performance issue
reproduction: 1. Set up clipboard sync between macOS and Windows. 2. Copy an image on macOS. 3. Wait for it to appear on Windows -> Slow. 4. Copy image on Windows -> Fast on macOS.
started: Ongoing issue. Recent commits b7ce3e62 and 55ff3cab fixed image correctness issues but performance issue remains.

## Eliminated

## Evidence

- timestamp: 2026-03-09
  checked: common.rs macOS fast path (lines 175-188)
  found: Raw public.tiff is read directly and stored with mime "image/tiff" - no conversion
  implication: Full uncompressed TIFF (~18MB for 3000x2000) enters the pipeline

- timestamp: 2026-03-09
  checked: sync_outbound.rs (lines 126-134)
  found: ALL representations are sent as-is, no format optimization
  implication: Raw TIFF bytes go straight into V3 binary payload

- timestamp: 2026-03-09
  checked: chunked_transfer.rs (lines 421-433)
  found: zstd compression applied for payloads > 8KB, but TIFF is mostly incompressible raw pixel data
  implication: Minimal compression savings on TIFF data

- timestamp: 2026-03-09
  checked: windows.rs write_image_windows (lines 263-268)
  found: image::load_from_memory decodes TIFF then converts to BMP for CF_DIB
  implication: CPU-intensive TIFF decode + BMP re-encode on Windows receiver

- timestamp: 2026-03-09
  checked: Windows capture path (non-macOS in common.rs, lines 259-289)
  found: Windows uses get_image()+to_png() which produces compressed PNG (~2-5MB)
  implication: Explains asymmetry - Windows->macOS is fast because PNG is small

## Resolution

root_cause: macOS TIFF fast path (common.rs:175-188) reads raw uncompressed TIFF from system clipboard and sends it as-is through the sync pipeline. A 3000x2000 image is ~18MB as TIFF vs ~2-5MB as PNG, causing 4-9x more data to transmit and expensive TIFF decode on Windows.

fix: Added tiff_to_png() conversion function in common.rs. The macOS fast path now reads public.tiff, converts to PNG immediately, and stores the result as image/png. Falls back to raw TIFF if conversion fails. This reduces payload by 80-90% and eliminates expensive TIFF decode on the receiver.

verification: cargo check --package uc-platform (clean), cargo test --package uc-platform -- clipboard (23 passed), cargo test --package uc-app -- sync_outbound (13 passed)

files_changed:

- src-tauri/crates/uc-platform/src/clipboard/common.rs
