# Stack Research

**Domain:** Cross-platform clipboard sync desktop app — new feature additions
**Researched:** 2026-03-02
**Confidence:** HIGH (all key recommendations verified via Context7 and official docs)

## Scope

This document covers only the **incremental stack additions** for the new milestone features. The existing base stack (Tauri 2, React 18, Rust, WebSocket via tokio-tungstenite, Redux Toolkit, Shadcn/ui) is documented in `.planning/codebase/STACK.md` and is not re-researched here.

---

## Recommended Stack

### Feature: Global Hotkey System

| Technology                           | Version             | Purpose                                      | Why Recommended                                                                                                                                                                                                      |
| ------------------------------------ | ------------------- | -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tauri-plugin-global-shortcut`       | 2.3.0               | Register system-wide hotkeys from Rust       | Official Tauri plugin, verified via Context7 and docs.rs. Must register on Rust side (not JS) so shortcuts work when the window is hidden/closed. JS-side registration stops working when the window is not visible. |
| `@tauri-apps/plugin-global-shortcut` | matching JS binding | Frontend shortcut event listener (secondary) | Only needed if frontend needs to react to shortcuts directly. Primary registration must be Rust-side for a quick-paste use case.                                                                                     |

**Critical architectural note:** Register the hotkey in the Tauri `.setup()` handler using the Rust API (`tauri_plugin_global_shortcut::Builder`), not in the frontend JavaScript. The JS `register()` API does not fire when no Tauri window is open. Since the quick-paste window is hidden by default, the shortcut must be alive independently of any window state.

```rust
// Correct pattern — register in main Tauri setup
app.handle().plugin(
    tauri_plugin_global_shortcut::Builder::new()
        .with_shortcuts(["alt+space", "ctrl+shift+v"])?
        .with_handler(|app, shortcut, event| {
            if event.state == ShortcutState::Pressed {
                if shortcut.matches(Modifiers::ALT, Code::Space) {
                    // show floating window
                    let _ = app.emit("show-quick-paste", ());
                }
            }
        })
        .build(),
)?;
```

### Feature: Floating / Overlay Quick-Paste Window

| Technology                   | Version                | Purpose                                                                                                        | Why Recommended                                                                                                                                                                                                                            |
| ---------------------------- | ---------------------- | -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Tauri `WebviewWindowBuilder` | built-in (Tauri 2.9.x) | Create the floating window with `.transparent(true).decorations(false).always_on_top(true).skip_taskbar(true)` | No additional dependency. Native Tauri API. Sufficient for Windows and Linux where NSPanel is not a concern.                                                                                                                               |
| `tauri-nspanel`              | git `v2.1` branch      | macOS-specific: converts window to `NSPanel` so it overlays fullscreen apps                                    | `WebviewWindowBuilder` alone does NOT overlay macOS fullscreen spaces. `NSPanel` is the correct macOS primitive for this. Used in production by EcoPaste (clipboard manager), Cap, Screenpipe. Not yet on crates.io — Git dependency only. |

**Platform strategy:**

- **macOS**: Use `tauri-nspanel` to convert the floating window to `NSPanel` with `is_floating_panel: true`. This is required for the window to appear over fullscreen apps (Mission Control, fullscreen terminals, etc.).
- **Windows / Linux**: `WebviewWindowBuilder` with `always_on_top(true)` and `skip_taskbar(true)` is sufficient.

```toml
# Cargo.toml — macOS-conditional dependency
[target.'cfg(target_os = "macos")'.dependencies]
tauri-nspanel = { git = "https://github.com/ahkohd/tauri-nspanel", branch = "v2.1" }
```

**Known limitation of `tauri-nspanel`:** Cannot call `window.is_maximized()` on a panel — it will crash. Do not use `tauri-plugin-window-state` on the floating panel window. This is a documented issue.

### Feature: Auto-Paste to Previous Application

| Technology               | Version                  | Purpose                                                                                          | Why Recommended                                                                                                                                                                                                 |
| ------------------------ | ------------------------ | ------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `enigo`                  | 0.5.0                    | Simulate `Cmd+V` / `Ctrl+V` keyboard input after restoring previous app focus                    | Only cross-platform Rust crate for programmatic keyboard simulation. Actively maintained. Verified version 0.5.0 released 2025. Supports macOS, Windows, Linux (X11 + experimental Wayland).                    |
| `objc2-app-kit`          | 0.3 (already in project) | macOS: get `NSWorkspace.frontmostApplication` before focus changes; store previous app reference | Already a project dependency. `NSWorkspace` exposes `frontmostApplication` and `NSWorkspaceDidActivateApplicationNotification`. Use this to record the previous app before the quick-paste window steals focus. |
| `windows-sys` / `winapi` | 0.3 (already in project) | Windows: `GetForegroundWindow` / `SetForegroundWindow` to restore focus                          | Already in project via `winapi`. No new dependency needed for Windows focus tracking.                                                                                                                           |

**Flow for auto-paste:**

1. Subscribe to `NSWorkspaceDidActivateApplicationNotification` (macOS) or poll `GetForegroundWindow` (Windows) to track the last non-UniClipboard focused app.
2. When the user selects an item in the quick-paste window, write to clipboard, then:
   - macOS: call `NSRunningApplication.activate(options:)` on the stored previous app, then enigo simulates `Cmd+V`.
   - Windows: `SetForegroundWindow(prev_hwnd)`, then enigo simulates `Ctrl+V`.
3. Hide the quick-paste window.

```toml
[dependencies]
enigo = "0.5"
```

### Feature: Image Clipboard Capture and Sync

| Technology                          | Version                             | Purpose                                                                             | Why Recommended                                                                                                                                                                                              |
| ----------------------------------- | ----------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `arboard` with `image-data` feature | 3.6.1                               | Read `ImageData` from clipboard (raw RGBA pixels + dimensions)                      | Already in project at 3.4. Upgrade to 3.6.1 for improved Windows PNG compatibility and file list support. The `image-data` feature provides `get_image()` → `ImageData { width, height, bytes: Cow<[u8]> }`. |
| `image` crate                       | 0.25.9 (already in project at 0.25) | Encode RGBA bytes → PNG for storage and transmission; decode PNG → RGBA for display | Already in project. PNG is the correct wire format: lossless, universally supported, handles transparency. Use `image::RgbaImage` + `image::codecs::png::PngEncoder`.                                        |

**Data flow for image capture:**

1. `arboard::Clipboard::get_image()` returns raw RGBA pixels.
2. Encode to PNG bytes using `image` crate for storage in SQLite (as blob) and for WebSocket transmission.
3. On receive: decode PNG bytes back to RGBA, write via `arboard::Clipboard::set_image(ImageData { ... })`.

**Linux Wayland note:** `arboard`'s Wayland support (`wayland-data-control` feature) is experimental. Prioritize X11 / XWayland for now. Document this as a known gap.

**`image-data` feature activation:**

```toml
arboard = { version = "3.6", features = ["image-data"] }
```

### Feature: Chunked Binary Transfer over WebSocket

| Technology             | Version                                           | Purpose                                                                                                                           | Why Recommended                                                                                                                                                            |
| ---------------------- | ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tokio-tungstenite`    | 0.28.0 (currently in project via libp2p / direct) | Send/receive `Message::Binary(Vec<u8>)` frames; configure `max_message_size` and `max_frame_size`                                 | Already the WebSocket library in use. Binary frame support is native. No new dependency for chunked transfer — implement chunking as an application-layer protocol on top. |
| `serde` + `serde_json` | 1 (already in project)                            | Encode chunk metadata (sequence number, total chunks, transfer ID, checksum) as JSON in a control frame before the binary payload | Already in project. Sufficient for the control envelope.                                                                                                                   |

**Chunked transfer design (application layer):**

WebSocket frames have no built-in fragmentation reassembly at the application level — the protocol itself supports fragmentation but tungstenite reassembles frames into complete messages by default. For large images (multi-MB), the correct approach is:

1. Split the encrypted binary payload into fixed-size chunks (e.g., 64 KB each).
2. Send a JSON `TransferStart` control message first: `{ id, total_chunks, total_bytes, content_type }`.
3. Send each chunk as `Message::Binary(chunk_bytes)` with a sequence header prepended as the first N bytes (fixed-width, e.g., 4 bytes = chunk index).
4. Send a `TransferComplete` control message with a Blake3 checksum for integrity verification.
5. Receiver buffers chunks by transfer ID, reassembles after all arrive, verifies checksum.

Configure tungstenite for large payloads:

```rust
let config = WebSocketConfig {
    max_message_size: Some(64 * 1024 * 1024),  // 64 MB
    max_frame_size: Some(4 * 1024 * 1024),      // 4 MB per frame
    ..Default::default()
};
```

**Why not WebSocket built-in fragmentation?** Tungstenite reassembles fragmented messages transparently before delivering to application code. The application never sees partial frames. Therefore chunking must be implemented at the application protocol level, not the WebSocket frame level.

---

## Supporting Libraries (New Additions Only)

| Library                              | Version    | Purpose                             | When to Use                                                |
| ------------------------------------ | ---------- | ----------------------------------- | ---------------------------------------------------------- |
| `tauri-plugin-global-shortcut`       | 2.3.0      | System-wide hotkey registration     | Always — required for quick-paste trigger                  |
| `@tauri-apps/plugin-global-shortcut` | latest npm | Frontend event bridge for shortcuts | If frontend needs to react to shortcuts                    |
| `tauri-nspanel`                      | git v2.1   | macOS panel window type             | macOS only, required for fullscreen overlay                |
| `enigo`                              | 0.5.0      | Cross-platform keyboard simulation  | Required for auto-paste after focus restore                |
| `arboard` (upgrade)                  | 3.6.1      | Image clipboard read/write          | Already in project — upgrade + enable `image-data` feature |

---

## Alternatives Considered

| Recommended                                | Alternative                                                    | Why Not                                                                                                                                                                                         |
| ------------------------------------------ | -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tauri-plugin-global-shortcut` (Rust-side) | JS-side `@tauri-apps/plugin-global-shortcut`                   | JS registration stops working when window is hidden. Quick-paste requires always-active hotkey independent of window state.                                                                     |
| `tauri-nspanel` (macOS)                    | `WebviewWindowBuilder` with `always_on_top(true)` only         | `always_on_top` does NOT overlay macOS fullscreen spaces. Users in fullscreen apps would get no response to the hotkey. `NSPanel` is the macOS-correct primitive.                               |
| `enigo`                                    | Raw platform APIs (`CGEvent` on macOS, `SendInput` on Windows) | `enigo` wraps both platforms in one dependency. Raw APIs require separate `unsafe` Rust implementations per platform, higher maintenance burden.                                                |
| `enigo`                                    | `tauri-plugin-shell` running `osascript`                       | Shell subprocess is slow (~200ms+), unreliable, requires permissions prompts. Unacceptable for <200ms target.                                                                                   |
| Application-layer chunking                 | WebSocket built-in fragmentation                               | Tungstenite reassembles WS fragments transparently — app never sees partial messages. App-layer chunking gives explicit control over progress tracking, retry logic, and checksum verification. |
| `image` crate for PNG encoding             | `png` crate directly                                           | `image` crate is already in project and provides a higher-level API. `png` crate is more low-level; no benefit for this use case.                                                               |

---

## What NOT to Use

| Avoid                                              | Why                                                                                                                                | Use Instead                                                                 |
| -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| JS-side global shortcut registration only          | Window must be alive for JS to run. Hidden windows stop receiving JS events.                                                       | Rust-side `tauri_plugin_global_shortcut::Builder` in `.setup()`             |
| `tauri-plugin-window-state` on the floating panel  | Crashes when applied to `NSPanel` windows (calls `is_maximized()` which panics on panels)                                          | Manually save/restore position if needed, skip for floating panel           |
| `aes-gcm` crate (already in Cargo.toml but unused) | Already flagged in CLAUDE.md as unused                                                                                             | `chacha20poly1305` already in project handles image encryption same as text |
| Wayland-native `arboard` feature                   | Experimental, not stable, many compositors don't support `wl-data-control` protocol                                                | Default X11/XWayland backend for now; Wayland is a future milestone item    |
| `clipboard-rs` for image capture                   | Already in project but less commonly used for image path; `arboard` with `image-data` feature is more idiomatic for raw pixel data | `arboard` 3.6.1 with `image-data` feature                                   |

---

## Installation

```bash
# Rust dependencies — add to src-tauri/Cargo.toml
cargo add tauri-plugin-global-shortcut --target 'cfg(any(target_os = "macos", windows, target_os = "linux"))'
cargo add enigo

# arboard upgrade (already present, add image-data feature)
# Edit Cargo.toml: arboard = { version = "3.6", features = ["image-data"] }

# macOS-only floating panel (add manually to Cargo.toml)
# [target.'cfg(target_os = "macos")'.dependencies]
# tauri-nspanel = { git = "https://github.com/ahkohd/tauri-nspanel", branch = "v2.1" }

# JS / Frontend
bun add @tauri-apps/plugin-global-shortcut
```

---

## Version Compatibility

| Package                              | Compatible With   | Notes                                                                                                                                                                                              |
| ------------------------------------ | ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tauri-plugin-global-shortcut@2.3.0` | `tauri@2.9.x`     | Part of tauri-apps plugins-workspace v2 branch. Compatible.                                                                                                                                        |
| `tauri-nspanel@v2.1 branch`          | `tauri@2.x`       | v2.1 branch targets Tauri 2. The older `v2` branch targets Tauri 2.0 stable. Use `v2.1` for Tauri 2.9.x.                                                                                           |
| `enigo@0.5.0`                        | Rust 2021 edition | Compatible with project's Rust edition. No known conflicts.                                                                                                                                        |
| `arboard@3.6.1`                      | `image@0.25.x`    | `arboard`'s `image-data` feature uses the `image` crate internally on Windows for bitmap decoding. Both versions must be compatible. Check for duplicate `image` crate versions with `cargo tree`. |
| `tokio-tungstenite@0.28.0`           | `tokio@1.28`      | Fully compatible. Current project already uses tokio 1.28 full.                                                                                                                                    |

---

## Stack Patterns by Variant

**If quick-paste window must appear over macOS fullscreen apps:**

- Use `tauri-nspanel` to convert to `NSPanel`
- Set `is_floating_panel: true` in the panel config
- This is the only option; `always_on_top` in Tauri does not work across Mission Control spaces

**If building for Windows / Linux only (no macOS):**

- Skip `tauri-nspanel`
- `WebviewWindowBuilder` with `.always_on_top(true).decorations(false).transparent(true).skip_taskbar(true)` is sufficient

**If image payloads are small (< 1 MB after encryption):**

- Skip chunked transfer implementation
- Send as single `Message::Binary` payload
- Add chunking only when images regularly exceed configurable threshold (recommend 256 KB default)

**If Wayland support is needed in a future milestone:**

- Enable `arboard`'s `wayland-data-control` feature
- Note: requires `wl-data-control` Wayland protocol support in compositor (GNOME 41+, KDE Plasma 5.18+)
- Not needed for this milestone

---

## Sources

- `/tauri-apps/tauri-plugin-global-shortcut` (Context7) — Rust-side registration API, Builder pattern, ShortcutState
- `/websites/rs_arboard` (Context7) — ImageData struct, `get_image()` / `set_image()`, image-data feature
- `/snapview/tokio-tungstenite` (Context7) — WebSocketConfig, max_message_size, binary message handling
- `/websites/v2_tauri_app` (Context7) — WebviewWindowBuilder, always_on_top, skip_taskbar, hide/show
- https://v2.tauri.app/plugin/global-shortcut/ — Official plugin docs, confirmed version 2.3.0
- https://github.com/ahkohd/tauri-nspanel — tauri-nspanel v2.1 branch, NSPanel usage, known limitations
- https://crates.io/crates/tauri-plugin-global-shortcut — Version 2.3.0 confirmed
- https://github.com/1Password/arboard/releases — arboard 3.6.1 latest release confirmed
- https://crates.io/crates/tokio-tungstenite — Version 0.28.0 confirmed latest
- https://crates.io/crates/image — image crate 0.25.9 confirmed latest
- https://crates.io/crates/enigo — enigo 0.5.0 confirmed latest (2025)
- https://github.com/enigo-rs/enigo — Cross-platform auto-paste simulation confirmed
- https://deepwiki.com/EcoPasteHub/EcoPaste — Real-world reference: EcoPaste uses tauri-nspanel for floating clipboard window
- https://docs.rs/objc2-app-kit/latest/objc2_app_kit/struct.NSWorkspace.html — NSWorkspace for previous app tracking (macOS)

---

_Stack research for: UniClipboard Desktop — quick-paste, image sync, chunked transfer, floating window_
_Researched: 2026-03-02_
