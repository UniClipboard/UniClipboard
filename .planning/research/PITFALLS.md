# Pitfalls Research

**Domain:** Cross-platform clipboard sync tool — floating window, image sync, chunked transfer, global hotkeys
**Researched:** 2026-03-02
**Confidence:** HIGH (Tauri focus bugs: official GitHub issues), HIGH (image format: arboard docs + official sources), MEDIUM (chunked transfer: WebSocket specs + community), HIGH (hotkey: official Tauri docs + GitHub issues)

## Critical Pitfalls

### Pitfall 1: Floating Window Steals Focus from Target Application on Hide

**What goes wrong:**
The quick-paste window is shown over the user's current app. When the user selects a clip and the window hides, focus does NOT automatically return to the previously active app. The user's keyboard input goes into a void — they must click back into their previous app before the paste lands.

**Why it happens:**
Tauri has open bugs on every major platform regarding focus management. `focusable: false` on macOS still steals focus (Tauri issue #14102, confirmed on Tauri 2.8.4/macOS 15). `focus: false` config is ignored on Windows (Tauri issue #7519). Linux does not focus windows correctly on creation in Tauri v2 (Tauri issue #10746). None of these have fixed releases as of 2026-03.

**How to avoid:**
Before showing the floating window, capture the currently active application handle using a platform-native call (macOS: `NSWorkspace.shared.frontmostApplication`, Windows: `GetForegroundWindow()`). Store it. After the user selects a clip and the window hides, programmatically re-activate the stored handle before writing to clipboard. Use `tauri-plugin-spotlight` on macOS — it handles this exact "hide and return focus" pattern and calls the right AppKit APIs. On Windows, call `SetForegroundWindow()` via a native Rust call before the simulated paste.

**Warning signs:**

- Developer tests the hotkey, sees the window appear, selects a clip, window disappears — but next keyboard input goes nowhere or into the floating window process
- Auto-paste pastes into the clipboard tool itself rather than the previous app
- Works on dev machine but breaks on other OS because developer only tested on one platform

**Phase to address:**
Floating window / quick-paste phase. Must be the first thing validated on each target platform before any other quick-paste work. Do not build the full clip list UI before verifying focus round-trip works.

---

### Pitfall 2: Auto-Paste Requires Accessibility Permissions on macOS (Hard User Requirement)

**What goes wrong:**
Auto-paste (simulating Cmd+V after returning focus) requires macOS Accessibility permissions. If the app does not have them, `CGEvent` keystroke injection silently does nothing. Users never see an error — the paste just fails. The permission dialog is buried in System Preferences and users frequently miss or deny it.

**Why it happens:**
macOS blocks programmatic keystroke injection into other applications without explicit user consent via the Accessibility API. This is a hard OS-level constraint, not a Tauri limitation. The Tauri binary itself (not a wrapper) must appear in System Preferences → Privacy & Security → Accessibility.

**How to avoid:**
At first launch or first use of auto-paste, check `AXIsProcessTrusted()`. If it returns false, surface a prominent onboarding prompt that explains WHY the permission is needed (so users don't deny it out of suspicion) and opens the correct System Preferences pane directly (`com.apple.preference.security?Privacy_Accessibility`). Fall back gracefully: if the permission is absent, write to clipboard but skip the simulated Cmd+V, show a toast telling the user to paste manually. Do not silently do nothing.

**Warning signs:**

- Auto-paste works when running as root or in development but fails for normal users
- No error surfaces — the clip appears copied but never pasted
- Works fine on Windows but breaks on macOS

**Phase to address:**
Floating window / quick-paste phase. Build the permission check and onboarding flow as a first-class feature, not an afterthought. Test specifically with a fresh macOS user account that has never granted Accessibility permissions.

---

### Pitfall 3: macOS Clipboard Gives TIFF, Not PNG

**What goes wrong:**
When the user copies a PNG or JPEG screenshot on macOS, the system clipboard stores it as `public.tiff` (NeXT TIFF v4.0), not as the original format. Code that reads the clipboard and expects PNG-encoded bytes will receive TIFF bytes and either display corruption or fail to encode for network transfer.

**Why it happens:**
macOS uses `NSImage` internally, which always serializes to TIFF when placed on the clipboard. There is no user-accessible way to change this. arboard abstracts over it to return raw RGBA pixels via `ImageData`, but if code bypasses arboard and reads the clipboard directly (e.g., via `NSPasteboard`), TIFF bytes are returned.

**How to avoid:**
Always read clipboard images through arboard's `ImageData` struct (raw RGBA pixel array + dimensions), never assume a specific encoded format. Before network transfer, explicitly encode the raw pixels to PNG using the `image` crate (`image::RgbaImage::from_raw()` + `image::codecs::png::PngEncoder`). Store the encoded PNG bytes, not the raw RGBA, in the database. On receive, decode PNG back to raw RGBA before writing to the local clipboard via arboard.

**Warning signs:**

- Image previews in the UI appear corrupted on macOS but fine on Windows
- Network-transferred images display as grey or zero-byte content
- `bytes.len()` is much smaller or larger than `width * height * 4` (indicates encoded format, not raw RGBA)
- MIME type detection returns `image/tiff` unexpectedly

**Phase to address:**
Image capture phase (before image sync). The capture-to-database pipeline must normalize to PNG at write time, not defer encoding to sync time. The existing TODO in `src/api/clipboardItems.ts` line 166 ("treating all entries as text") must be resolved here.

---

### Pitfall 4: Linux Clipboard Data Vanishes When App Loses Selection Ownership

**What goes wrong:**
On Linux (X11 and Wayland), clipboard contents are owned and served by the copying process. When the clipboard sync app shows its floating window and focus shifts, the previously copied item from another app is no longer being served. Clipboard managers that rely on reading content on-demand rather than at copy-time will get empty or stale data.

**Why it happens:**
X11/Wayland clipboard model: data stays inside the originating process until another process requests it. There is no persistent clipboard buffer at the OS level (unlike macOS and Windows). If the originating app exits or the selection ownership transfers, the data is gone unless a clipboard manager daemon has already made a copy.

**How to avoid:**
Capture clipboard content immediately at the `ClipboardChanged` event in `ClipboardWatcher` — do not defer reading. Do not try to re-read clipboard contents after showing the floating window, because selection ownership may have already shifted. For the quick-paste scenario: all items shown must come from the local database (already captured), never from a live clipboard read.

Separately: when writing to the clipboard via arboard on Linux, do not drop the `Clipboard` object immediately. Keep it alive until a separate process (or clipboard manager) has had time to read it. Use arboard's `wait` method if synchronous confirmation is needed.

**Warning signs:**

- Clipboard items appear empty in the floating window on Linux
- Items captured on macOS/Windows sync correctly but Linux items are empty
- Content disappears if the user copies something and immediately switches focus

**Phase to address:**
Clipboard capture phase. The existing `ClipboardWatcher` must be verified to capture-on-change (not on-demand) before Linux image sync is built. Wayland support requires `arboard` feature flag `wayland-data-control` enabled in `Cargo.toml`.

---

### Pitfall 5: Global Hotkey Registration Silently Fails When Conflicted or Permission Missing

**What goes wrong:**
The global hotkey (e.g., `Ctrl+Shift+V`) silently does nothing if: (a) another app already owns that key combo, (b) the required Tauri capability permissions are missing from the config, (c) the `#[cfg(desktop)]` guard is absent. Users think the feature is broken. Developers see no error in logs because the registration appears to succeed.

**Why it happens:**
`tauri-plugin-global-shortcut` returns success even when the underlying OS registration fails due to conflicts (varies by platform). Capability permissions (`global-shortcut:allow-register`) must be explicitly declared — missing them causes silent no-ops. The plugin is desktop-only and will fail at build time on mobile targets without the `#[cfg(desktop)]` guard.

**How to avoid:**
Always check the return value of `register()` and log failures explicitly. Provide a user-visible indication when the hotkey cannot be registered (e.g., settings page shows "Hotkey conflict detected — please choose a different shortcut"). Make the hotkey user-configurable from the Settings UI so users can resolve conflicts themselves. Use platform-appropriate defaults: `Cmd+Shift+V` on macOS, `Ctrl+Shift+V` on Windows/Linux. Verify capability permissions are declared in `src-tauri/capabilities/` before testing.

**Warning signs:**

- Hotkey works in development (where another app does not own the shortcut) but not on user machines
- No error logged but hotkey callback never fires
- Works after system restart (meaning another app claims the key on login)

**Phase to address:**
Global hotkey phase. Implement conflict detection and user-configurable fallback before shipping. Do not hard-code a single shortcut string that ignores `Command` vs `Ctrl` difference.

---

### Pitfall 6: Floating Window Created/Destroyed Per-Show Instead of Hidden/Shown

**What goes wrong:**
The quick-paste window takes 300–800ms to appear after the hotkey fires because the WebView is being re-initialized on every show. The project requirement is <200ms. Users perceive it as laggy and stop using the hotkey.

**Why it happens:**
Developers create a new `WebviewWindowBuilder` in the hotkey handler to keep code simple. WebView initialization (including JS engine startup, DOM parse, React hydration) takes hundreds of milliseconds even on fast machines. This is not a Tauri limitation — it applies to any WebView-based app.

**How to avoid:**
Pre-create the floating window at app startup with `visible: false`. On hotkey press, call `window.show()` + `window.set_focus()` only. The WebView is already warm. The hotkey handler must run in Rust (not JS) to avoid the IPC round-trip adding latency. Use `always_on_top: true` and `skip_taskbar: true` on the window config so it behaves like a panel overlay.

**Warning signs:**

- Window appears in 300ms+ on first hotkey press
- Performance improves on second press (window already exists and is cached by OS)
- Window flickers when re-creating

**Phase to address:**
Floating window phase. The window creation strategy must be decided before implementing window content.

---

## Technical Debt Patterns

| Shortcut                                                       | Immediate Benefit               | Long-term Cost                                                                                      | When Acceptable                                                        |
| -------------------------------------------------------------- | ------------------------------- | --------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| Read clipboard image as raw bytes without format normalization | Works for display in current OS | Transfer format mismatch across platforms; TIFF on macOS causes decode failures on Windows receiver | Never — normalize to PNG at capture time                               |
| Hard-code single global hotkey string                          | Simple implementation           | Silently broken on macOS (Cmd vs Ctrl) or conflicts with system apps                                | Never in production — always use `CommandOrControl` or platform-branch |
| Create floating window on each hotkey press                    | Simple code                     | Exceeds 200ms latency requirement; visible jank                                                     | Never — pre-create on startup                                          |
| Load full image into memory before chunking                    | Simple for small images         | OOM for 10MB+ clipboard images on mobile/embedded hosts                                             | Only with an enforced size cap (e.g., reject images >5MB)              |
| Send chunk sequence numbers as array index                     | Easy to implement               | No gap detection; receiver cannot distinguish "chunk 3 not yet arrived" from "no chunk 3 exists"    | Never — use explicit sequence + total-count metadata                   |

## Integration Gotchas

| Integration                       | Common Mistake                                               | Correct Approach                                                                                                                                |
| --------------------------------- | ------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| macOS NSWorkspace (previous app)  | Call after showing floating window                           | Call BEFORE showing floating window; store handle; restore after hide                                                                           |
| arboard on Linux                  | Drop `Clipboard` object after write                          | Keep `Clipboard` alive or use `wait()`; background thread must run long enough for other apps to read                                           |
| arboard `ImageData`               | Assume `bytes` field is PNG                                  | It is raw RGBA — must encode to PNG with `image` crate before storage or transfer                                                               |
| Tauri window focus config         | Trust `focus: false` / `focusable: false` in tauri.conf.json | These are broken on all platforms in Tauri 2.x; use programmatic Rust APIs (`WebviewWindowBuilder::focused(false)`) or `tauri-plugin-spotlight` |
| Tauri global-shortcut permissions | Forget capability declarations                               | Explicitly add `global-shortcut:allow-register` etc. to capability config files in `src-tauri/capabilities/`                                    |
| WebSocket binary send             | Base64-encode image bytes as text message                    | Send as binary frame (`Vec<u8>`) directly; Base64 adds 33% overhead                                                                             |

## Performance Traps

| Trap                                              | Symptoms                                                                               | Prevention                                                                                          | When It Breaks                                               |
| ------------------------------------------------- | -------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| Load entire image into memory before sending      | Memory spike visible in Activity Monitor; OOM crash on large screenshots (Retina 5MB+) | Stream in chunks; enforce max size per chunk (e.g., 64KB per frame); enforce total image size limit | First Retina screenshot (2–8MB) on a device with limited RAM |
| No backpressure on chunked send                   | Send buffer (`bufferedAmount`) grows unboundedly; connection drops                     | Check `bufferedAmount` before sending next chunk; wait for drain before continuing                  | Transfer of images >2MB on a slow LAN connection             |
| Reassemble all chunks in RAM before writing to DB | Same OOM risk as single-payload send                                                   | Stream chunks to DB incrementally; write blob row-by-row                                            | First large image sync                                       |
| WebView warm-up on every hotkey press             | 300–800ms window appearance latency                                                    | Pre-create hidden window at startup                                                                 | Always — first user interaction                              |

## Security Mistakes

| Mistake                                                                         | Risk                                                                                  | Prevention                                                                                                                |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Skip XChaCha20 encryption for image data "because it's binary anyway"           | Image content transmitted in cleartext; violates project constraint                   | Apply same encryption path as text — the existing `CaptureClipboardUseCase` must handle image representations identically |
| Include chunk sequence number in plaintext metadata outside encryption envelope | Sequence numbers reveal traffic patterns (attacker infers file sizes)                 | Encrypt entire chunk payload including sequence metadata; only expose chunk index at transport layer for ordering         |
| Trust that chunk ordering == arrival order                                      | Out-of-order chunks produce corrupt image reassembly silently                         | Use sequence numbers + total count; verify checksum after reassembly; reject and retry if checksum fails                  |
| Expose raw image dimensions before authentication                               | Side-channel: attacker can infer content type and approximate subject from dimensions | Include image dimensions only in the encrypted payload, not in transport headers                                          |

## UX Pitfalls

| Pitfall                                       | User Impact                                                  | Better Approach                                                                     |
| --------------------------------------------- | ------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| Show floating window at center of screen      | User must move eyes away from cursor to find the window      | Show at cursor position or at a consistent corner near the active monitor           |
| Dismiss floating window on any focus loss     | Window closes when user accidentally hovers a different app  | Dismiss only on Escape, clip selection, or explicit click outside the window bounds |
| No progress indicator during large image sync | User thinks sync is broken during 3-10 second image transfer | Show per-item sync progress in clip history list (spinner → checkmark)              |
| Show raw file path for image clipboard items  | Users see cryptic `/var/folders/.../blob_xyz` paths          | Show image thumbnail + dimensions; never expose internal storage paths in the UI    |
| Silently drop images above size limit         | User copies a large image, sync never happens, no feedback   | Surface a toast: "Image too large to sync (12MB, limit 8MB)"                        |

## "Looks Done But Isn't" Checklist

- [ ] **Floating window focus return:** Verify with a test: open TextEdit, type some text, press hotkey, select a clip — confirm the text appears in TextEdit. Do this on macOS, Windows, and Linux separately.
- [ ] **Auto-paste on macOS:** Test with a fresh macOS account that has never granted Accessibility permissions. Verify the onboarding prompt appears and works.
- [ ] **Image capture on macOS:** Copy a PNG from Preview, capture it in the app, open the saved representation from DB and verify it decodes as a valid image (not TIFF bytes).
- [ ] **Image capture on Linux Wayland:** Copy an image, immediately switch focus, verify the item appears in history (not empty).
- [ ] **Chunked transfer reassembly:** Send an image that requires exactly N+1 chunks (one chunk boundary case). Verify the assembled image is pixel-identical to the original using a hash comparison.
- [ ] **Global hotkey conflict:** Install Spotlight (macOS) or another app that uses the same key combo. Verify the app detects the conflict and surfaces a user-facing message rather than silently failing.
- [ ] **Hotkey fires from Rust handler, not JS:** Measure time from hotkey press to window visible. Must be <200ms on a mid-range machine.
- [ ] **Chunk transfer resumes after reconnect:** Drop and restore the network connection mid-transfer. Verify the transfer completes (or retries cleanly) rather than hanging.
- [ ] **Encryption of image data:** Capture an image, sync to second device, inspect the raw WebSocket traffic — verify it is not human-readable (confirms encryption active for binary data).
- [ ] **Linux Wayland clipboard data:** Enable `wayland-data-control` feature in arboard. Verify clipboard capture works on a Wayland session (not just XWayland).

## Recovery Strategies

| Pitfall                                              | Recovery Cost | Recovery Steps                                                                                                           |
| ---------------------------------------------------- | ------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Focus not returned to previous app                   | MEDIUM        | Add platform-native "previous app" capture/restore; requires platform-specific Rust code; ~1-2 days                      |
| Image stored as TIFF bytes instead of normalized PNG | HIGH          | Database migration required; existing image blobs must be re-encoded; adds migration step to release                     |
| Floating window created/destroyed (not hidden/shown) | LOW           | Refactor window initialization to app startup; window content unaffected                                                 |
| Hard-coded hotkey string                             | LOW           | Add platform branch (`#[cfg(target_os = "macos")]`) and settings UI for user override                                    |
| Chunked transfer without checksum                    | HIGH          | Protocol version bump required; existing in-progress transfers cannot be validated retroactively; all peers must upgrade |
| No backpressure on WebSocket send                    | MEDIUM        | Add `bufferedAmount` check before each chunk send; no protocol change needed                                             |

## Pitfall-to-Phase Mapping

| Pitfall                                    | Prevention Phase                       | Verification                                                                              |
| ------------------------------------------ | -------------------------------------- | ----------------------------------------------------------------------------------------- |
| Floating window steals focus               | Quick-paste floating window phase      | Manual test: hotkey → select clip → verify paste lands in previous app on macOS + Windows |
| Auto-paste needs Accessibility permissions | Quick-paste floating window phase      | Test on fresh macOS account with no prior permissions granted                             |
| macOS TIFF instead of PNG                  | Image clipboard capture phase          | Hash-compare captured image vs. original in test; verify PNG decode succeeds              |
| Linux clipboard ownership loss             | Image clipboard capture phase          | Test on Wayland session; verify capture on focus-switch scenario                          |
| Global hotkey silent failure               | Global hotkey registration phase       | Test with conflicting app installed; verify conflict surfaced to user                     |
| Window create/destroy latency              | Quick-paste floating window phase      | Measure hotkey-to-visible latency; must pass <200ms threshold                             |
| Large image OOM                            | Chunked binary transfer phase          | Transfer a 10MB image; monitor RSS before/after; must not spike above 2x image size       |
| Chunk reassembly without checksum          | Chunked binary transfer phase          | Corrupt one chunk byte in a test; verify receiver detects and retries                     |
| Missing encryption for image data          | Image capture + chunked transfer phase | Wireshark/network inspection of sync traffic; encrypted payloads must be opaque           |

## Sources

- [Tauri issue #14102 — `focusable: false` broken on macOS](https://github.com/tauri-apps/tauri/issues/14102)
- [Tauri issue #7519 — `focus` property ignored on Windows](https://github.com/tauri-apps/tauri/issues/7519)
- [Tauri issue #10746 — Window not properly focused on Linux v2](https://github.com/tauri-apps/tauri/issues/10746)
- [tauri-plugin-spotlight — Spotlight-style hide/focus-return for macOS](https://crates.io/crates/tauri-plugin-spotlight)
- [arboard — Cross-platform Rust clipboard library (1Password)](https://github.com/1Password/arboard)
- [arboard issue #62 — Image rendering from clipboard](https://github.com/1Password/arboard/issues/62)
- [tauri-plugin-global-shortcut — Official docs](https://v2.tauri.app/plugin/global-shortcut/)
- [Tauri discussion #10017 — Unable to register global shortcuts](https://github.com/tauri-apps/tauri/discussions/10017)
- [WebSocket chunking deep-dive — xjavascript.com](https://www.xjavascript.com/blog/chunking-websocket-transmission/)
- [websockets memory management — official docs](https://websockets.readthedocs.io/en/stable/topics/memory.html)
- [Alfred Clipboard History — Accessibility permissions pattern](https://www.alfredapp.com/help/features/clipboard/)
- [Paste.app — macOS Accessibility permission requirement](https://pasteapp.io/help/paste-on-mac)
- [clipboard-rs — Alternative Rust clipboard crate with PNG output](https://github.com/ChurchTao/clipboard-rs)

---

_Pitfalls research for: uniclipboard-desktop — floating window, image sync, chunked transfer, global hotkeys_
_Researched: 2026-03-02_
