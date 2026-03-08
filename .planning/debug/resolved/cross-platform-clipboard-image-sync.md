---
status: resolved
trigger: 'Two clipboard image sync issues between macOS and Windows after merging dev branch'
created: 2026-03-08T00:00:00Z
updated: 2026-03-08T00:00:00Z
---

## Current Focus

hypothesis: Issue B root cause - clipboard-rs set_image() on Windows fails with OSError(1418) because clipboard-rs internal clipboard open/close is unreliable on Windows threads. write_image_windows() exists with proper retry logic but is never called.
test: Implement image write fallback in Windows write_snapshot to use write_image_windows when clipboard-rs fails
expecting: Image writes to Windows clipboard succeed via native fallback
next_action: Implement fix for Issue B, then investigate Issue A

## Symptoms

expected: Images should sync correctly between macOS and Windows in both directions
actual: |
Issue A (Windows->macOS): Windows outbound only captures text representations, no image
Issue B (macOS->Windows): macOS sends image/tiff to Windows, Windows fails with OSError(1418) "thread doesn't have open clipboard"
errors: |
Issue A: No explicit error, just only text captured on Windows
Issue B: "set png image error, code = OSError(1418)" followed by "Failed to apply inbound clipboard message"
reproduction: |
A: Copy image on Windows, sync to macOS - only text arrives
B: Copy image on macOS, sync to Windows - write fails with OS clipboard error
started: After merging dev branch. macOS<->macOS works fine.

## Eliminated

## Evidence

- timestamp: 2026-03-08T00:10:00Z
  checked: windows.rs write_snapshot code path for images
  found: |
  CommonClipboardImpl::write_snapshot uses clipboard-rs ctx.set_image() for all image MIME types.
  On Windows, this internally calls OpenClipboard/SetClipboardData/CloseClipboard.
  Error OSError(1418) = ERROR_CLIPBOARD_NOT_OPEN means SetClipboardData called without clipboard being open.
  write_image_windows() function exists (line 221) with proper retry logic for opening clipboard, but is NEVER CALLED.
  The fallback_eligible check (line 100) only triggers for text/plain, not images.
  implication: Image writes always go through clipboard-rs which has a Windows threading bug. The native fallback exists but is dead code.

- timestamp: 2026-03-08T00:12:00Z
  checked: common.rs write_snapshot TIFF handling
  found: |
  For image/\* MIME types, write_snapshot calls RustImageData::from_bytes(&rep.bytes) then ctx.set_image(img).
  RustImageData::from_bytes uses the image crate which supports TIFF decoding.
  The error "set png image error" means from_bytes succeeded but set_image failed.
  clipboard-rs set_image() converts to PNG internally and calls the Win32 API to set clipboard data.
  implication: The TIFF data is decoded successfully, the failure is in the Win32 clipboard write call.

- timestamp: 2026-03-08T00:15:00Z
  checked: Windows read_snapshot image capture path (Issue A analysis)
  found: |
  read_snapshot on Windows: CommonClipboardImpl first tries clipboard-rs ContentFormat::Image + get_image().
  If that fails, Windows-specific fallback reads CF_DIB via clipboard-win.
  If both fail, only text is captured.
  Cannot test Windows-specific clipboard behavior from macOS.
  implication: Issue A may be clipboard-rs not detecting image format on Windows, AND CF_DIB fallback also failing. Need Windows testing to confirm.

## Resolution

root_cause: |
Issue B (macOS->Windows image write failure): clipboard-rs's ctx.set_image() on Windows fails with
OSError(1418) because its internal clipboard open/close mechanism is unreliable on Windows threads.
The write_image_windows() function existed with proper Win32 clipboard handling but was NEVER CALLED -
it was dead code. The write_snapshot fallback logic only handled text/plain, not image MIME types.

Additionally, write_image_windows itself had a bug: it used both manual OpenClipboard() AND
clipboard-win's set_clipboard() (which opens clipboard internally), causing double-open conflicts.

Issue A (Windows->macOS no image captured): This requires Windows-side testing to confirm. The code
path exists (clipboard-rs get_image + CF_DIB fallback) but may fail for certain image sources on Windows.
This issue is NOT addressed in this fix and needs Windows-side investigation.

fix: |

1. Added image fallback path in WindowsClipboard::write_snapshot: when CommonClipboardImpl::write_snapshot
   fails for a single-image snapshot, drops the clipboard-rs mutex and falls back to write_image_windows().
2. Rewrote write_image_windows() to use clipboard-win's Clipboard struct with retry (new_attempts(10))
   and RawData(CF_DIB) format, replacing the buggy manual OpenClipboard + set_clipboard combination.
3. Added is_single_image_snapshot() helper function.
4. Removed unused winapi dependency from uc-platform/Cargo.toml.

verification: |

- cargo check passes on macOS (Windows code is cfg-gated)
- uc-core tests: 358 passed
- uc-app lib tests: 163 passed
- Human verified on real Windows device (2026-03-09)

files_changed:

- src-tauri/crates/uc-platform/src/clipboard/platform/windows.rs
- src-tauri/crates/uc-platform/Cargo.toml
