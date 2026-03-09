# Architecture Research

**Domain:** Cross-platform clipboard synchronization — Hexagonal Architecture with Tauri 2
**Researched:** 2026-03-02
**Confidence:** HIGH (based on direct codebase analysis + Tauri 2 official docs)

## Standard Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Frontend (React + TypeScript)                         │
│                                                                               │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────────────┐   │
│  │  Main Window     │  │  FloatingPaste   │  │  Redux + RTK Query       │   │
│  │  (label: main)   │  │  Window          │  │  (appApi, clipboard      │   │
│  │  DashboardPage   │  │  (label: quick-  │  │   slices, device slices) │   │
│  │  HistoryList     │  │  paste)          │  └──────────────────────────┘   │
│  └──────────────────┘  └──────────────────┘                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                   Tauri IPC (Commands + Events)                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                      uc-tauri (Adapter Layer)                                 │
│                                                                               │
│  ┌────────────────┐  ┌────────────────┐  ┌───────────────────────────────┐  │
│  │  Commands      │  │  Bootstrap     │  │  AppRuntime / UseCases        │  │
│  │  clipboard.rs  │  │  wiring.rs     │  │  accessor                     │  │
│  │  settings.rs   │  │  runtime.rs    │  │  (wires ports into usecases)  │  │
│  │  hotkey.rs     │  │                │  └───────────────────────────────┘  │
│  │  window.rs     │  └────────────────┘                                      │
│  └────────────────┘                                                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                      uc-app (Application Layer)                               │
│                                                                               │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  Use Cases: CaptureClipboard, ListClipboardEntries, RestoreClipboard   │  │
│  │  SyncOutbound, SyncInbound, DeleteEntry, QuickPasteEntries (new)       │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────────────────┤
│                      uc-core (Domain Layer)                                   │
│                                                                               │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  Ports: SystemClipboardPort, ClipboardTransportPort, BlobStorePort     │  │
│  │  ClipboardChangeHandler, HotkeyRegistrarPort (new), WindowPort (new)   │  │
│  │  ChunkedTransferPort (new)                                              │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────────────────┤
│              uc-infra                    uc-platform                          │
│                                                                               │
│  ┌─────────────────────────┐  ┌────────────────────────────────────────────┐ │
│  │  Diesel repos (SQLite)  │  │  PlatformRuntime (event loop)              │ │
│  │  XChaCha20 encryption   │  │  ClipboardWatcher (clipboard_rs)           │ │
│  │  BlobStore (filesystem) │  │  GlobalHotkeyAdapter (tauri-plugin-global- │ │
│  │  ThumbnailGenerator     │  │    shortcut)                               │ │
│  │  ChunkedTransferAdapter │  │  WindowManagerAdapter (WebviewWindowBuilder│ │
│  │    (new, in network/)   │  │    API)                                    │ │
│  └─────────────────────────┘  └────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component               | Responsibility                                                                                                                                | Typical Implementation                                                                                          |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| FloatingPaste Window    | Show recent clipboard entries; receive hotkey-triggered show/hide; call write-back to system clipboard                                        | Separate Tauri WebviewWindow (label: "quick-paste"), React SPA route at `/quick-paste`, auto-hide on focus loss |
| HotkeyRegistrarPort     | Register/unregister OS-level global hotkeys, emit hotkey events to app layer                                                                  | `tauri-plugin-global-shortcut` adapter in uc-platform                                                           |
| WindowManagerPort       | Programmatically show/hide/position windows by label                                                                                          | Tauri AppHandle wrapper in uc-platform/adapters                                                                 |
| ImageCaptureAdapter     | Read image from `clipboard_rs` `ContentFormat::Image`, convert to PNG bytes, emit as `ObservedClipboardRepresentation` with `mime: image/png` | Already in `CommonClipboardImpl::read_snapshot()` — gating/persistence is the missing piece                     |
| ChunkedTransferAdapter  | Split large payloads (>64KB) into numbered chunks, reassemble on receiver side, track transfer progress                                       | New adapter in uc-infra/network, implements `ChunkedTransportPort`                                              |
| SyncOutbound (extended) | Accept image representations in addition to text; select best representation per peer capability; call chunked transport for large payloads   | Extension of existing `SyncOutboundClipboardUseCase`                                                            |
| ThumbnailGenerator      | Generate low-res WebP preview from PNG image bytes; store in BlobStore                                                                        | Already in uc-infra/clipboard/thumbnail_generator.rs — needs wiring for image entries                           |

## Recommended Project Structure

```
src-tauri/crates/
├── uc-core/src/ports/
│   ├── hotkey.rs                 # NEW: HotkeyRegistrarPort trait
│   ├── window_manager.rs         # NEW: WindowManagerPort trait
│   └── chunked_transport.rs      # NEW: ChunkedTransportPort trait
│
├── uc-platform/src/
│   ├── hotkey/
│   │   └── global_shortcut_adapter.rs  # NEW: wraps tauri-plugin-global-shortcut
│   ├── window/
│   │   └── window_manager_adapter.rs   # NEW: wraps AppHandle WebviewWindowBuilder
│   └── runtime/
│       └── runtime.rs            # EXTEND: handle HotkeyTriggered command
│
├── uc-infra/src/
│   └── network/
│       └── chunked/
│           ├── mod.rs
│           ├── sender.rs         # NEW: ChunkedSender — splits payload, sends chunks
│           ├── receiver.rs       # NEW: ChunkedReceiver — reassembles chunks
│           └── protocol.rs       # NEW: ChunkEnvelope struct (chunk_id, seq, total, data)
│
├── uc-app/src/usecases/
│   └── clipboard/
│       └── get_quick_paste_entries.rs  # NEW: list entries optimized for quick-paste UI
│
└── uc-tauri/src/
    ├── commands/
    │   ├── hotkey.rs             # NEW: register_hotkey, unregister_hotkey commands
    │   └── window.rs             # NEW: show_quick_paste_window, hide_quick_paste_window
    └── bootstrap/
        └── wiring.rs             # EXTEND: wire HotkeyRegistrarPort, WindowManagerPort

src/
├── pages/
│   └── QuickPastePage.tsx        # NEW: floating clipboard history UI
├── components/
│   └── quick-paste/
│       ├── EntryList.tsx         # NEW: compact clipboard entry list
│       └── EntryRow.tsx          # NEW: single entry row with paste action
└── hooks/
    └── useQuickPasteWindow.ts    # NEW: handles show/hide, keyboard nav
```

### Structure Rationale

- **uc-core/ports/hotkey.rs and window_manager.rs:** New ports follow the existing pattern — trait definitions only, no implementations. uc-platform supplies implementations.
- **uc-platform/hotkey/:** Platform adapter keeps the `tauri-plugin-global-shortcut` dependency isolated from app/infra layers. The adapter translates plugin callbacks into `PlatformCommand::HotkeyTriggered { shortcut_id }` for the PlatformRuntime event loop.
- **uc-infra/network/chunked/:** Infrastructure layer owns chunking because it is a transport concern, not business logic. The `SyncOutboundUseCase` calls `ChunkedTransportPort` the same way it calls `ClipboardTransportPort` — it does not know about chunk internals.
- **QuickPastePage as a separate route:** The floating window loads the same frontend SPA but routes to `/quick-paste`. This reuses all existing Redux state, RTK Query caches, and component infrastructure without duplicating logic.

## Architectural Patterns

### Pattern 1: Global Hotkey as Platform Event

**What:** OS-level hotkey registration lives in uc-platform (adapter for `tauri-plugin-global-shortcut`). When triggered, the adapter sends a `PlatformCommand::HotkeyTriggered { shortcut_id: String }` through the existing platform command channel. The PlatformRuntime event loop handles it and calls the appropriate application callback — e.g., a `HotkeyHandler` trait (similar to `ClipboardChangeHandler`).

**When to use:** Any OS-level input event that needs to reach application logic without coupling the platform layer to business logic.

**Trade-offs:** One extra hop through the channel vs. calling app directly. The indirection is the correct architectural choice — it mirrors the existing `ClipboardChangeHandler` pattern and enforces the dependency inversion principle.

**Example:**

```rust
// uc-core/src/ports/hotkey.rs (new)
#[async_trait]
pub trait HotkeyHandlerPort: Send + Sync {
    async fn on_hotkey_triggered(&self, shortcut_id: &str) -> Result<()>;
}

// uc-platform/src/hotkey/global_shortcut_adapter.rs (new)
// In setup, after plugin init:
app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+V", move |_app, _shortcut, _state| {
    let _ = platform_cmd_tx.try_send(PlatformCommand::HotkeyTriggered {
        shortcut_id: "quick-paste".to_string(),
    });
})?;

// uc-platform/src/runtime/runtime.rs (extended)
PlatformCommand::HotkeyTriggered { shortcut_id } => {
    if let Some(handler) = &self.hotkey_handler {
        let _ = handler.on_hotkey_triggered(&shortcut_id).await;
    }
}
```

### Pattern 2: Floating Window as Separate Tauri Window

**What:** The quick-paste window is created programmatically using `WebviewWindowBuilder`. It loads the same frontend SPA but at route `/quick-paste`. Window is created once at startup (hidden), shown/repositioned on hotkey, hidden on focus loss or Escape.

**When to use:** UI that needs to float above other applications, appear on demand near cursor position, and not share the main window's navigation state.

**Trade-offs:** Adds a second WebviewWindow (slightly more memory); requires a second webview initialization. Alternative of using a popup overlay inside the main window was rejected because: the main window may be hidden to tray, and the floating window must appear without showing the main window.

**Example:**

```rust
// uc-tauri/src/commands/window.rs (new)
#[tauri::command]
pub async fn show_quick_paste_window(
    app: AppHandle,
    x: f64,
    y: f64,
) -> Result<(), String> {
    match app.get_webview_window("quick-paste") {
        Some(window) => {
            window.set_position(PhysicalPosition::new(x as i32, y as i32))
                .map_err(|e| e.to_string())?;
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }
        None => {
            WebviewWindowBuilder::new(
                &app,
                "quick-paste",
                WebviewUrl::App("index.html#/quick-paste".into()),
            )
            .position(x, y)
            .inner_size(360.0, 480.0)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .build()
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
```

### Pattern 3: Image Capture via Existing ClipboardWatcher Path

**What:** Image capture is already partially implemented — `CommonClipboardImpl::read_snapshot()` already reads `ContentFormat::Image` and converts to PNG bytes with `mime: image/png`. The missing pieces are: (1) the `CaptureClipboard` use case must persist image representations to blob storage (not inline) and trigger thumbnail generation; (2) the `SyncOutbound` use case must pass image representations to the chunked transport.

**When to use:** Images arrive as `ObservedClipboardRepresentation` with `format_id: "image"` and `mime: Some(MimeType("image/png"))`. The existing spool queue and blob worker pipeline already handles routing large payloads to blob storage.

**Trade-offs:** No new capture path needed. The existing platform capture infrastructure handles images transparently. The only changes are in the use case policy (allow image representations through the sync path) and transport (chunked sending).

**Example:**

```rust
// uc-app/src/usecases/clipboard/sync_outbound.rs (extended)
// Change the representation selection logic:
let selected_representations: Vec<_> = snapshot.representations.iter()
    .filter(|rep| {
        rep.mime.as_ref().is_some_and(|mime| {
            is_text_plain_mime(mime.as_str()) || is_image_mime(mime.as_str())
        })
    })
    .collect();

// For image representations, use chunked transport instead of direct send:
for rep in &selected_representations {
    if rep.mime.as_ref().is_some_and(|m| is_image_mime(m.as_str())) {
        self.chunked_transport.send_chunked(&peer.peer_id, &rep.bytes).await?;
    } else {
        self.clipboard_network.send_clipboard(&peer.peer_id, payload.clone()).await?;
    }
}
```

### Pattern 4: Chunked Transfer as Infrastructure Wrapper

**What:** `ChunkedTransportPort` wraps `ClipboardTransportPort` at the infrastructure layer. The chunking adapter splits payloads larger than a threshold (recommended 64KB) into numbered `ChunkEnvelope` frames, sends them over the existing WebSocket path, and reassembles on the receiver side. The reassembled payload is passed back through the normal inbound clipboard pipeline.

**When to use:** Any payload exceeding the inline threshold (images, future file attachments). The threshold is configurable in settings.

**Trade-offs:** Adds reassembly state on the receiver (in-memory buffer keyed by transfer ID + sender). Partial transfers must be cleaned up if the connection drops. For the initial implementation, receiver-side timeout cleanup is sufficient (no streaming resume for v1).

**Example:**

```rust
// uc-infra/src/network/chunked/protocol.rs (new)
#[derive(Serialize, Deserialize)]
pub struct ChunkEnvelope {
    pub transfer_id: String,    // UUID for this transfer
    pub sequence: u32,          // 0-indexed chunk position
    pub total_chunks: u32,      // total expected chunks
    pub mime_type: String,      // original MIME type for reassembly
    pub data: Vec<u8>,          // encrypted chunk bytes
}

// uc-infra/src/network/chunked/sender.rs (new)
pub struct ChunkedSender {
    transport: Arc<dyn ClipboardTransportPort>,
    chunk_size: usize,  // default 64 * 1024
}

impl ChunkedSender {
    pub async fn send(&self, peer_id: &str, mime: &str, payload: &[u8]) -> Result<()> {
        let transfer_id = Uuid::new_v4().to_string();
        let chunks: Vec<&[u8]> = payload.chunks(self.chunk_size).collect();
        let total = chunks.len() as u32;
        for (seq, chunk) in chunks.iter().enumerate() {
            let envelope = ChunkEnvelope {
                transfer_id: transfer_id.clone(),
                sequence: seq as u32,
                total_chunks: total,
                mime_type: mime.to_string(),
                data: chunk.to_vec(),
            };
            let bytes = serde_json::to_vec(&envelope)?;
            self.transport.send_clipboard(peer_id, bytes).await?;
        }
        Ok(())
    }
}
```

## Data Flow

### Quick-Paste Window Trigger Flow

```
[User presses Cmd+Shift+V]
    |
    v
[tauri-plugin-global-shortcut callback]
    |  try_send(PlatformCommand::HotkeyTriggered { "quick-paste" })
    v
[PlatformRuntime::handle_command()]
    |  hotkey_handler.on_hotkey_triggered("quick-paste")
    v
[AppRuntime (implements HotkeyHandlerPort)]
    |  get cursor position from OS
    |  show_quick_paste_window command
    v
[Tauri command: show_quick_paste_window(x, y)]
    |  app.get_webview_window("quick-paste").show() + set_position()
    v
[QuickPaste React window appears at cursor position]
[QuickPaste React window appears at cursor position]
    |  RTK Query: get_clipboard_entries (separate renderer cache; same backend DB)
    v
[User selects entry → paste_clipboard_entry command]
    |  AppRuntime → RestoreClipboardSelection use case
    |  SystemClipboardPort.write_snapshot(selected)
    v
[Window hides; focus returns to previous app]
    |
    v
[User pastes with Cmd+V in previous app]
```

### Image Capture Flow

```
[User copies image in any app]
    |
    v
[ClipboardWatcher detects ContentFormat::Image change]
    |
    v
[CommonClipboardImpl::read_snapshot()]
    |  ctx.get_image() → img.to_png() → bytes
    |  ObservedClipboardRepresentation { format_id: "image", mime: "image/png", bytes }
    v
[PlatformRuntime → ClipboardChangeHandler::on_clipboard_changed(snapshot)]
    |
    v
[AppRuntime → CaptureClipboard use case]
    |  representation size > inline threshold (e.g. 1MB)?
    |  → YES: create staged PersistedRepresentation, enqueue to SpoolQueue
    |  → NO: store inline in DB
    v
[BackgroundBlobWorker dequeues, writes PNG to BlobStore]
    |  BlobId stored in PersistedRepresentation
    v
[ThumbnailGenerator creates WebP thumbnail]
    |  thumbnail stored in BlobStore, ThumbnailMetadata saved to DB
    v
[Frontend: get_clipboard_entries returns entry with thumbnail_url]
    |  <img src="uc://thumbnail/{representation_id}" />
    v
[URI protocol handler: resolve_uc_thumbnail_request → ResolveThumbnailResource use case]
    |  reads thumbnail blob from BlobStore
    v
[Image preview displayed in history list]
```

### Image Sync Flow (Outbound)

```
[CaptureClipboard completes; blob is ready]
    |
    v
[SyncOutbound use case receives snapshot with image representation]
    |  representation.mime == "image/png"?
    |  → YES: load blob bytes from BlobStore
    v
[Encrypt full image bytes with XChaCha20-Poly1305]
    |
    v
[ChunkedSender.send(peer_id, "image/png", encrypted_bytes)]
    |  split into N × 64KB ChunkEnvelope frames
    |  send each via ClipboardTransportPort.send_clipboard()
    v
[Peer receives N frames via WebSocket]
    |
    v
[ChunkedReceiver.accumulate(ChunkEnvelope)]
    |  buffer keyed by transfer_id
    |  when all chunks received: reassemble
    v
[SyncInbound use case: decrypt reassembled bytes]
    |  write to SystemClipboard as image/png representation
    v
[CaptureClipboard triggered on receiver side with origin: RemotePush]
    |  stores image in local BlobStore + thumbnail
    v
[Receiver frontend displays image in history list]
```

### State Management

```
Backend (no global mutable state):
  AppRuntime (Arc<AppRuntime>) → manages UseCases accessor
      |
      ├─ AppDeps (ports wired at startup)
      |   ├─ BlobStorePort        (filesystem)
      |   ├─ ThumbnailRepository  (Diesel)
      |   ├─ ClipboardTransportPort (WebSocket/libp2p)
      |   └─ ChunkedTransportPort (wraps above, new)
      |
      └─ HotkeyRegistrarPort → PlatformRuntime channel

Frontend (Redux Toolkit):
  appApi (RTK Query)
      ├─ getClipboardEntries     (polling, used by main window + quick-paste)
      ├─ getClipboardEntryDetail (on-demand)
      └─ pasteClipboardEntry     (mutation — triggers restore + hides window)
```

## Integration Points

### New Ports (uc-core)

| Port                   | Location                             | Consumer                       | Provider                               |
| ---------------------- | ------------------------------------ | ------------------------------ | -------------------------------------- |
| `HotkeyRegistrarPort`  | `uc-core/ports/hotkey.rs`            | uc-app (register on startup)   | uc-platform (global-shortcut plugin)   |
| `HotkeyHandlerPort`    | `uc-core/ports/hotkey.rs`            | uc-platform (calls back)       | uc-app (AppRuntime implements)         |
| `WindowManagerPort`    | `uc-core/ports/window_manager.rs`    | uc-app (show/hide window)      | uc-platform/uc-tauri (Tauri AppHandle) |
| `ChunkedTransportPort` | `uc-core/ports/chunked_transport.rs` | uc-app (sync outbound/inbound) | uc-infra (ChunkedSender/Receiver)      |

### Internal Boundaries

| Boundary                         | Communication                                                       | Notes                                                                                                          |
| -------------------------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| uc-platform → uc-app (hotkey)    | `HotkeyHandlerPort::on_hotkey_triggered()` callback                 | Mirrors `ClipboardChangeHandler` pattern                                                                       |
| uc-app → uc-platform (window)    | `WindowManagerPort::show_window()`                                  | Called from HotkeyHandler on AppRuntime                                                                        |
| uc-app → uc-infra (chunked sync) | `ChunkedTransportPort::send_chunked()`                              | Replaces direct `ClipboardTransportPort` for large payloads                                                    |
| Quick-paste window → uc-tauri    | Tauri commands (`get_clipboard_entries`, `restore_clipboard_entry`) | Reuses existing command set — no new commands needed for the list/paste flow                                   |
| Frontend windows (shared state)  | Redux RTK Query cache is per-window (separate renderers)            | Quick-paste window must call `get_clipboard_entries` independently; cannot share the main window's Redux store |

### External Dependencies (New)

| Dependency                           | Where                  | Purpose                                                                                           |
| ------------------------------------ | ---------------------- | ------------------------------------------------------------------------------------------------- |
| `tauri-plugin-global-shortcut = "2"` | `src-tauri/Cargo.toml` | OS global hotkey registration (macOS, Windows, Linux)                                             |
| `@tauri-apps/plugin-global-shortcut` | `package.json`         | Frontend JS binding (only needed if registering from frontend, not recommended for this use case) |

## Suggested Build Order

The dependency graph dictates this order. Each phase can only begin when the listed prerequisites are in place.

### Phase 1: Image Capture and Display (No New Dependencies)

**Build first because:** Image capture infrastructure (`CommonClipboardImpl`) is already in place. The pipeline from capture → blob → thumbnail → `uc://thumbnail/` already exists in framework. This phase only wires it end-to-end and validates the data model.

**Components:**

1. Extend `CaptureClipboard` use case: allow image representations to flow through persistence (currently the spool/blob pipeline exists but image MIME types may be filtered or not fully wired)
2. Verify `ThumbnailGenerator` creates WebP previews for `image/png` representations
3. Update `ListClipboardEntries` / `ListEntryProjections` to include `thumbnail_url` in response DTO
4. Update frontend `ClipboardEntryRow` to render `<img>` for image entries using `uc://thumbnail/<id>`

**Prerequisites:** Nothing new. All infrastructure exists.

### Phase 2: Global Hotkey Infrastructure

**Build second because:** The quick-paste window needs a trigger mechanism. The `tauri-plugin-global-shortcut` adds a new dependency that must be verified working before building UI on top of it.

**Components:**

1. Add `tauri-plugin-global-shortcut` to Cargo.toml and main.rs
2. Implement `HotkeyRegistrarPort` and `HotkeyHandlerPort` traits in uc-core
3. Implement `GlobalShortcutAdapter` in uc-platform (translates plugin callback to `PlatformCommand::HotkeyTriggered`)
4. Extend `PlatformRuntime::handle_command()` for `HotkeyTriggered`
5. Implement `AppRuntime` as `HotkeyHandlerPort` (receives hotkey event, decides what to do)
6. Register default hotkey `CmdOrCtrl+Shift+V` on startup

**Prerequisites:** Phase 1 (to have something useful to show in the quick-paste window).

### Phase 3: Quick-Paste Floating Window

**Build third because:** Requires Phase 2 (hotkey trigger) and Phase 1 (image thumbnails to display).

**Components:**

1. Implement `WindowManagerPort` trait in uc-core
2. Implement `TauriWindowManagerAdapter` in uc-platform (wraps AppHandle)
3. Create `QuickPastePage.tsx` and route at `/quick-paste`
4. Register Tauri commands: `show_quick_paste_window(x, y)`, `hide_quick_paste_window()`
5. Connect hotkey handler: on `HotkeyTriggered("quick-paste")`, call `show_quick_paste_window` with cursor position
6. Add `on_window_event` for focus loss → auto-hide the quick-paste window
7. Connect entry selection to `restore_clipboard_entry` command + window hide

**Prerequisites:** Phase 2 (hotkey), Phase 1 (image display in list).

### Phase 4: Chunked Transfer for Image Sync

**Build last because:** Depends on image capture (Phase 1) being stable. Chunked transfer is infrastructure-only — it does not affect the UI or user-visible feature surface until image sync is connected.

**Components:**

1. Define `ChunkedTransportPort` trait in uc-core
2. Implement `ChunkEnvelope` protocol struct in uc-infra
3. Implement `ChunkedSender` in uc-infra
4. Implement `ChunkedReceiver` with in-memory reassembly buffer and timeout cleanup
5. Extend `SyncOutbound` use case to route image representations through `ChunkedSender`
6. Extend `SyncInbound` use case to handle `ChunkEnvelope` frames from inbound stream
7. Wire `ChunkedTransportPort` into `AppDeps` and through `UseCases` accessor

**Prerequisites:** Phase 1 (image data model), stable WebSocket transport.

## Anti-Patterns

### Anti-Pattern 1: Quick-Paste Window as Popup in Main Window

**What people do:** Show a floating panel inside the main window's React tree as an absolutely-positioned overlay.

**Why it's wrong:** The main window may be hidden to tray. The quick-paste popup must appear even when the main window is not visible. A popup inside the main window also cannot float above other applications.

**Do this instead:** Create a separate `WebviewWindowBuilder` window with `always_on_top(true)`, `decorations(false)`, `skip_taskbar(true)`. Load the same SPA at route `/quick-paste`. Keep it alive between invocations by hiding/showing (avoid re-creating to meet the <200ms latency requirement — the first creation is slow, subsequent shows are fast).

### Anti-Pattern 2: Calling tauri-plugin-global-shortcut from Frontend JavaScript

**What people do:** Import `@tauri-apps/plugin-global-shortcut` in React and register shortcuts from the frontend.

**Why it's wrong:** Registration from the frontend only works when the webview has focus. The whole point of global shortcuts is that they fire even when the app is backgrounded. A backgrounded window may not have a running JS context.

**Do this instead:** Register shortcuts in Rust during app setup using `app.global_shortcut().register()`. The Rust callback sends a `PlatformCommand::HotkeyTriggered` through the existing channel. This fires regardless of window focus state.

### Anti-Pattern 3: Sending Full Image Bytes as a Single WebSocket Message

**What people do:** Serialize the entire encrypted image payload as one JSON blob and send via `ClipboardTransportPort::send_clipboard()`.

**Why it's wrong:** WebSocket frames have practical size limits (often 16MB per frame, but the existing JSON-over-WebSocket protocol will serialize to base64 inside JSON, tripling the size). A 5MB PNG becomes ~15MB of JSON. This causes timeout failures, memory pressure on both sides, and blocks the WebSocket channel while the large send is in progress.

**Do this instead:** Use `ChunkedSender` to split payloads larger than 64KB into numbered frames. The chunked framing also enables progress reporting and partial failure recovery (resend missing chunks in v2).

### Anti-Pattern 4: Sharing Redux Store Between Main and Quick-Paste Windows

**What people do:** Assume the two Tauri WebviewWindows share the same JavaScript runtime and Redux store.

**Why it's wrong:** Each Tauri WebviewWindow has an independent renderer process (or at minimum an independent JavaScript context). Redux state is not shared.

**Do this instead:** Both windows make their own RTK Query calls. The main window's clipboard history list and the quick-paste window both call `get_clipboard_entries` — they share the backend cache (the same SQLite database) but have independent frontend state. Use the `window.__TAURI__` inter-window communication APIs if you need to signal state changes between windows (e.g., notify quick-paste when an entry was added by the main window), but prefer polling via RTK Query for simplicity.

## Sources

- Tauri 2 official docs: `WebviewWindowBuilder`, `GlobalShortcutExt`, `on_window_event` — HIGH confidence (Context7 + official docs.rs)
- `tauri-plugin-global-shortcut` Rust API: `GlobalShortcutExt::register()`, `on_shortcut()` callback pattern — HIGH confidence (official docs)
- Existing codebase: `CommonClipboardImpl` already reads images (`ContentFormat::Image`), `ThumbnailMetadata` model exists, `BlobStorePort` exists — HIGH confidence (direct codebase analysis)
- `SyncOutboundClipboardUseCase` text-only filter at line 86-98 is the exact extension point for image support — HIGH confidence (direct code read)
- Chunked transfer pattern: standard WebSocket framing practice, no specific Tauri documentation — MEDIUM confidence (established network programming pattern)
- `on_window_event(Focused(false))` for auto-hiding floating windows — HIGH confidence (official Tauri docs example)

---

_Architecture research for: uniclipboard-desktop (quick-paste window, image clipboard, chunked transfer, global hotkey)_
_Researched: 2026-03-02_
