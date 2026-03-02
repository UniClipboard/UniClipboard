# Project Research Summary

**Project:** UniClipboard Desktop — Quick-Paste, Image Sync, Chunked Transfer Milestone
**Domain:** Cross-platform clipboard synchronization (desktop, LAN-first)
**Researched:** 2026-03-02
**Confidence:** HIGH

## Executive Summary

UniClipboard Desktop is adding three interconnected feature clusters to an existing Tauri 2 + React + Rust clipboard sync tool: a global-hotkey-triggered floating quick-paste window, image clipboard capture and cross-device sync, and a chunked binary transfer infrastructure. Research across stack, features, architecture, and pitfalls converges on a clear implementation approach: these features must be built in dependency order (image capture first, hotkey infrastructure second, floating window UI third, chunked transfer fourth), because each phase is a prerequisite for the next. The architecture work can reuse existing hexagonal architecture patterns almost entirely — no new architectural paradigms are needed, only new ports and adapters following established project conventions.

The recommended stack additions are minimal and precise: `tauri-plugin-global-shortcut` (Rust-side registration only — JS-side registration breaks when windows are hidden), `tauri-nspanel` (macOS only, required for fullscreen overlay), `enigo` (cross-platform keyboard simulation for auto-paste), and an arboard upgrade to 3.6.1 with the `image-data` feature. All other functionality builds on the existing stack. The chunked transfer protocol is application-layer only — tungstenite already handles WebSocket transport and reassembles frames transparently.

The two highest-risk areas are macOS-specific: floating window focus management (Tauri has confirmed open bugs on focus restoration across all platforms as of 2026-03) and auto-paste requiring macOS Accessibility permissions that users frequently miss or deny. Both must be addressed with platform-native workarounds in the floating window phase. A secondary risk is macOS TIFF image format normalization — the clipboard always returns TIFF on macOS, not PNG, and normalization must happen at capture time or database migration will be required later. Building the image capture pipeline correctly in Phase 1 avoids this expensive retroactive fix.

## Key Findings

### Recommended Stack

The existing stack handles nearly everything. New additions are surgical. See `.planning/research/STACK.md` for full rationale and version details.

**Core new technologies:**

- `tauri-plugin-global-shortcut` 2.3.0: OS-level hotkey registration — must register in Rust `.setup()` handler, not frontend JS, because JS registration stops working when no window is focused
- `tauri-nspanel` (git v2.1 branch): macOS NSPanel conversion — required for floating window to appear over fullscreen spaces; `always_on_top` alone does NOT work across Mission Control on macOS
- `enigo` 0.5.0: Cross-platform keyboard simulation — wraps macOS CGEvent and Windows SendInput; required for auto-paste after focus restore; `tauri-plugin-shell` alternative is too slow (~200ms+)
- `arboard` upgrade to 3.6.1 with `image-data` feature: Raw RGBA pixel read from clipboard; already in project at 3.4, upgrade needed for Windows PNG compatibility
- Application-layer chunked transfer over existing `tokio-tungstenite`: No new dependency; implement chunking as `ChunkEnvelope` protocol on top of existing WebSocket binary frames

**Critical version constraints:**

- `tauri-nspanel` v2.1 branch (not v2.0) for Tauri 2.9.x compatibility
- `arboard` + `image` crate versions must be compatible — run `cargo tree` to verify no duplicate `image` versions
- Do NOT use `tauri-plugin-window-state` on the floating panel window — it calls `is_maximized()` which crashes on NSPanel

### Expected Features

See `.planning/research/FEATURES.md` for full competitor analysis and prioritization matrix.

**Must have (P1 — this milestone):**

- Global hotkey triggers quick-paste floating window — entry point to entire feature set
- Quick-paste window appears in <200ms — users notice above this threshold; requires pre-warmed window
- Auto-paste to previously active application on selection — core workflow payoff
- Fuzzy search in quick-paste window — type immediately, no mouse required
- Image clipboard capture and thumbnail display — images as first-class content type
- Image synchronization over LAN — primary new sync feature; requires chunked transfer
- Chunked transfer infrastructure — prerequisite for image sync; also enables future file sync
- Pin/favorite entries — prevents useful items from rolling off history
- App exclusion list — security hygiene; password managers must not be logged
- Paste without formatting — strip HTML/RTF; one of most-requested clipboard manager features

**Should have (P2 — after P1 stable):**

- History search in main window — reuses fuzzy search built for quick-paste
- Type filter in search (text vs image) — low effort once search exists
- Transfer progress indicator — low effort on top of chunked transfer events
- Window position memory — polish

**Defer to v2+:**

- Manual categories / collections — pinning covers core need for now
- WebDAV cross-internet sync — already scoped to next milestone
- Local OCR search in images — Apple Vision / Tesseract feasible but not this milestone
- File sync — chunked transfer foundation built this milestone enables it; implement next milestone
- Cloud sync, AI features, mobile — explicitly anti-features for this product positioning

### Architecture Approach

The hexagonal architecture already in place accommodates all new features through new ports and adapters — no structural changes. The floating window uses a separate Tauri `WebviewWindow` (label: `quick-paste`) pre-created at startup and hidden/shown on hotkey press. The global hotkey flows through a new `HotkeyHandlerPort` mirroring the existing `ClipboardChangeHandler` pattern. Image capture is already partially implemented in `CommonClipboardImpl` — the missing pieces are use case wiring and blob persistence. Chunked transfer wraps the existing `ClipboardTransportPort` as a new `ChunkedTransportPort` adapter. See `.planning/research/ARCHITECTURE.md` for full component diagrams, data flows, and code examples.

**Major components (new):**

1. `GlobalShortcutAdapter` (uc-platform) — wraps `tauri-plugin-global-shortcut`, emits `PlatformCommand::HotkeyTriggered` to existing channel
2. `WindowManagerAdapter` (uc-platform) — wraps Tauri `AppHandle`, show/hide/position the quick-paste window
3. `QuickPastePage.tsx` (frontend) — separate React route at `/quick-paste`, independent RTK Query calls (no shared Redux store between windows)
4. `ChunkedSender` / `ChunkedReceiver` (uc-infra/network/chunked) — `ChunkEnvelope` protocol with UUID transfer ID, sequence number, total count, and encrypted bytes
5. `SyncOutbound` extension — routes image MIME representations through `ChunkedSender` instead of direct `ClipboardTransportPort`
6. New ports in uc-core: `HotkeyRegistrarPort`, `HotkeyHandlerPort`, `WindowManagerPort`, `ChunkedTransportPort`

**Key architectural decision:** Each Tauri WebviewWindow has an independent JavaScript renderer — the main window and quick-paste window do NOT share Redux state. Both must independently call `get_clipboard_entries` via RTK Query. Do not attempt to share the Redux store.

### Critical Pitfalls

See `.planning/research/PITFALLS.md` for full details, recovery strategies, and verification checklists.

1. **Floating window focus not returned to previous app** — Tauri has open bugs on focus management on all platforms (issues #14102, #7519, #10746). Use platform-native APIs: capture `NSWorkspace.shared.frontmostApplication` (macOS) or `GetForegroundWindow()` (Windows) BEFORE showing the floating window; restore programmatically after clip selection. Consider `tauri-plugin-spotlight` for macOS which handles this pattern correctly. Validate on each target OS before building clip list UI.

2. **Auto-paste requires macOS Accessibility permissions** — `CGEvent` keystroke injection silently does nothing without explicit user consent. Check `AXIsProcessTrusted()` at first use; surface onboarding prompt with direct link to System Preferences; fall back to clipboard-write-only without auto-paste if permission denied.

3. **macOS clipboard returns TIFF, not PNG** — Always read through arboard's `ImageData` (raw RGBA pixels), never assume encoded format. Encode to PNG immediately at capture time using `image` crate. If deferred, a database migration will be required — HIGH recovery cost.

4. **Global hotkey registration silently fails** — Plugin returns success even when OS-level conflict exists. Always log the return value; expose conflict detection in Settings UI; make hotkey user-configurable; add `global-shortcut:allow-register` to capability config files in `src-tauri/capabilities/`.

5. **Floating window created/destroyed instead of hidden/shown** — WebView init takes 300-800ms; the <200ms requirement means the window MUST be pre-created at startup with `visible: false`. Subsequent `window.show()` calls are instant.

6. **Linux clipboard ownership loss** — X11/Wayland has no persistent clipboard buffer; the originating process serves data. Capture immediately at `ClipboardChanged` event; never defer clipboard reading. Keep arboard `Clipboard` object alive after write.

## Implications for Roadmap

Based on research, the dependency graph dictates a four-phase structure. Each phase is a prerequisite for the next.

### Phase 1: Image Capture and Local Display

**Rationale:** No new dependencies required. Infrastructure already exists (`CommonClipboardImpl` reads images, `ThumbnailGenerator` exists, `BlobStorePort` exists). Completing this phase first establishes the correct image data model (PNG-normalized) before any sync or UI work builds on it. Getting the format normalization wrong here has HIGH recovery cost (database migration). Quick wins with observable UI improvement.

**Delivers:** Image clipboard entries appear as thumbnails in the main history list. Users see images in clipboard history for the first time.

**Addresses:** "Image clipboard capture and display" (P1 table stakes), "Image display in history list" (P1)

**Avoids:** macOS TIFF pitfall — normalize to PNG at capture time; verify at this phase before sync adds complexity

**Key tasks:**

- Wire `CaptureClipboard` use case to persist image representations through blob/spool pipeline
- Verify `ThumbnailGenerator` generates WebP previews for `image/png` entries
- Update `ListClipboardEntries` DTO to include `thumbnail_url`
- Update frontend `ClipboardEntryRow` to render `<img>` for image entries via `uc://thumbnail/<id>`

### Phase 2: Global Hotkey Infrastructure

**Rationale:** The quick-paste window needs a trigger before UI is built on top. Adding `tauri-plugin-global-shortcut` is a new Rust dependency that must be validated working (including capability permissions and conflict detection) before floating window work begins.

**Delivers:** OS-level hotkey fires reliably from background state, emits `HotkeyTriggered` event through platform channel. No visible UI yet — validation milestone only.

**Uses:** `tauri-plugin-global-shortcut` 2.3.0 (Rust-side registration in `.setup()`)

**Implements:** `HotkeyRegistrarPort`, `HotkeyHandlerPort` in uc-core; `GlobalShortcutAdapter` in uc-platform

**Avoids:** Silent hotkey failure pitfall — implement conflict detection and user-configurable shortcut before moving on; add capability permissions to `src-tauri/capabilities/`

### Phase 3: Quick-Paste Floating Window

**Rationale:** Requires Phase 2 (hotkey trigger) and Phase 1 (images to display in list). This is the highest-risk phase due to Tauri focus management bugs. Focus return, auto-paste permissions, and window show/hide latency must all be solved and validated on each target platform before shipping.

**Delivers:** Press hotkey → floating window appears at cursor position in <200ms → select item → item pastes into previous app. The core quick-paste workflow.

**Uses:** `tauri-nspanel` (macOS fullscreen overlay), `enigo` 0.5.0 (auto-paste keyboard simulation), `objc2-app-kit` / `winapi` (previous app tracking — already in project)

**Implements:** `WindowManagerPort`, `TauriWindowManagerAdapter`, `QuickPastePage.tsx`, hotkey-to-window connection

**Avoids:** Focus-return pitfall (validate first on each OS), auto-paste Accessibility permissions (build onboarding flow), window creation latency (pre-create hidden at startup), window positioned at cursor (not screen center)

**Also delivers:** Keyboard-only navigation, fuzzy search, pinning support in quick-paste UI, paste-without-formatting option

### Phase 4: Chunked Transfer for Image Sync

**Rationale:** Depends on Phase 1 (image data model stable) and existing WebSocket transport being stable. This is infrastructure-only — no new visible UI surface until connected to sync. Build it after the UI phases are complete so the protocol design can be informed by real image sizes captured in Phase 1.

**Delivers:** Images sync reliably across LAN devices. Progress events available for UI (P2 feature). Foundation established for future file sync milestone.

**Addresses:** "Image LAN sync" (P1), "Chunked transfer infrastructure" (P1 prerequisite), "Transfer progress indicator" (P2)

**Avoids:** Single large WebSocket message pitfall (Base64 tripling, buffer limits, channel blocking), chunk reassembly without checksum (verify with Blake3 after reassembly), large image OOM (stream in 64KB chunks, enforce size limit), missing encryption for image data (same XChaCha20 path as text)

**Key tasks:**

- Define `ChunkedTransportPort` in uc-core
- Implement `ChunkEnvelope` protocol (transfer_id, sequence, total_chunks, mime_type, encrypted data)
- Implement `ChunkedSender` and `ChunkedReceiver` with in-memory reassembly buffer + timeout cleanup
- Extend `SyncOutbound` to route image representations through `ChunkedSender`
- Wire into `AppDeps` and `UseCases` accessor

### Phase Ordering Rationale

- **Image before hotkey/window:** Phase 1 has no new dependencies and establishes the data model that all other phases depend on. Starting with hotkey or window first would require placeholder data.
- **Hotkey before window:** The window needs a trigger. Validating hotkey registration in isolation (Phase 2) catches capability config issues before UI complexity is added.
- **Window before chunked transfer:** The quick-paste window is the highest-risk phase (Tauri bugs, platform divergence, permission prompts). Completing it while chunked transfer is still simple keeps debugging surface small. Chunked transfer adds no user-visible value until image capture (Phase 1) is working.
- **Chunked transfer last:** It is infrastructure that enables sync, not local capture. Phase 1 delivers local image history value immediately; Phase 4 extends it cross-device. This sequencing provides earlier user-visible value.

### Research Flags

Phases likely needing deeper research during task planning:

- **Phase 3 (Floating Window):** Focus management is actively broken in Tauri 2.x — check latest Tauri issue tracker before implementation. Consider `tauri-plugin-spotlight` API details for macOS focus return. Verify `tauri-nspanel` v2.1 branch API for current Tauri 2.9.x compatibility before task breakdown.
- **Phase 4 (Chunked Transfer):** Backpressure mechanism (`bufferedAmount` in tungstenite) needs validation — confirm the Rust API for checking send buffer before chunk send. Chunk-level retry-on-reconnect may need a protocol version flag if implemented beyond v1 no-retry behavior.

Phases with standard patterns (skip research-phase):

- **Phase 1 (Image Capture):** All infrastructure exists in codebase. Task is wiring + testing, not research. Direct code reading is sufficient.
- **Phase 2 (Hotkey Infrastructure):** Official `tauri-plugin-global-shortcut` docs are comprehensive. Pattern mirrors existing `ClipboardChangeHandler`. No ambiguity.

## Confidence Assessment

| Area         | Confidence  | Notes                                                                                                                                                          |
| ------------ | ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Stack        | HIGH        | All libraries verified on crates.io / official docs; versions confirmed; alternatives evaluated with clear rationale                                           |
| Features     | MEDIUM-HIGH | Competitor analysis from official sources (Maccy GitHub, Ditto docs, CopyQ docs); some UX claims from community sources                                        |
| Architecture | HIGH        | Based on direct codebase analysis + Tauri 2 official docs; existing patterns are clear and extend naturally                                                    |
| Pitfalls     | HIGH        | Tauri focus bugs documented in official GitHub issues with issue numbers; macOS TIFF behavior documented in arboard; accessibility requirement from Apple docs |

**Overall confidence:** HIGH

### Gaps to Address

- **Linux Wayland image capture:** arboard's `wayland-data-control` feature is experimental. Current plan: default to X11/XWayland. Validate on real Wayland session before declaring Linux support complete. May need user-facing documentation about Wayland limitations.
- **enigo Wayland support:** enigo 0.5.0 has experimental Wayland support. Auto-paste on Wayland may not work reliably. Same mitigation as above — document and fall back to clipboard-write-only.
- **`tauri-nspanel` API surface on Tauri 2.9.x:** Research confirmed compatibility with Tauri 2.x via v2.1 branch, but specific API details for the floating panel config should be verified against the branch README before Phase 3 task breakdown.
- **Chunk resume-on-reconnect:** Research recommends v1 implementation without resume (timeout and retry the full transfer). If product requirements demand resume, a protocol version bump and more complex receiver state machine will be needed — defer this decision to Phase 4 task planning.

## Sources

### Primary (HIGH confidence)

- `/tauri-apps/tauri-plugin-global-shortcut` via Context7 — Rust-side registration API, Builder pattern, ShortcutState
- `/websites/rs_arboard` via Context7 — ImageData struct, `get_image()` / `set_image()`, image-data feature
- `/snapview/tokio-tungstenite` via Context7 — WebSocketConfig, max_message_size, binary message handling
- https://v2.tauri.app/plugin/global-shortcut/ — Official plugin docs, version 2.3.0 confirmed
- https://github.com/tauri-apps/tauri/issues/14102 — `focusable: false` broken on macOS (confirmed Tauri 2.8.4/macOS 15)
- https://github.com/tauri-apps/tauri/issues/7519 — `focus` property ignored on Windows
- https://github.com/tauri-apps/tauri/issues/10746 — Window focus broken on Linux v2
- https://crates.io/crates/enigo — enigo 0.5.0 confirmed latest (2025)
- https://crates.io/crates/tauri-plugin-global-shortcut — Version 2.3.0 confirmed
- https://github.com/1Password/arboard/releases — arboard 3.6.1 latest release confirmed
- Direct codebase analysis — `CommonClipboardImpl`, `ThumbnailGenerator`, `BlobStorePort`, `SyncOutboundClipboardUseCase` all confirmed in place

### Secondary (MEDIUM confidence)

- https://github.com/ahkohd/tauri-nspanel — tauri-nspanel v2.1 branch, NSPanel usage, known `is_maximized()` crash limitation
- https://deepwiki.com/EcoPasteHub/EcoPaste — Real-world reference: EcoPaste uses tauri-nspanel for floating clipboard window
- Maccy GitHub, Ditto official site, CopyQ docs — competitor feature analysis
- https://crates.io/crates/tauri-plugin-spotlight — Focus-return pattern for macOS

### Tertiary (MEDIUM-LOW confidence)

- Community sources for UX patterns (hotkey trigger, cursor position, auto-paste behavior) — corroborated across multiple independent sources but not from official documentation

---

_Research completed: 2026-03-02_
_Ready for roadmap: yes_
