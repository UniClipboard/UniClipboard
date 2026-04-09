# Phase 61: Daemon Outbound Clipboard Sync - Research

**Researched:** 2026-03-25
**Domain:** Rust async / clipboard sync / daemon integration
**Confidence:** HIGH

## Summary

Phase 61 adds outbound clipboard synchronization to the daemon: after the daemon captures a local clipboard change (via `DaemonClipboardChangeHandler`), it must trigger `SyncOutboundClipboardUseCase` to push that content to paired peers. This mirrors the logic already implemented in `AppRuntime::on_clipboard_changed` in `uc-tauri/src/bootstrap/runtime.rs`.

All the required infrastructure already exists in `uc-app` and is accessible through `CoreRuntime::wiring_deps()`. The daemon's `ClipboardWatcherWorker` has a `DaemonClipboardChangeHandler` that is the single entry point. The work is to extend `DaemonClipboardChangeHandler::on_clipboard_changed` to call `OutboundSyncPlanner::plan()` after successful capture, then dispatch `SyncOutboundClipboardUseCase::execute()` and optionally `SyncOutboundFileUseCase::execute()` in background tasks.

**Primary recommendation:** Extend `DaemonClipboardChangeHandler::on_clipboard_changed` in `uc-daemon/src/workers/clipboard_watcher.rs` to call `OutboundSyncPlanner` + `SyncOutboundClipboardUseCase` after capture, mirroring the `AppRuntime::on_clipboard_changed` pattern from `uc-tauri`. Move `extract_file_paths_from_snapshot` to a shared location in `uc-app` or `uc-core` so daemon and Tauri both use it without duplication.

## Standard Stack

### Core

| Library                                                                    | Version   | Purpose                                                | Why Standard                                                  |
| -------------------------------------------------------------------------- | --------- | ------------------------------------------------------ | ------------------------------------------------------------- |
| `uc-app::usecases::sync_planner::OutboundSyncPlanner`                      | workspace | Centralized outbound sync eligibility policy           | Single policy point per v0.3.0 decision                       |
| `uc-app::usecases::clipboard::sync_outbound::SyncOutboundClipboardUseCase` | workspace | Encrypts and fans out clipboard to peers               | Full protocol implementation with V3 payload                  |
| `uc-app::usecases::file_sync::SyncOutboundFileUseCase`                     | workspace | File transfer for file clipboard items                 | Already in `CoreUseCases::sync_outbound_file()`               |
| `uc-infra::clipboard::TransferPayloadEncryptorAdapter`                     | workspace | XChaCha20-Poly1305 encryption for outbound             | uc-infra adapter for encryption port                          |
| `tokio::task::spawn_blocking`                                              | 1.x       | Execute sync `SyncOutboundClipboardUseCase::execute()` | `execute()` is synchronous with internal `executor::block_on` |

### Supporting

| Library                            | Version   | Purpose                                    | When to Use                                            |
| ---------------------------------- | --------- | ------------------------------------------ | ------------------------------------------------------ |
| `uc_observability::FlowId`         | workspace | Correlation ID across capture → sync spans | Per AppRuntime pattern — generate per clipboard change |
| `tracing::{info_span, Instrument}` | 0.1       | Span-based async instrumentation           | Wrap outbound_sync and outbound_file_sync tasks        |

### Alternatives Considered

| Instead of                                     | Could Use                             | Tradeoff                                                                                              |
| ---------------------------------------------- | ------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `spawn_blocking` for SyncOutbound              | Make execute() async                  | `execute()` is designed sync with `executor::block_on`; changing it risks breaking AppRuntime callers |
| Inline outbound sync in `on_clipboard_changed` | Separate `OutboundSyncService` struct | Inline is simpler and follows AppRuntime pattern exactly                                              |

**Installation:** No new dependencies required — all crates already present in the workspace.

## Architecture Patterns

### Recommended Project Structure

No new files needed. Changes are additive in:

```
src-tauri/crates/uc-daemon/src/workers/
└── clipboard_watcher.rs   # Extend DaemonClipboardChangeHandler::on_clipboard_changed

src-tauri/crates/uc-app/src/usecases/
└── (possible) clipboard/file_path_extractor.rs  # Extract shared helper if desired
```

### Pattern 1: AppRuntime Outbound Sync (the reference implementation)

**What:** `AppRuntime::on_clipboard_changed` in `uc-tauri/src/bootstrap/runtime.rs` lines 568–757 is the canonical outbound sync implementation.

**Flow:**

1. `consume_origin_for_snapshot_or_default` → determines `ClipboardChangeOrigin`
2. `CaptureClipboardUseCase::execute_with_origin()` → persists entry, returns `entry_id`
3. If `origin == LocalCapture`: `extract_file_paths_from_snapshot()` → resolve file URIs to paths + read fs::metadata per path → build `Vec<FileCandidate>`
4. `OutboundSyncPlanner::plan(snapshot, origin, file_candidates, extracted_paths_count)` → returns `OutboundSyncPlan`
5. If `plan.clipboard.is_some()`: `spawn_blocking(|| sync_outbound_uc.execute(intent.snapshot, origin, Some(flow_id), intent.file_transfers))`
6. If `!plan.files.is_empty()`: `tokio::spawn(|| for file in plan.files { sync_outbound_file_uc.execute(path, transfer_id).await })`

**Example:**

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs:666-703
let planner = OutboundSyncPlanner::new(self.wiring_deps().settings.clone());
let plan = planner.plan(outbound_snapshot, origin, file_candidates, extracted_paths_count).await;

if let Some(clipboard_intent) = plan.clipboard {
    let outbound_sync_uc = self.usecases().sync_outbound_clipboard();
    tokio::task::spawn_blocking(move || {
        outbound_sync_uc.execute(
            clipboard_intent.snapshot,
            origin,
            Some(flow_id_str),
            clipboard_intent.file_transfers,
        )
    });
}
```

### Pattern 2: DaemonClipboardChangeHandler Extension Point

**What:** `DaemonClipboardChangeHandler::on_clipboard_changed` in `uc-daemon/src/workers/clipboard_watcher.rs` already has `CoreRuntime` and calls `CaptureClipboardUseCase`. Outbound sync is added AFTER the capture succeeds.

**Current state (lines 89–153):**

```rust
// After capture returns Ok(Some(entry_id)) — outbound sync goes HERE
match usecase.execute_with_origin(snapshot, origin).await {
    Ok(Some(entry_id)) => {
        // Broadcast WS event (existing)
        // NEW: call OutboundSyncPlanner + SyncOutboundClipboardUseCase
    }
    ...
}
```

**What needs to be added:**

- `DaemonClipboardChangeHandler` needs access to the outbound snapshot (clone snapshot before calling `execute_with_origin`, same as `AppRuntime` clones to `outbound_snapshot`)
- File path extraction (currently in `uc-tauri`; must be duplicated or moved)
- `SyncOutboundClipboardUseCase` construction (needs `TransferPayloadEncryptorAdapter` from `uc-infra`)
- `SyncOutboundFileUseCase` construction (available via `CoreUseCases::sync_outbound_file()`)

### Pattern 3: SyncOutboundClipboardUseCase Construction

**What:** `SyncOutboundClipboardUseCase::new()` requires 8 ports. All are available from `CoreRuntime::wiring_deps()`.

```rust
// Source: uc-tauri/src/bootstrap/runtime.rs:429-462
SyncOutboundClipboardUseCase::new(
    deps.clipboard.system_clipboard.clone(),
    deps.network_ports.clipboard.clone(),       // ClipboardTransportPort
    deps.network_ports.peers.clone(),            // PeerDirectoryPort
    deps.security.encryption_session.clone(),
    deps.device.device_identity.clone(),
    deps.settings.clone(),
    Arc::new(TransferPayloadEncryptorAdapter),   // uc-infra — must add dep
    deps.device.paired_device_repo.clone(),
)
```

**Important:** `uc-daemon` currently does NOT depend on `uc-infra`. `TransferPayloadEncryptorAdapter` lives in `uc-infra`. Two options:

1. Add `uc-infra` as a dependency to `uc-daemon` (simplest, consistent with how daemon already uses uc-infra via uc-bootstrap transitively)
2. Move `TransferPayloadEncryptorAdapter` to `uc-app` or `uc-core` (larger scope change, not warranted)

**Recommendation:** Add `uc-infra` as a direct dependency to `uc-daemon/Cargo.toml` and use `TransferPayloadEncryptorAdapter` directly. Check existing `uc-daemon/Cargo.toml` first — `uc-infra` may already be an indirect dependency via `uc-bootstrap`.

### Pattern 4: File Path Extraction

**What:** `extract_file_paths_from_snapshot()` in `uc-tauri/src/bootstrap/runtime.rs:808-852` parses `text/uri-list` / `file/uri-list` / `files` / `public.file-url` representations into `Vec<PathBuf>`. On macOS, calls `resolve_apfs_file_reference()`.

**Options for daemon:**

1. **Duplicate** the function in `clipboard_watcher.rs` (minimal refactoring, 40 lines)
2. **Extract** to `uc-app/src/usecases/clipboard/file_path_extractor.rs` as a pub free function (better, eliminates future Phase 64 work)
3. **Inline** extraction in the handler (workable for Phase 61, but not ideal)

**Recommendation:** Extract to `uc-app` as a pub free function `extract_file_paths_from_snapshot(snapshot: &SystemClipboardSnapshot) -> Vec<PathBuf>`. The APFS resolver is macOS-specific and lives in `uc-tauri`; `uc-app` can provide the cross-platform base and let callers inject the APFS resolver or handle it at the call site. Alternatively, duplicate with a `#[cfg(target_os = "macos")]` guard directly in the daemon handler.

**Simplest path for Phase 61:** Duplicate in `clipboard_watcher.rs` without APFS resolution (daemon runs on macOS too, but APFS is a file system detail the daemon can add later). Mark with a TODO comment.

### Anti-Patterns to Avoid

- **Calling `SyncOutboundClipboardUseCase::execute()` directly in async context without `spawn_blocking`:** The method internally calls `executor::block_on()` which panics if called from within an async executor. Always use `tokio::task::spawn_blocking`.
- **Re-syncing `RemotePush` origin:** `OutboundSyncPlanner::plan()` already guards against this, but origin must be passed correctly. The `DaemonClipboardChangeHandler` correctly computes origin via `ClipboardChangeOriginPort` (for Phase 62 inbound sync loop prevention).
- **Constructing `SyncOutboundClipboardUseCase` outside of the capture success branch:** The use case is cheap to construct (no IO), but should only execute when capture succeeded. No need to build it before the capture result is known.
- **Missing the `outbound_snapshot` clone:** In `AppRuntime`, the snapshot is cloned to `outbound_snapshot` BEFORE being moved into `execute_with_origin()`. The daemon handler must do the same — clone the snapshot before passing to `usecase.execute_with_origin(snapshot, origin)`.

## Don't Hand-Roll

| Problem                  | Don't Build           | Use Instead                                                           | Why                                                                              |
| ------------------------ | --------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Sync eligibility policy  | Custom peer filter    | `OutboundSyncPlanner::plan()`                                         | Handles RemotePush guard, file_sync toggle, all_files_excluded, settings failure |
| Peer fanout              | Direct peer iteration | `SyncOutboundClipboardUseCase::execute()`                             | Handles encryption, V3 framing, ensure_business_path, partial failure            |
| File transfer initiation | Direct network call   | `SyncOutboundFileUseCase::execute()`                                  | Handles peer discovery, settings filter, transfer id tracking                    |
| Origin detection         | Custom hash map       | `ClipboardChangeOriginPort::consume_origin_for_snapshot_or_default()` | Already wired in `DaemonClipboardChangeHandler`, prevents write-back loops       |

**Key insight:** All outbound sync logic is already implemented in `uc-app`. The daemon's job is purely to call these use cases in the right sequence with the right parameters, not to re-implement any sync policy.

## Common Pitfalls

### Pitfall 1: `execute()` is synchronous — panics in async context

**What goes wrong:** `SyncOutboundClipboardUseCase::execute()` calls `executor::block_on()` internally. Calling it from within `on_clipboard_changed` (which is async) will panic with "cannot start a runtime from within a runtime".
**Why it happens:** The use case was designed to be called from a blocking thread context.
**How to avoid:** Always use `tokio::task::spawn_blocking(move || outbound_sync_uc.execute(...))`. The `AppRuntime` does exactly this (runtime.rs:681).
**Warning signs:** Tokio panic at runtime: "thread panicked at 'Cannot start a runtime from within a runtime'".

### Pitfall 2: Snapshot moved before clone for outbound

**What goes wrong:** `usecase.execute_with_origin(snapshot, origin)` takes ownership of `snapshot`. If `outbound_snapshot` is not cloned first, the compiler prevents reuse and the sync gets the wrong (or no) data.
**Why it happens:** Rust ownership — `execute_with_origin` consumes the snapshot.
**How to avoid:** Clone snapshot to `outbound_snapshot` before calling `execute_with_origin`, exactly like `AppRuntime::on_clipboard_changed` line 586 (`let outbound_snapshot = snapshot.clone()`).
**Warning signs:** Compile error "use of moved value: snapshot".

### Pitfall 3: uc-infra missing from uc-daemon Cargo.toml

**What goes wrong:** `TransferPayloadEncryptorAdapter` is in `uc-infra`. If `uc-daemon` doesn't declare it as a direct dependency, the compiler will fail.
**Why it happens:** `uc-daemon` currently only depends on `uc-bootstrap`, `uc-app`, `uc-core`, `uc-platform`, and `uc-infra::clipboard::InMemoryClipboardChangeOrigin` (the latter may already bring in uc-infra transitively).
**How to avoid:** Add `uc-infra = { path = "../../crates/uc-infra" }` to `uc-daemon/Cargo.toml`. Verify by checking current Cargo.toml first.
**Warning signs:** Compile error "use of undeclared crate or module `uc_infra`".

### Pitfall 4: `extracted_paths_count` / `file_candidates` confusion

**What goes wrong:** Passing `0` for `extracted_paths_count` when the snapshot actually had file representations causes the planner to incorrectly allow clipboard sync even when all files were excluded by metadata failure.
**Why it happens:** `OutboundSyncPlanner` uses `extracted_paths_count > 0 && eligible_files.is_empty()` to detect "all files excluded by metadata failure" — this guard only works when the count reflects the actual number of extracted paths.
**How to avoid:** Set `extracted_paths_count = resolved_paths.len()` BEFORE filtering by `fs::metadata()`. Same order as `AppRuntime` lines 638-664.
**Warning signs:** File clipboard items sync successfully even when all files exceed size limit.

### Pitfall 5: No WS event for outbound sync started/completed

**What goes wrong:** Frontend may not know outbound sync is happening — the current `clipboard.new_content` WS event only indicates a local capture occurred, not that sync was dispatched.
**Why it happens:** Phase 61 scope is outbound sync triggering; WS event for sync status may be out of scope.
**How to avoid:** This is explicitly out of scope for Phase 61 (no frontend changes per REQUIREMENTS.md "GUI feature changes: Not needed for runtime mode separation"). Log outbound sync completion at `info!` level for observability.

## Code Examples

### 1. Complete handler extension (reference pattern)

```rust
// Source: uc-tauri/src/bootstrap/runtime.rs:568-757 (adapted for daemon)
async fn on_clipboard_changed(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
    let snapshot_hash = snapshot.snapshot_hash().to_string();
    let origin = self.clipboard_change_origin
        .consume_origin_for_snapshot_or_default(&snapshot_hash, ClipboardChangeOrigin::LocalCapture)
        .await;

    let outbound_snapshot = snapshot.clone();  // Clone BEFORE moving into capture

    let usecase = self.build_capture_use_case();
    match usecase.execute_with_origin(snapshot, origin).await {
        Ok(Some(entry_id)) => {
            // ... existing WS broadcast code ...

            // Outbound sync: extract file candidates
            let resolved_paths = if origin == ClipboardChangeOrigin::LocalCapture {
                extract_file_paths_from_snapshot(&outbound_snapshot)
            } else {
                vec![]
            };
            let extracted_paths_count = resolved_paths.len();
            let file_candidates: Vec<FileCandidate> = resolved_paths
                .into_iter()
                .filter_map(|path| {
                    std::fs::metadata(&path).ok().map(|meta| FileCandidate {
                        path,
                        size: meta.len(),
                    })
                })
                .collect();

            let deps = self.runtime.wiring_deps();
            let planner = OutboundSyncPlanner::new(deps.settings.clone());
            let plan = planner.plan(outbound_snapshot, origin, file_candidates, extracted_paths_count).await;

            if let Some(clipboard_intent) = plan.clipboard {
                let outbound_sync_uc = build_sync_outbound_clipboard_use_case(&self.runtime);
                tokio::task::spawn_blocking(move || {
                    outbound_sync_uc.execute(
                        clipboard_intent.snapshot,
                        origin,
                        None,  // no flow_id for daemon (or generate one)
                        clipboard_intent.file_transfers,
                    )
                });
            }

            if !plan.files.is_empty() {
                let deps = self.runtime.wiring_deps();
                let outbound_file_uc = SyncOutboundFileUseCase::new(
                    deps.settings.clone(),
                    deps.device.paired_device_repo.clone(),
                    deps.network_ports.peers.clone(),
                    deps.network_ports.file_transfer.clone(),
                );
                tokio::spawn(async move {
                    for file_intent in plan.files {
                        let _ = outbound_file_uc.execute(file_intent.path, Some(file_intent.transfer_id)).await;
                    }
                });
            }
        }
        // ... existing Ok(None) and Err branches
    }
}
```

### 2. SyncOutboundClipboardUseCase construction for daemon

```rust
// Dependencies available via self.runtime.wiring_deps()
fn build_sync_outbound_clipboard_use_case(runtime: &CoreRuntime) -> SyncOutboundClipboardUseCase {
    let deps = runtime.wiring_deps();
    SyncOutboundClipboardUseCase::new(
        deps.clipboard.system_clipboard.clone(),
        deps.network_ports.clipboard.clone(),
        deps.network_ports.peers.clone(),
        deps.security.encryption_session.clone(),
        deps.device.device_identity.clone(),
        deps.settings.clone(),
        Arc::new(uc_infra::clipboard::TransferPayloadEncryptorAdapter),
        deps.device.paired_device_repo.clone(),
    )
}
```

### 3. extract_file_paths_from_snapshot (to extract or duplicate)

```rust
// Source: uc-tauri/src/bootstrap/runtime.rs:808-852
// Parses text/uri-list / file/uri-list / files / public.file-url representations
fn extract_file_paths_from_snapshot(snapshot: &SystemClipboardSnapshot) -> Vec<PathBuf> {
    // ... parses file:// URIs from representations ...
    // On macOS: calls resolve_apfs_file_reference() — macOS-only
}
```

## State of the Art

| Old Approach                                 | Current Approach                      | When Changed    | Impact                                                      |
| -------------------------------------------- | ------------------------------------- | --------------- | ----------------------------------------------------------- |
| OutboundSync inside wiring.rs                | `OutboundSyncPlanner` in uc-app       | v0.3.0 Phase 35 | Single policy decision point — daemon must use same planner |
| Tauri-owned clipboard watcher                | Daemon-owned `ClipboardWatcherWorker` | Phase 57        | Daemon now has the snapshot at capture time                 |
| file_transfer_wiring.rs standalone           | `FileTransferOrchestrator` in uc-app  | Phase 60        | Daemon can reuse orchestrator in Phase 63                   |
| `SyncOutboundClipboardUseCase::execute` sync | No change                             | —               | Still uses `executor::block_on`; must use `spawn_blocking`  |

## Open Questions

1. **Should `extract_file_paths_from_snapshot` be moved to `uc-app`?**
   - What we know: It currently lives in `uc-tauri/runtime.rs` as a private function; the APFS resolver is macOS-specific and uses CoreFoundation which is platform-specific.
   - What's unclear: Whether `uc-app` can host platform-conditional code for macOS, or whether it should remain behind the platform boundary.
   - Recommendation: For Phase 61, duplicate the cross-platform base in `clipboard_watcher.rs` (skip APFS resolution or add it behind `#[cfg(target_os = "macos")]`). Phase 64 (Tauri sync retirement) is the right time to consolidate.

2. **Does `uc-daemon` currently have `uc-infra` as a direct dependency?**
   - What we know: `uc-daemon/src/main.rs` imports `use uc_infra::clipboard::InMemoryClipboardChangeOrigin;` (line 27) — so uc-infra IS already a direct dep.
   - What's unclear: Whether `uc_infra::clipboard::TransferPayloadEncryptorAdapter` is in the same `uc_infra::clipboard` module — confirmed it is.
   - Recommendation: No Cargo.toml change needed. Verify the exact import path is `uc_infra::clipboard::TransferPayloadEncryptorAdapter`.

3. **Should outbound sync emit a WS event back to GUI clients?**
   - What we know: Current `clipboard.new_content` event covers local capture; outbound sync success/failure is only logged in AppRuntime.
   - What's unclear: Whether Phase 61 should add a new `clipboard.sync_sent` WS event or rely on logging.
   - Recommendation: Out of scope for Phase 61. Log at `info!` level same as AppRuntime.

## Environment Availability

Step 2.6: SKIPPED (no external dependencies — all ports are workspace-internal, no new CLI tools or services required).

## Validation Architecture

### Test Framework

| Property           | Value                                               |
| ------------------ | --------------------------------------------------- |
| Framework          | cargo test (Rust)                                   |
| Config file        | src-tauri/Cargo.toml                                |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon`           |
| Full suite command | `cd src-tauri && cargo test -p uc-daemon -p uc-app` |

### Phase Requirements → Test Map

| Req ID  | Behavior                                                                 | Test Type | Automated Command                                            | File Exists? |
| ------- | ------------------------------------------------------------------------ | --------- | ------------------------------------------------------------ | ------------ |
| PH61-01 | `DaemonClipboardChangeHandler` triggers outbound sync after LocalCapture | unit      | `cd src-tauri && cargo test -p uc-daemon clipboard_watcher`  | ❌ Wave 0    |
| PH61-02 | RemotePush origin skips outbound sync (no double-sync)                   | unit      | `cd src-tauri && cargo test -p uc-daemon clipboard_watcher`  | ❌ Wave 0    |
| PH61-03 | `OutboundSyncPlanner` settings load failure uses safe defaults           | unit      | `cd src-tauri && cargo test -p uc-app sync_planner`          | ✅ existing  |
| PH61-04 | File candidates built correctly from file URI representations            | unit      | `cd src-tauri && cargo test -p uc-daemon extract_file_paths` | ❌ Wave 0    |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo check -p uc-daemon`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-daemon -p uc-app`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — add test module for outbound sync dispatch (mock `SyncOutboundClipboardUseCase` or stub network)
- [ ] `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — test: `RemotePush` origin produces no outbound sync call

_(Existing `OutboundSyncPlanner` tests in `uc-app/src/usecases/sync_planner/planner.rs` cover sync eligibility logic — no Wave 0 gap there)_

## Sources

### Primary (HIGH confidence)

- `uc-tauri/src/bootstrap/runtime.rs` lines 562–757 — canonical `AppRuntime::on_clipboard_changed` outbound sync implementation (direct code inspection)
- `uc-app/src/usecases/clipboard/sync_outbound.rs` — `SyncOutboundClipboardUseCase` full API with `execute()` signature
- `uc-app/src/usecases/sync_planner/planner.rs` — `OutboundSyncPlanner::plan()` API and behavior
- `uc-daemon/src/workers/clipboard_watcher.rs` — current `DaemonClipboardChangeHandler` implementation
- `uc-daemon/src/main.rs` — daemon composition root confirming `uc-infra` already imported
- `uc-app/src/deps.rs` — `AppDeps`/`NetworkPorts`/`ClipboardPorts` structure

### Secondary (MEDIUM confidence)

- `uc-app/src/usecases/file_sync/sync_outbound.rs` — `SyncOutboundFileUseCase` API (confirmed in `CoreUseCases::sync_outbound_file`)
- `.planning/STATE.md` decisions — `[v0.3.0]: OutboundSyncPlanner consolidation — single policy decision point`

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all APIs confirmed by direct source inspection
- Architecture: HIGH — complete AppRuntime reference implementation exists, daemon just mirrors it
- Pitfalls: HIGH — `spawn_blocking` requirement confirmed by `executor::block_on` in `execute()`, snapshot clone requirement confirmed by Rust ownership rules

**Research date:** 2026-03-25
**Valid until:** 2026-04-25 (stable domain — no external dependencies)
