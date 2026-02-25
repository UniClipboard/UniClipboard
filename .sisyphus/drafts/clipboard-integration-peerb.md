# Draft: PeerB Clipboard Integration Mode

## Requirements (confirmed)

- Dev environment runs two instances (peerA, peerB) for local P2P sync testing.
- peerA: normal behavior (listen to local system clipboard changes; capture; outbound sync as usual; and on inbound remote sync, may write back to system clipboard per normal flow).
- peerB: MUST NOT listen to local system clipboard changes (no local capture/outbound triggered by OS clipboard).
- peerB: MUST still receive remote sync clipboard data and persist/show it (e.g., entries in app).
- peerB: MUST NOT actually write to the OS/system clipboard even when receiving remote clipboard sync.
- Implementation should NOT be a hardcoded if/else keyed on "peerB"; prefer an abstract, configurable approach.

## Technical Decisions (tentative)

- Prefer capability/config-driven adapter selection (ports/adapters) over scattered conditional logic.
- Introduce a "clipboard integration mode" (e.g., full vs passive) configurable per instance.
- Mode selection (confirmed): use env var `UC_CLIPBOARD_MODE=full|passive` (default: `full`).
- Passive mode behavior (confirmed): user-triggered `restore_clipboard_entry` / `sync_clipboard_items` returns an explicit error (not silent no-op; not virtual clipboard).

## Codebase Findings (current behavior)

### Local OS clipboard change → capture pipeline

- OS watcher lives in `src-tauri/crates/uc-platform/src/runtime/runtime.rs`:
  - `PlatformCommand::StartClipboardWatcher` starts `clipboard_rs` watcher.
  - On `PlatformEvent::ClipboardChanged { snapshot }`, calls `clipboard_handler.on_clipboard_changed(snapshot)`.
- Handler is `AppRuntime` in `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`:
  - Implements `uc_core::ports::ClipboardChangeHandler`.
  - Builds and executes `CaptureClipboardUseCase::execute_with_origin(snapshot, origin)`.
  - Emits `clipboard://event` to frontend.
  - Spawns outbound sync via `SyncOutboundClipboardUseCase`.

### Watcher start timing

- `src-tauri/crates/uc-app/src/usecases/app_lifecycle/mod.rs` `AppLifecycleCoordinator::ensure_ready()` always executes `StartClipboardWatcher`.
- `StartClipboardWatcher` calls `WatcherControlPort::start_watcher()`.
- Default wiring uses `InMemoryWatcherControl` (`src-tauri/crates/uc-platform/src/adapters/in_memory_watcher_control.rs`) which sends `PlatformCommand::StartClipboardWatcher`.

### Remote inbound sync today (problematic for peerB)

- Inbound receive loop in `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` calls `SyncInboundClipboardUseCase::execute(message)`.
- `SyncInboundClipboardUseCase` (`src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs`):
  - Decrypts `ClipboardMessage`.
  - Sets `ClipboardChangeOrigin::RemotePush` (TTL 100ms) via `ClipboardChangeOriginPort`.
  - Writes snapshot to `SystemClipboardPort::write_snapshot(snapshot)` (OS clipboard write).
  - Relies on running watcher + callback handler to persist the entry into DB.

### Outbound echo avoidance

- `SyncOutboundClipboardUseCase` (`src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs`) skips outbound when origin == `RemotePush`.

## Key Problem Statement (reframed)

- In current design, inbound persistence is coupled to "write to system clipboard" + "watcher running".
- For peerB, we want to disable both OS write and OS watch, but still persist/show inbound remote content.

## Risks / Pitfalls Noted

- Inbound dedupe in `sync_inbound.rs` reads the system clipboard to decide "already applied". In passive mode, OS clipboard is shared with peerA, so this dedupe can incorrectly skip persisting inbound messages.
- If we introduce "ingest inbound directly" in full mode while watcher is also running, we risk double-ingesting the same remote push.

## Proposal (recommended)

### Introduce Clipboard Integration Policy (capability-driven)

Add a small, pure-data policy object (no peer-name branching) that the runtime + use cases can consult:

- `ClipboardIntegrationMode` (enum): `full | passive` (optionally extend later)
- Derived booleans:
  - `observe_local_system_clipboard` (start OS watcher)
  - `write_system_clipboard` (allow any OS clipboard writes)
  - `apply_remote_to_system_clipboard` (remote inbound should write to OS clipboard)

How to select per instance (dev-friendly, non-hardcoded):

- Add env var (recommended): `UC_CLIPBOARD_MODE=full|passive`.
  - peerA script sets `full`; peerB sets `passive`.
  - Defaults to `full` when unset.
- Alternative: put the mode into `config.toml` (pure facts) and let each instance use a different config via `UC_CONFIG_PATH`.
- Alternative: put the mode into `settings.json` (per-UC_PROFILE app dir); then peerA/peerB can persist different modes.

### Behavior by mode

`full` (today's behavior, preserved)

- Watcher starts (OS clipboard changes → capture pipeline).
- Inbound remote sync: decrypt → set origin RemotePush → write snapshot to OS clipboard → watcher captures → persist entry + emit `clipboard://event`.
- Outbound echo avoidance stays where it is: `SyncOutboundClipboardUseCase` already skips when origin == `RemotePush`.

`passive` (desired for peerB)

- Watcher never starts (no local OS clipboard capture, no outbound triggered by OS changes).
- Inbound remote sync: decrypt → construct snapshot → directly ingest/persist without OS clipboard writes.
  - Use `CaptureClipboardUseCase::execute_with_origin(snapshot, RemotePush)` as the canonical ingest.
  - Emit `clipboard://event` to refresh UI (since we are bypassing the OS watcher callback).
- System clipboard writes are blocked/no-op (inbound apply and also user-triggered restore).

### Concrete wiring / injection points

1. Disable local OS watch in passive mode

- Best abstraction: provide a `WatcherControlPort` adapter that is policy-aware.
  - In `full`: delegate to `InMemoryWatcherControl` (sends StartClipboardWatcher to PlatformRuntime).
  - In `passive`: return `Ok(())` without sending the command (Noop watcher control).
  - This keeps `AppLifecycleCoordinator` behavior intact (still "starts" watcher, but effectively no-ops).

2. Ensure passive inbound persists without OS clipboard

- Update inbound apply logic so it can take the "direct ingest" path when OS writes are disabled.
  - Preferred: extend `SyncInboundClipboardUseCase` to accept a reference to a small ingest use case (or the ports needed to construct one) plus the policy.
  - In passive: skip system clipboard read/write dedupe; dedupe should be based on message id/content hash + local persistence, not OS clipboard state.

3. Guard all other OS clipboard write paths

- `restore_clipboard_entry` (command) calls `RestoreClipboardSelectionUseCase::restore_snapshot` which writes to `SystemClipboardPort`.
- In passive mode: either
  - return an explicit error "clipboard writes disabled for this instance"; OR
  - swap `SystemClipboardPort` to an in-memory/no-op implementation for this instance.
    (Decision depends on whether you want peerB UI restore to "work" without touching OS.)

## Alternatives (for review)

### A) Virtual clipboard adapter (more complex)

- Replace `SystemClipboardPort` with an in-memory clipboard in passive mode.
- Add an internal watcher/event hook so `write_snapshot` triggers the same capture pipeline as OS changes.
- Pros: minimal changes to inbound use case (can keep writing to clipboard port).
- Cons: more moving parts; harder to reason about; still need careful dedupe.

### B) Only gate watcher + OS write at platform runtime (insufficient alone)

- Make `PlatformRuntime` ignore StartClipboardWatcher / WriteClipboard when disabled.
- Pros: very localized.
- Cons: inbound still won't persist because persistence is currently coupled to watcher-driven capture.

## Testing notes (suggested)

- Unit tests (Rust):
  - passive: inbound message results in persisted entry without calling `SystemClipboardPort::write_snapshot`.
  - passive: `StartClipboardWatcher` effectively no-ops (no StartClipboardWatcher command sent).
- Dev QA:
  - Run dual instances; copy locally; verify peerB logs do not show "Clipboard changed".
  - Trigger remote inbound; verify peerB shows new entry + frontend receives `clipboard://event`.
  - Verify OS clipboard unchanged on peerB (macOS: `pbpaste` unchanged).

## Observations (from user logs)

- peerB currently receives macOS clipboard change callbacks and executes capture pipeline:
  - platform clipboard read -> snapshot -> uc_app capture usecase -> encrypting writer -> event emitted to frontend.
- This interferes with local P2P sync tests because both instances react to the same OS clipboard.

## Open Questions

- Do you need a third mode later (e.g., "no listen but allow OS write"), or is "passive" sufficient?
- In passive mode, should we fully disable outbound sync from ALL local actions (including UI restore), or is "no OS watch + no OS write" enough?

## Scope Boundaries

- INCLUDE: disable local clipboard listening + disable OS clipboard writing for one instance while keeping remote sync + persistence.
- EXCLUDE: network/libp2p behavior changes, encryption session changes, UI redesign (unless necessary for verification).
