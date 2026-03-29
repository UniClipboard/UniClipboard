# Phase 73: Refactor Clipboard Restore Loop Prevention - Research

**Researched:** 2026-03-29
**Domain:** Rust refactor — clipboard write orchestration, origin guard centralisation, hexagonal architecture (uc-app / uc-bootstrap / uc-daemon)
**Confidence:** HIGH

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**D-01:** `ClipboardWriteCoordinator` provides a single `write(snapshot: SystemClipboardSnapshot, intent: ClipboardWriteIntent) -> Result<()>` method as the **only** entry point for all programmatic clipboard writes

**D-02:** `ClipboardWriteIntent` enum has three variants: `LocalRestore`, `RemotePush`, `LocalCapture` (for test scenarios)

**D-03:** Coordinator owns the entire write pipeline: compute `origin_guard_key()` → call `remember_local/remote_snapshot_hash()` → call `write_snapshot()` → consume guard on failure

**D-04:** Coordinator TTL for guards: 2 seconds (matching the existing `Duration::from_secs(2)` in `RestoreClipboardSelectionUseCase`)

**D-05:** `RestoreClipboardSelectionUseCase::build_snapshot()` **stays** in the use case with full business logic (query entry, select representation, read blob)

**D-06:** `RestoreClipboardSelectionUseCase::execute()` delegates snapshot construction to `build_snapshot()`, then calls `coordinator.write(snapshot, LocalRestore)` instead of `restore_snapshot()` method

**D-07:** `CopyFileToClipboardUseCase::execute()` delegates to `coordinator.write(snapshot, LocalRestore)` instead of `write_files_to_clipboard()` method

**D-08:** `InboundClipboardSyncWorker` calls `coordinator.write(snapshot, RemotePush)` instead of manually calling `remember_remote_snapshot_hash` + `write_snapshot`

**D-09:** Coordinator is the **sole** caller of `snapshot.origin_guard_key()` — external callers never directly invoke `origin_guard_key()` anymore

**D-10:** If future key derivation rules change, only `ClipboardWriteCoordinator::write()` needs updating (one location)

**D-11:** `InMemoryClipboardChangeOrigin` is constructed exactly once in `uc-bootstrap/assembly.rs` (via `build_core()`) and exposed through `WiringDeps::clipboard.clipboard_change_origin`

**D-12:** `ClipboardWriteCoordinator` is constructed in `uc-bootstrap/assembly.rs` and takes the shared `Arc<dyn ClipboardChangeOriginPort>` as a constructor dependency

**D-13:** All daemon workers (`DaemonClipboardChangeHandler`, `InboundClipboardSyncWorker`, `FileSyncOrchestratorWorker`) receive `ClipboardWriteCoordinator` (not raw `ClipboardChangeOriginPort`)

**D-14:** Direct construction of `InMemoryClipboardChangeOrigin` outside `uc-bootstrap/assembly.rs` is prevented — the struct remains `pub(crate)` in `uc-infra` and the only constructor is `pub fn new()` callable from bootstrap

**D-15:** `ClipboardWriteCoordinator` lives in `uc-app/src/usecases/clipboard/clipboard_write_coordinator.rs` following the `FileTransferOrchestrator` pattern

**D-16:** Coordinator is accessible via `CoreUseCases::clipboard_write_coordinator()` accessor (alongside existing `restore_clipboard_selection()`, `copy_file_to_clipboard()`, etc.)

### Claude's Discretion

- Exact struct field layout of `ClipboardWriteCoordinator`
- Whether `restore_snapshot()` method on `RestoreClipboardSelectionUseCase` is deleted or kept as a delegating shim
- Whether `CopyFileToClipboardUseCase::write_files_to_clipboard()` is deleted or kept as a delegating shim
- Internal error types and logging details
- How `InboundClipboardSyncWorker` transitions from manual guard calls to coordinator calls (refactor in-place or rewrite)

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope.
</user_constraints>

---

## Summary

Phase 73 centralises clipboard write orchestration behind a new `ClipboardWriteCoordinator` struct. Today the guard-registration pattern (`origin_guard_key()` → `remember_local/remote_snapshot_hash()` → `write_snapshot()` → cleanup-on-error) is duplicated in three callsites: `RestoreClipboardSelectionUseCase::restore_snapshot()`, `CopyFileToClipboardUseCase::write_files_to_clipboard()`, and `SyncInboundClipboardUseCase` (inside the V3 inbound path). Additionally, `restore_file_to_clipboard_after_transfer` in `FileSyncOrchestratorWorker` hand-rolls the same pattern. Each callsite also keeps its own direct reference to `Arc<dyn ClipboardChangeOriginPort>`, and the daemon entrypoint in `entrypoint.rs` extracts the raw Arc and passes it to every worker individually — creating the risk that a future developer could accidentally construct a second `InMemoryClipboardChangeOrigin` somewhere.

The fix is a single `ClipboardWriteCoordinator` that (1) holds `Arc<dyn SystemClipboardPort>` + `Arc<dyn ClipboardChangeOriginPort>`, (2) exposes only `write(snapshot, intent)`, and (3) is built once in `uc-bootstrap/assembly.rs` and stored in `BackgroundRuntimeDeps`, mirroring the `FileTransferOrchestrator` pattern. Workers and use cases then accept `Arc<ClipboardWriteCoordinator>` instead of the raw origin port.

The `SyncInboundClipboardUseCase` inbound path is a special case: the guard registration is currently buried inside the use case implementation (around line 530 of `sync_inbound.rs`). Per D-08, that internal guard registration must move so the `InboundClipboardSyncWorker` calls `coordinator.write(snapshot, RemotePush)` instead. This means the inbound use case must be refactored to return the constructed `SystemClipboardSnapshot` to the caller rather than writing it internally — or alternatively the use case's OS-write step is extracted into a separate method that the worker replaces with the coordinator call.

**Primary recommendation:** Follow the `FileTransferOrchestrator` template exactly. Build `ClipboardWriteCoordinator` in `assembly.rs` via `build_clipboard_write_coordinator()`, store it in `BackgroundRuntimeDeps`, expose it via `CoreUseCases::clipboard_write_coordinator()`, and update all four callsites in one pass.

---

## Standard Stack

### Core (no new dependencies)

| Library / Crate | Version   | Purpose                                                                                                | Why Standard                       |
| --------------- | --------- | ------------------------------------------------------------------------------------------------------ | ---------------------------------- |
| `uc-core`       | workspace | `SystemClipboardPort`, `ClipboardChangeOriginPort`, `SystemClipboardSnapshot`, `ClipboardChangeOrigin` | Domain types — already in scope    |
| `uc-app`        | workspace | `ClipboardWriteCoordinator` target location; `CoreUseCases` accessor host                              | App layer owns write orchestration |
| `uc-bootstrap`  | workspace | `build_clipboard_write_coordinator()` builder; `BackgroundRuntimeDeps` extension                       | Sole composition root              |
| `uc-daemon`     | workspace | Worker refactor targets                                                                                | Workers consume coordinator        |
| `uc-infra`      | workspace | `InMemoryClipboardChangeOrigin` visibility change to `pub(crate)`                                      | Locks down construction site       |
| `tokio`         | workspace | Async execution, already used by all workers                                                           | No change                          |
| `tracing`       | workspace | `info_span!`, `instrument` — matching existing logging patterns                                        | No change                          |
| `anyhow`        | workspace | `Result<()>` error propagation — matches all existing use cases                                        | No change                          |

**Installation:** No new Cargo.toml changes needed.

---

## Architecture Patterns

### Recommended New File Structure

```
src-tauri/crates/uc-app/src/usecases/clipboard/
├── clipboard_write_coordinator.rs   ← NEW
├── mod.rs                           ← add pub mod + pub use
├── restore_clipboard_selection.rs   ← refactor execute(), optionally remove restore_snapshot()
└── sync_inbound.rs                  ← extract OS-write step for coordinator hand-off
```

### Pattern 1: ClipboardWriteCoordinator Struct

Mirrors `FileTransferOrchestrator` which holds `Arc<Inner>` and is cloned cheaply across workers.

```rust
// Source: project pattern from file_transfer_orchestrator.rs
pub struct ClipboardWriteCoordinator {
    system_clipboard: Arc<dyn SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
}

impl ClipboardWriteCoordinator {
    pub fn new(
        system_clipboard: Arc<dyn SystemClipboardPort>,
        clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    ) -> Self {
        Self { system_clipboard, clipboard_change_origin }
    }

    pub async fn write(
        &self,
        snapshot: SystemClipboardSnapshot,
        intent: ClipboardWriteIntent,
    ) -> Result<()> {
        let origin_guard_key = snapshot.origin_guard_key();
        let ttl = Duration::from_secs(2);
        match intent {
            ClipboardWriteIntent::LocalRestore | ClipboardWriteIntent::LocalCapture => {
                self.clipboard_change_origin
                    .remember_local_snapshot_hash(origin_guard_key.clone(), ttl)
                    .await;
            }
            ClipboardWriteIntent::RemotePush => {
                self.clipboard_change_origin
                    .remember_remote_snapshot_hash(origin_guard_key.clone(), ttl)
                    .await;
            }
        }
        if let Err(err) = self.system_clipboard.write_snapshot(snapshot) {
            self.clipboard_change_origin
                .consume_origin_for_snapshot_or_default(
                    &origin_guard_key,
                    ClipboardChangeOrigin::LocalCapture,
                )
                .await;
            return Err(err);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardWriteIntent {
    LocalRestore,
    RemotePush,
    LocalCapture,
}
```

### Pattern 2: build_clipboard_write_coordinator() in assembly.rs

Mirrors the existing `build_file_transfer_orchestrator()` function in `assembly.rs` (line 1042):

```rust
// Source: assembly.rs build_file_transfer_orchestrator pattern
pub fn build_clipboard_write_coordinator(
    system_clipboard: Arc<dyn SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
) -> Arc<ClipboardWriteCoordinator> {
    Arc::new(ClipboardWriteCoordinator::new(system_clipboard, clipboard_change_origin))
}
```

Called inside `wire_dependencies_with_identity_store()` right after the `clipboard_change_origin` construction at line 790, and stored in `BackgroundRuntimeDeps`.

### Pattern 3: CoreUseCases Accessor

Mirrors every other use case accessor in `mod.rs`. The coordinator is stored on `CoreRuntime` (like `file_transfer_orchestrator`) or retrieved from `BackgroundRuntimeDeps`:

```rust
// Source: CoreUseCases::setup_orchestrator() pattern in mod.rs
pub fn clipboard_write_coordinator(&self) -> Arc<ClipboardWriteCoordinator> {
    self.runtime.clipboard_write_coordinator().clone()
}
```

### Pattern 4: RestoreClipboardSelectionUseCase Refactor

The `execute()` method currently calls `self.restore_snapshot(entry_id, snapshot)`. After refactor:

```rust
// Source: restore_clipboard_selection.rs execute() + daemon routes.rs pattern
pub async fn execute(&self, entry_id: &EntryId) -> Result<()> {
    info!(entry_id = %entry_id, "restore.execute requested");
    let snapshot = self.build_snapshot(entry_id).await?;
    self.coordinator.write(snapshot, ClipboardWriteIntent::LocalRestore).await
}
```

`build_snapshot()` remains intact (D-05). The `restore_snapshot()` method may be deleted or retained as a shim — deletion is cleaner.

### Pattern 5: SyncInboundClipboardUseCase OS-Write Extraction

The guard registration at `sync_inbound.rs:530` is currently buried inside the inbound apply logic. The D-08 decision says `InboundClipboardSyncWorker` calls `coordinator.write(snapshot, RemotePush)`. This requires extracting the OS-clipboard-write step from inside `SyncInboundClipboardUseCase`.

Two valid approaches:

**Option A (in-place refactor):** The use case calls `coordinator.write()` directly (coordinator injected as constructor dependency). This is fully contained inside the use case layer.

**Option B (caller takes control):** The use case returns the `SystemClipboardSnapshot` to the worker, which then calls `coordinator.write()`. This is a larger API surface change.

Option A is simpler and more consistent with the existing pattern (use cases own business logic; coordinator is just another dependency). The use case already holds `Arc<dyn SystemClipboardPort>` and `Arc<dyn ClipboardChangeOriginPort>` — these are replaced by `Arc<ClipboardWriteCoordinator>`.

### Pattern 6: InMemoryClipboardChangeOrigin Visibility Lock

In `uc-infra/src/clipboard/change_origin.rs`, the struct is `pub struct InMemoryClipboardChangeOrigin`. Change to `pub(crate)`:

```rust
pub(crate) struct InMemoryClipboardChangeOrigin { ... }
```

Assembly.rs already imports it from `uc_infra::clipboard::InMemoryClipboardChangeOrigin` — this import stays valid as `uc-bootstrap` is in the same workspace and `uc-infra` is a direct dependency of `uc-bootstrap`. The `pub(crate)` restriction prevents other workspace crates from constructing it.

Note: If `uc-infra/src/clipboard/mod.rs` re-exports `InMemoryClipboardChangeOrigin` with `pub use`, that re-export must also be narrowed to `pub(crate) use` or removed.

### Anti-Patterns to Avoid

- **Passing raw `Arc<dyn ClipboardChangeOriginPort>` to workers post-refactor:** All workers should receive `Arc<ClipboardWriteCoordinator>` going forward (D-13). Do not leave `clipboard_change_origin` in `FileSyncOrchestratorWorker::new()` signature after the refactor.
- **Calling `snapshot.origin_guard_key()` outside the coordinator:** After refactor, only `ClipboardWriteCoordinator::write()` calls it (D-09).
- **Calling `remember_remote_snapshot_hash` or `remember_local_snapshot_hash` outside the coordinator:** These are now coordinator-internal concerns.
- **Building `InMemoryClipboardChangeOrigin` in tests inside uc-daemon or uc-tauri:** Tests that need an origin store should construct `ClipboardWriteCoordinator` with a mock `ClipboardChangeOriginPort`.
- **Returning `Result<()>` with a TTL parameter:** The coordinator owns the TTL constant (2s); callers must not pass TTL.

---

## Don't Hand-Roll

| Problem                        | Don't Build                 | Use Instead                                                                    | Why                                                                     |
| ------------------------------ | --------------------------- | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------- |
| Thread-safe shared coordinator | Custom Mutex-wrapped struct | `Arc<ClipboardWriteCoordinator>` clone (existing pattern)                      | All fields are already `Arc<dyn Trait>` — no additional locking needed  |
| Guard cleanup on failure       | Custom drop guard or RAII   | The `consume_origin_for_snapshot_or_default` call inside `coordinator.write()` | Already proven correct in three existing callsites                      |
| Multiple coordinator instances | Separate factory per worker | `Arc<ClipboardWriteCoordinator>` cloned in `BackgroundRuntimeDeps`             | Same Arc must be shared; new instances break the single-store invariant |

---

## Common Pitfalls

### Pitfall 1: SyncInboundClipboardUseCase Has Guard Logic in Two Places

**What goes wrong:** `sync_inbound.rs` calls `remember_remote_snapshot_hash` at line 531 (V3 Full-mode OS write). If you only update the worker without also removing this internal call, the guard is registered twice (once by the coordinator, once by the use case).

**Why it happens:** The OS-write and guard-registration inside `SyncInboundClipboardUseCase` were added in Phase 62 when there was no coordinator abstraction.

**How to avoid:** When refactoring `InboundClipboardSyncWorker` (D-08), also remove the `remember_remote_snapshot_hash` call and the paired `consume_origin_for_snapshot_or_default` cleanup inside `SyncInboundClipboardUseCase`. The use case's Full-mode OS-write path becomes: build snapshot → return snapshot to caller. The worker then calls `coordinator.write(snapshot, RemotePush)`.

**Warning signs:** Both `InboundClipboardSyncWorker` AND `SyncInboundClipboardUseCase` call `remember_remote_snapshot_hash` in the same code path after the refactor.

### Pitfall 2: FileSyncOrchestratorWorker Has Its Own Standalone Function

**What goes wrong:** `restore_file_to_clipboard_after_transfer()` in `file_sync_orchestrator.rs` (lines 299-392) is a free async function that manually does the full guard-register → write → cleanup-on-error pattern. This function must be replaced by a `coordinator.write()` call. Forgetting to update this callsite leaves a bypass.

**Why it happens:** This function was added by Phase 63 directly in the daemon worker file, separate from the use case layer.

**How to avoid:** Replace the free function body with a `coordinator.write(snapshot, LocalRestore)` call. The path-canonicalization and existence-check logic that precedes the write stays as-is. The `has_pending_origin()` check (FCLIP-03 guard) stays as-is.

**Warning signs:** `restore_file_to_clipboard_after_transfer` still contains direct calls to `remember_local_snapshot_hash` after the refactor.

### Pitfall 3: Coordinator Stored in Wrong Place

**What goes wrong:** If the coordinator is stored only in `CoreRuntime` (via `CoreUseCases` accessor) but not in `BackgroundRuntimeDeps`, then daemon workers that receive dependencies at construction time cannot get a clone of it before `CoreRuntime` is built. Daemon workers are constructed in `entrypoint.rs` from `BackgroundRuntimeDeps`.

**Why it happens:** `CoreUseCases` is a short-lived accessor (`&self` borrow of runtime), not an owned `Arc`. Workers need an owned `Arc<ClipboardWriteCoordinator>` at construction time.

**How to avoid:** Store `Arc<ClipboardWriteCoordinator>` in `BackgroundRuntimeDeps` (like `file_transfer_orchestrator`). The `CoreUseCases::clipboard_write_coordinator()` accessor then reads from `self.runtime.clipboard_write_coordinator` (a field on `CoreRuntime`) which was populated from `BackgroundRuntimeDeps` at bootstrap time.

**Warning signs:** `entrypoint.rs` cannot find `clipboard_write_coordinator` in its runtime object at worker construction time.

### Pitfall 4: InMemoryClipboardChangeOrigin pub(crate) Breaks uc-infra Re-export

**What goes wrong:** `uc-infra/src/clipboard/mod.rs` currently re-exports `InMemoryClipboardChangeOrigin` as `pub`. If you only change the struct visibility to `pub(crate)`, the `pub use` in `mod.rs` will fail to compile because you cannot re-export an item with broader visibility than the item itself.

**How to avoid:** Change both the struct definition (`pub struct` → `pub(crate) struct`) AND any `pub use` re-export in `uc-infra/src/clipboard/mod.rs` to `pub(crate) use` (or remove the re-export entirely).

**Warning signs:** Compilation error: "visibility of re-exported item is more permissive than visibility of item".

### Pitfall 5: RemotePush TTL Mismatch

**What goes wrong:** `SyncInboundClipboardUseCase` currently uses `REMOTE_SNAPSHOT_HASH_TTL_MS` (a constant defined in `sync_inbound.rs`) rather than `Duration::from_secs(2)`. If the coordinator always uses 2 seconds, the effective TTL for remote guards changes.

**Why it happens:** The remote path had its own TTL constant that may differ from the 2-second local restore TTL.

**How to avoid:** Read the value of `REMOTE_SNAPSHOT_HASH_TTL_MS` in `sync_inbound.rs` before finalising the coordinator TTL. Per D-04, the coordinator TTL is 2 seconds. If the remote TTL constant is different and significant, document the intentional TTL unification in the commit message or consider making the TTL per-intent (Claude's discretion).

**Warning signs:** `REMOTE_SNAPSHOT_HASH_TTL_MS` != 2000 — check the actual value in `sync_inbound.rs` before coding.

---

## Code Examples

### Complete Refactor of RestoreClipboardSelectionUseCase::execute()

```rust
// Source: restore_clipboard_selection.rs (existing pattern)
// Before:
pub async fn execute(&self, entry_id: &EntryId) -> Result<()> {
    info!(entry_id = %entry_id, "restore.execute requested");
    let snapshot = self.build_snapshot(entry_id).await?;
    self.restore_snapshot(entry_id, snapshot).await
}

// After:
pub async fn execute(&self, entry_id: &EntryId) -> Result<()> {
    info!(entry_id = %entry_id, "restore.execute requested");
    let snapshot = self.build_snapshot(entry_id).await?;
    self.coordinator
        .write(snapshot, ClipboardWriteIntent::LocalRestore)
        .await
}
```

### Complete Refactor of CopyFileToClipboardUseCase::execute()

```rust
// Source: copy_file_to_clipboard.rs execute() → write_files_to_clipboard() (existing pattern)
// After (replaces the write_files_to_clipboard() body call):
let path_list = build_path_list(&file_paths);
let snapshot = build_file_snapshot(&path_list);
self.coordinator
    .write(snapshot, ClipboardWriteIntent::LocalRestore)
    .await?;
info!(file_count = file_paths.len(), "Files written to system clipboard");
```

### FileSyncOrchestratorWorker restore_file_to_clipboard_after_transfer replacement

```rust
// Source: file_sync_orchestrator.rs restore_file_to_clipboard_after_transfer (existing)
// After (replace guard+write section, keep path-canonicalization and existence checks):
let path_list = build_path_list(&file_paths);
let snapshot = build_file_snapshot(&path_list);
if let Err(err) = coordinator.write(snapshot, ClipboardWriteIntent::LocalRestore).await {
    warn!(error = %err, "Failed to write file URIs to system clipboard");
} else {
    info!(file_count = file_paths.len(), "File URIs written to system clipboard");
}
```

### build_clipboard_write_coordinator in assembly.rs

```rust
// Source: assembly.rs build_file_transfer_orchestrator() pattern (line 1042)
pub fn build_clipboard_write_coordinator(
    system_clipboard: Arc<dyn SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
) -> Arc<uc_app::usecases::clipboard::ClipboardWriteCoordinator> {
    Arc::new(uc_app::usecases::clipboard::ClipboardWriteCoordinator::new(
        system_clipboard,
        clipboard_change_origin,
    ))
}
```

Then in `wire_dependencies_with_identity_store()` after line 791:

```rust
let clipboard_write_coordinator = build_clipboard_write_coordinator(
    deps.clipboard.system_clipboard.clone(),
    deps.clipboard.clipboard_change_origin.clone(),
);
```

And store in `BackgroundRuntimeDeps`:

```rust
pub clipboard_write_coordinator: Arc<uc_app::usecases::clipboard::ClipboardWriteCoordinator>,
```

---

## Integration Points: All Four Callsites

| Callsite                                              | File                                                       | Current Pattern                                                     | After Refactor                                                                                             |
| ----------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `RestoreClipboardSelectionUseCase::execute()`         | `uc-app/usecases/clipboard/restore_clipboard_selection.rs` | calls `self.restore_snapshot()` which owns guard logic              | calls `self.coordinator.write(snapshot, LocalRestore)`                                                     |
| `CopyFileToClipboardUseCase::execute()`               | `uc-app/usecases/file_sync/copy_file_to_clipboard.rs`      | calls `self.write_files_to_clipboard()` which owns guard logic      | calls `self.coordinator.write(snapshot, LocalRestore)`                                                     |
| `SyncInboundClipboardUseCase` (V3 Full-mode OS write) | `uc-app/usecases/clipboard/sync_inbound.rs` (line ~530)    | internally calls `remember_remote_snapshot_hash` + `write_snapshot` | guard logic removed; caller (`InboundClipboardSyncWorker`) calls `coordinator.write(snapshot, RemotePush)` |
| `restore_file_to_clipboard_after_transfer()`          | `uc-daemon/workers/file_sync_orchestrator.rs` (line ~299)  | standalone async fn with full manual guard pattern                  | replaced by `coordinator.write(snapshot, LocalRestore)`                                                    |

---

## State of the Art

| Old Approach                                                  | Current Approach                                                   | When Changed | Impact                                                        |
| ------------------------------------------------------------- | ------------------------------------------------------------------ | ------------ | ------------------------------------------------------------- |
| Direct `set_next_origin` + `write_snapshot`                   | `remember_local/remote_snapshot_hash` + `write_snapshot` + cleanup | Phase 57/62  | More precise hash-based guard matching                        |
| Single `ClipboardChangeOriginPort` Arc shared via constructor | Same Arc + manual guard calls at each callsite                     | Phase 62     | Works but duplicates pattern; Phase 73 introduces coordinator |
| `ClipboardChangeOriginPort` passed to workers directly        | Will be replaced by `ClipboardWriteCoordinator`                    | Phase 73     | Callsites become one-liners; single SSOT for guard logic      |

---

## Open Questions

1. **REMOTE_SNAPSHOT_HASH_TTL_MS actual value**
   - What we know: defined as a constant in `sync_inbound.rs` around the guard registration at line 530
   - What's unclear: whether it equals 2000ms or a different value
   - Recommendation: Check before writing coordinator — if different from 2s, use the larger value or make TTL configurable per-intent. The CONTEXT.md D-04 says 2s; verify against the constant.

2. **SyncInboundClipboardUseCase OS-write extraction strategy**
   - What we know: guard registration is embedded inside the full inbound apply path
   - What's unclear: whether the snapshot is already materialised before the write call or built inline
   - Recommendation: Read sync_inbound.rs lines 510-550 carefully during planning. The coordinator should be injected as a constructor dependency (Option A) so the use case calls `coordinator.write()` directly rather than exposing snapshot to the worker.

---

## Environment Availability

Step 2.6: SKIPPED (no external tool dependencies — this phase is a pure Rust refactor with no new CLI tools, services, databases, or runtimes required)

---

## Validation Architecture

### Test Framework

| Property           | Value                                                               |
| ------------------ | ------------------------------------------------------------------- |
| Framework          | cargo test (built-in)                                               |
| Config file        | `src-tauri/Cargo.toml` workspace                                    |
| Quick run command  | `cd src-tauri && cargo test -p uc-app clipboard_write_coordinator`  |
| Full suite command | `cd src-tauri && cargo test -p uc-app -p uc-daemon -p uc-bootstrap` |

### Phase Requirements → Test Map

| Req ID  | Behavior                                                                                        | Test Type    | Automated Command                                                          | File Exists?                                    |
| ------- | ----------------------------------------------------------------------------------------------- | ------------ | -------------------------------------------------------------------------- | ----------------------------------------------- |
| PH73-01 | `ClipboardWriteCoordinator::write(LocalRestore)` registers local hash guard and writes snapshot | unit         | `cd src-tauri && cargo test -p uc-app test_coordinator_local_restore`      | ❌ Wave 0                                       |
| PH73-02 | `ClipboardWriteCoordinator::write(RemotePush)` registers remote hash guard and writes snapshot  | unit         | `cd src-tauri && cargo test -p uc-app test_coordinator_remote_push`        | ❌ Wave 0                                       |
| PH73-03 | On `write_snapshot` failure, coordinator consumes guard to prevent stale state                  | unit         | `cd src-tauri && cargo test -p uc-app test_coordinator_cleanup_on_failure` | ❌ Wave 0                                       |
| PH73-04 | `RestoreClipboardSelectionUseCase::execute()` calls coordinator instead of owning guard logic   | unit         | `cd src-tauri && cargo test -p uc-app restore_uses_coordinator`            | ❌ Wave 0 (existing tests pass once refactored) |
| PH73-05 | `CopyFileToClipboardUseCase::execute()` calls coordinator                                       | unit         | `cd src-tauri && cargo test -p uc-app copy_file_uses_coordinator`          | ❌ Wave 0                                       |
| PH73-06 | `InMemoryClipboardChangeOrigin` is `pub(crate)` — no external construction compiles             | compile-time | `cd src-tauri && cargo build -p uc-app` (no test, enforced by compiler)    | ❌ Wave 0                                       |
| PH73-07 | All existing `change_origin.rs` unit tests pass unchanged                                       | regression   | `cd src-tauri && cargo test -p uc-infra`                                   | ✅ exists                                       |
| PH73-08 | `FileSyncOrchestratorWorker` compiles without `clipboard_change_origin` field                   | compile-time | `cd src-tauri && cargo build -p uc-daemon`                                 | ✅ (will verify post-refactor)                  |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-app clipboard_write`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-app -p uc-daemon -p uc-bootstrap`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-app/src/usecases/clipboard/clipboard_write_coordinator.rs` — covers PH73-01, PH73-02, PH73-03 (new file)
- [ ] New test module `#[cfg(test)] mod tests` at bottom of `clipboard_write_coordinator.rs` — covers PH73-01 through PH73-05
- [ ] Existing tests in `restore_clipboard_selection.rs` must be updated to inject a mock coordinator

---

## Project Constraints (from CLAUDE.md)

- **Cargo commands MUST run from `src-tauri/`** — never from project root
- **No `unwrap()` or `expect()` in production code** — use `?`, `match`, or `unwrap_or_default()`
- **Never use `if let` for error cases that should be reported** — use `match` when the `Err` variant must be visible
- **Hexagonal architecture** — new coordinator lives in `uc-app` (application layer), not `uc-daemon` or `uc-infra`
- **Use `tracing` spans** — wrap each coordinator write in an `info_span!` for observability
- **No fixed pixel values in frontend** — not applicable (pure Rust phase)
- **Port/Adapter pattern** — coordinator depends on `Arc<dyn SystemClipboardPort>` + `Arc<dyn ClipboardChangeOriginPort>`, not on concrete types

---

## Sources

### Primary (HIGH confidence)

- Codebase: `src-tauri/crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs` — existing guard pattern (lines 194-248)
- Codebase: `src-tauri/crates/uc-app/src/usecases/file_sync/copy_file_to_clipboard.rs` — duplicate guard pattern (lines 162-190)
- Codebase: `src-tauri/crates/uc-daemon/src/workers/file_sync_orchestrator.rs` — third standalone guard pattern (lines 299-392)
- Codebase: `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` — fourth guard pattern (around line 530)
- Codebase: `src-tauri/crates/uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs` — template for coordinator struct pattern
- Codebase: `src-tauri/crates/uc-bootstrap/src/assembly.rs` — `build_file_transfer_orchestrator()` template (line 1042), `wire_dependencies_with_identity_store()` composition point (line 710-881)
- Codebase: `src-tauri/crates/uc-app/src/usecases/mod.rs` — `CoreUseCases` accessor pattern
- Codebase: `src-tauri/crates/uc-daemon/src/entrypoint.rs` — current shared-Arc extraction pattern (lines 107-146)
- Codebase: `src-tauri/crates/uc-infra/src/clipboard/change_origin.rs` — `InMemoryClipboardChangeOrigin` (pub struct, needs pub(crate))
- Codebase: `src-tauri/crates/uc-core/src/ports/clipboard/clipboard_change_origin.rs` — port trait
- Codebase: `src-tauri/crates/uc-core/src/clipboard/system.rs:230` — `origin_guard_key()` method

### Secondary (MEDIUM confidence)

- `.planning/phases/73-.../73-CONTEXT.md` — locked decisions D-01 through D-16 (project-specific, HIGH within project scope)

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — pure Rust refactor, no new dependencies, all patterns already established in codebase
- Architecture: HIGH — coordinator pattern directly mirrors `FileTransferOrchestrator`, all callsites identified and read
- Pitfalls: HIGH — identified from direct code reading: duplicate TTL constant, buried guard in sync_inbound, re-export visibility, standalone function in daemon worker

**Research date:** 2026-03-29
**Valid until:** 2026-04-28 (stable codebase, not fast-moving)
