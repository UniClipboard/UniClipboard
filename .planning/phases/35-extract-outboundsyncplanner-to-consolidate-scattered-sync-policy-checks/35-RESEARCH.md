# Phase 35: Extract OutboundSyncPlanner - Research

**Researched:** 2026-03-16
**Domain:** Rust internal refactoring — sync policy consolidation in uc-app
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Types and location**

- Define `OutboundSyncPlan`, `ClipboardSyncIntent`, `FileSyncIntent` in `uc-app`
- `OutboundSyncPlan` has optional `clipboard` field and `files` vec
- `ClipboardSyncIntent` carries the snapshot and file transfer mappings
- `FileSyncIntent` carries path, transfer_id, and filename per eligible file

**Planner design**

- `OutboundSyncPlanner::plan(snapshot, origin, settings) -> OutboundSyncPlan` consolidates all eligibility logic:
  - Load settings once (not per-check)
  - Classify content type (once, before peer loop — Phase 25 decision)
  - Extract file paths (only for LocalCapture + file_sync_enabled)
  - Filter files by max_file_size
  - Determine clipboard sync eligibility (skip when all files excluded)
- Runtime becomes thin dispatch: plan → spawn clipboard sync if plan.clipboard.is_some() → spawn file sync for plan.files

**Redundant check removal**

- Gradually remove Stage 2/3 defensive checks in `SyncOutboundClipboardUseCase` and `SyncOutboundFileUseCase` since the Plan already guarantees correctness
- Keep removal within this phase scope per Issue #279

**Testing approach**

- Planner is unit-testable: mock settings + snapshot → assert plan output
- Cover edge cases: oversized files, file_sync disabled, mixed file sizes, all files excluded, non-LocalCapture origin

### Claude's Discretion

- Whether `plan()` is async (due to settings load) or takes pre-loaded settings as parameter
- Internal module organization within uc-app (new file vs existing module)
- Exact method signatures and builder patterns
- How much Stage 2/3 cleanup to include vs defer

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

## Summary

Phase 35 is a pure internal refactoring. The goal is to extract an `OutboundSyncPlanner` domain service that consolidates sync eligibility decisions currently scattered across three stages inside `on_clipboard_changed()` in `uc-tauri/src/bootstrap/runtime.rs` (lines 1140-1362). The planner produces an `OutboundSyncPlan` value that the runtime dispatches without further logic.

Currently the runtime performs: (1) settings load + file path extraction + max_file_size filtering, (2) `all_files_excluded` guard to skip clipboard sync when all files are oversized, and (3) spawning clipboard and file sync tasks with the computed inputs. All three stages make independent policy decisions. This creates the class of bugs where copying an oversized file can still sync a thumbnail to peers.

After this phase, `OutboundSyncPlanner::plan()` owns all eligibility decisions atomically. The runtime becomes a thin dispatcher: call `plan()`, then spawn clipboard sync if `plan.clipboard.is_some()`, then spawn file sync for each entry in `plan.files`. The redundant `file_sync_enabled` guard in `SyncOutboundFileUseCase::execute()` and the `max_file_size` re-check can be removed since the plan already guarantees correctness.

**Primary recommendation:** Define `OutboundSyncPlanner` as a plain struct (no trait) in `uc-app/src/usecases/sync_planner.rs` with a single `plan()` method. Make the method async (takes `Arc<dyn SettingsPort>` as a dependency, loads settings internally) so callers need no knowledge of settings loading. This follows the existing use case constructor pattern.

## Standard Stack

### Core

| Library     | Version | Purpose                                     | Why Standard      |
| ----------- | ------- | ------------------------------------------- | ----------------- |
| tokio       | 1       | Async runtime for settings load in `plan()` | Already in uc-app |
| async-trait | 0.1     | Port traits                                 | Already in uc-app |
| uuid        | 1.10    | transfer_id generation                      | Already in uc-app |
| anyhow      | 1.0     | Error propagation                           | Already in uc-app |
| tracing     | 0.1     | Structured logging inside planner           | Already in uc-app |

No new dependencies are required. This phase adds no Cargo.toml changes.

## Architecture Patterns

### Recommended Project Structure

New files to add under `uc-app`:

```
src-tauri/crates/uc-app/src/usecases/
├── sync_planner/
│   ├── mod.rs           # pub use, module declaration
│   ├── planner.rs       # OutboundSyncPlanner struct + plan() impl
│   └── types.rs         # OutboundSyncPlan, ClipboardSyncIntent, FileSyncIntent
```

Alternative (acceptable if single file suffices): `sync_planner.rs` flat file. The multi-file layout is preferred if tests are substantial.

### Pattern 1: Plan types as plain data (no trait)

`OutboundSyncPlan` is a pure value type — no async, no ports. It is the output of planning and the input to dispatching.

```rust
// src-tauri/crates/uc-app/src/usecases/sync_planner/types.rs
use std::path::PathBuf;
use uc_core::SystemClipboardSnapshot;
use uc_core::network::protocol::FileTransferMapping;

/// A single file eligible for outbound transfer.
pub struct FileSyncIntent {
    pub path: PathBuf,
    pub transfer_id: String,
    pub filename: String,
}

/// Clipboard sync is eligible; carries snapshot and file transfer mappings.
pub struct ClipboardSyncIntent {
    pub snapshot: SystemClipboardSnapshot,
    pub file_transfers: Vec<FileTransferMapping>,
}

/// The output of OutboundSyncPlanner::plan(). Drives runtime dispatch.
pub struct OutboundSyncPlan {
    /// None means: skip clipboard sync entirely.
    pub clipboard: Option<ClipboardSyncIntent>,
    /// Empty means: no files to transfer.
    pub files: Vec<FileSyncIntent>,
}
```

### Pattern 2: Planner as a struct with Arc<dyn Port> constructor

Follows the established uc-app use case pattern (Arc<dyn Port> injected at construction).

```rust
// src-tauri/crates/uc-app/src/usecases/sync_planner/planner.rs
use std::sync::Arc;
use uc_core::ports::SettingsPort;
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};

pub struct OutboundSyncPlanner {
    settings: Arc<dyn SettingsPort>,
}

impl OutboundSyncPlanner {
    pub fn new(settings: Arc<dyn SettingsPort>) -> Self {
        Self { settings }
    }

    pub async fn plan(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
    ) -> OutboundSyncPlan {
        // 1. Load settings once
        // 2. Check origin (LocalCapture only for file sync)
        // 3. Check file_sync_enabled
        // 4. extract_file_paths_from_snapshot()
        // 5. Filter by max_file_size, generate transfer_ids
        // 6. Compute all_files_excluded guard
        // 7. Return OutboundSyncPlan
    }
}
```

### Pattern 3: Runtime becomes thin dispatcher

After adding the planner, `on_clipboard_changed()` changes from 3 policy stages to:

```rust
// In runtime.rs on_clipboard_changed()
let plan = OutboundSyncPlanner::new(self.deps.settings.clone())
    .plan(outbound_snapshot, origin)
    .await;

if let Some(clipboard_intent) = plan.clipboard {
    // spawn clipboard sync task
}
for file_intent in plan.files {
    // spawn file sync task
}
```

### Pattern 4: Settings load via SettingsPort (async)

The planner must be async because `SettingsPort::load()` is async. The decision context specifies that `plan()` loads settings internally — not the caller. This avoids leaking settings types into the runtime dispatch layer.

```rust
// SettingsPort is defined in uc-core/src/ports
// Existing pattern — settings loaded per-request (not cached)
// Per [Phase 24] decision: "Settings loaded from storage each time (not cached) -- SQLite + WAL fast for 2-5 devices"
let settings = self.settings.load().await;
```

### Pattern 5: extract_file_paths_from_snapshot moves into planner

Currently `extract_file_paths_from_snapshot()` is a private function in `uc-tauri/src/bootstrap/runtime.rs`. The planner needs this logic. Options:

- **Option A (recommended):** Move the function to `uc-app/src/usecases/sync_planner/planner.rs` as a private helper. This is a pure function (no I/O) that belongs in the planning layer.
- **Option B:** Keep it in runtime.rs and pass extracted paths into `plan()`. This leaks implementation detail into the caller — not recommended.

Note: `resolve_apfs_file_reference()` is macOS-specific (uses CoreFoundation) and lives in uc-tauri. The planner in uc-app cannot depend on uc-tauri. Resolution: pass already-resolved paths into the planner, or keep the extraction in the runtime and pass raw paths. The cleaner approach is to keep APFS resolution in uc-tauri and have the planner accept pre-resolved paths, OR move the resolver to uc-core (it has no Tauri dep). Given the locked decision says `plan(snapshot, origin, settings)`, the snapshot-to-path extraction must happen inside `plan()`. This means the APFS resolution must be handled before the snapshot reaches the planner, or the planner accepts a callback/resolver.

**Recommended resolution:** The planner extracts paths from the snapshot (pure URI parsing, no APFS), and APFS resolution happens in uc-tauri before the planner is called. On macOS, runtime.rs pre-resolves APFS references on the snapshot before passing to `plan()`. However, inspecting the code: `extract_file_paths_from_snapshot` already calls `resolve_apfs_file_reference` inside the loop, so moving it to uc-app would require also moving `resolve_apfs_file_reference`. Since uc-app cannot use CoreFoundation, the simpler design is:

- Keep APFS resolution in uc-tauri/runtime.rs
- The planner takes a `Vec<PathBuf>` of pre-extracted paths instead of re-extracting from the snapshot, OR
- Provide the extraction as a pure function that lives in uc-tauri and is passed as a closure, OR
- **Simplest:** Move `extract_file_paths_from_snapshot` (without APFS logic) to uc-app, and inject the APFS-resolved paths separately

Given the CONTEXT.md signature is `plan(snapshot, origin, settings)`, the planner should extract paths internally. The cleanest solution within uc-app: `plan()` extracts paths from the snapshot using pure URI parsing (no APFS), and uc-tauri runtime applies APFS resolution as a pre-processing step on the snapshot representations before calling the planner. This is already implicit in the current flow since the snapshot is what uc-tauri receives from the clipboard watcher.

### Anti-Patterns to Avoid

- **Planner as trait with mock impl:** The planner is not an external dependency — it is pure domain logic. No trait needed; test it directly.
- **Settings loaded by caller and passed as value:** Leaks settings type into dispatch layer; keeps settings coupling in runtime.rs.
- **file_transfers computed in runtime after plan():** Defeats the purpose; transfer_ids must be generated inside the planner.

## Don't Hand-Roll

| Problem                     | Don't Build                 | Use Instead                                                         | Why                                                                                     |
| --------------------------- | --------------------------- | ------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| UUID generation             | Custom ID generator         | `uuid::Uuid::new_v4().to_string()`                                  | Already in uc-app                                                                       |
| File metadata check         | Custom fs wrapper           | `std::fs::metadata()`                                               | Existing pattern in runtime.rs                                                          |
| Content type classification | Re-implement                | `classify_snapshot()` from `uc_core::settings::content_type_filter` | Already correct, tested                                                                 |
| Sync policy filtering       | Re-implement per-peer logic | `apply_sync_policy()` / `apply_file_sync_policy()`                  | Used indirectly — planner handles pre-conditions; per-peer filtering stays in use cases |

**Key insight:** The planner handles pre-conditions (settings load, origin check, file extraction, size filtering, all_files_excluded guard). Per-peer filtering remains in `SyncOutboundClipboardUseCase::apply_sync_policy()` and `apply_file_sync_policy()`. The planner does NOT replace per-peer filtering — it replaces runtime-level policy decisions that happen before the use cases are even invoked.

## Common Pitfalls

### Pitfall 1: APFS path resolution scope

**What goes wrong:** Moving `extract_file_paths_from_snapshot` to uc-app introduces a CoreFoundation dependency (macOS-only) that cannot compile in a platform-independent crate.
**Why it happens:** `resolve_apfs_file_reference` uses `core_foundation` crate which is uc-tauri-level (platform-specific).
**How to avoid:** Keep APFS resolution in uc-tauri. Either: (a) pre-resolve in runtime before calling the planner, or (b) move pure URI extraction to uc-app and apply APFS resolution as a post-processing step in runtime. Option (b) matches the current code structure best.
**Warning signs:** `core_foundation` appearing as a dependency in uc-app `Cargo.toml`.

### Pitfall 2: Double settings load

**What goes wrong:** The planner loads settings, but `SyncOutboundFileUseCase::execute()` also loads settings (for `file_sync_enabled` and `max_file_size` guards). After the planner guarantees these conditions, the use case redundant loads still run.
**Why it happens:** Guards in use cases are defensive — they were correct before the planner existed.
**How to avoid:** After the planner is proven correct, remove the `file_sync_enabled` and `max_file_size` guards from `SyncOutboundFileUseCase::execute()`. The CONTEXT.md explicitly calls this out as within-phase scope.
**Warning signs:** Settings loaded 3+ times per clipboard change event.

### Pitfall 3: plan() signature mismatch with CONTEXT.md

**What goes wrong:** Implementing `plan(snapshot, origin, settings)` as taking a pre-loaded `Settings` value vs. taking `Arc<dyn SettingsPort>` as a constructor dependency.
**Why it happens:** Both are valid; the CONTEXT.md says "Claude's Discretion" for this.
**How to avoid:** Taking `Arc<dyn SettingsPort>` in the constructor is the established pattern in this codebase and is unit-testable with `MockSettings`. Using pre-loaded settings as a parameter works too but requires the caller to load settings first. Prefer constructor injection.

### Pitfall 4: plan() returning Result vs infallible

**What goes wrong:** Making `plan()` return `Result<OutboundSyncPlan>` causes callers to handle errors. But the current runtime code uses `unwrap_or` defaults when settings fail — the safe fallback is to proceed with sync (not fail hard).
**Why it happens:** Settings load is async and fallible.
**How to avoid:** `plan()` should be infallible: on settings load failure, log a warning and return the safe default plan (clipboard sync allowed, no file sync). This matches the existing `unwrap_or(true)` pattern for `file_sync_enabled`. Return `OutboundSyncPlan` not `Result<OutboundSyncPlan>`.

### Pitfall 5: Forgetting all_files_excluded guard semantic

**What goes wrong:** Planner generates `files: []` but still sets `clipboard: Some(...)`, allowing clipboard sync of a file thumbnail/icon when all files were excluded by size.
**Why it happens:** The guard must check: was there a file clipboard AND were all files excluded? Not just "is files empty?"
**How to avoid:** The guard is: `let all_files_excluded = !raw_file_paths.is_empty() && file_sync_entries.is_empty()`. If `all_files_excluded`, set `clipboard: None`.

## Code Examples

Verified patterns from existing codebase:

### File path extraction (current implementation in runtime.rs)

```rust
// Source: uc-tauri/src/bootstrap/runtime.rs lines 1413-1457
fn extract_file_paths_from_snapshot(snapshot: &SystemClipboardSnapshot) -> Vec<PathBuf> {
    // Looks for text/uri-list or file/uri-list MIME types, or "files"/"public.file-url" format IDs
    // Parses file:// URIs into PathBufs
    // Deduplicates and sorts
    // On macOS: calls resolve_apfs_file_reference() for /.file/id=... paths
}
```

### Settings load with fallback (existing pattern)

```rust
// Source: runtime.rs line 1214-1222
let settings_snapshot = self.deps.settings.load().await;
let file_sync_enabled = settings_snapshot
    .as_ref()
    .map(|s| s.file_sync.file_sync_enabled)
    .unwrap_or(true); // safe default: proceed
let max_file_size = settings_snapshot
    .as_ref()
    .map(|s| s.file_sync.max_file_size)
    .unwrap_or(u64::MAX); // safe default: no limit
```

### Mock pattern for unit tests (from sync_policy.rs tests)

```rust
// Source: uc-app/src/usecases/file_sync/sync_policy.rs tests
struct MockSettings {
    settings: Option<Settings>,
}
#[async_trait::async_trait]
impl SettingsPort for MockSettings {
    async fn load(&self) -> anyhow::Result<Settings> {
        match &self.settings {
            Some(s) => Ok(s.clone()),
            None => Err(anyhow::anyhow!("settings load error")),
        }
    }
    async fn save(&self, _settings: &Settings) -> anyhow::Result<()> { Ok(()) }
}
```

### All-files-excluded guard (current implementation)

```rust
// Source: runtime.rs lines 1273-1280
let all_files_excluded =
    !file_paths_for_sync.is_empty() && file_sync_entries.is_empty();

if all_files_excluded {
    tracing::info!(
        excluded_file_count = file_paths_for_sync.len(),
        "Skipping outbound clipboard sync: all files excluded by size limit"
    );
}
```

### File size filtering with transfer_id generation (current implementation)

```rust
// Source: runtime.rs lines 1235-1267
let file_sync_entries: Vec<(PathBuf, String, String)> = file_paths_for_sync
    .iter()
    .filter_map(|path| {
        match std::fs::metadata(path) {
            Ok(meta) if meta.len() > max_file_size => {
                tracing::warn!(..., "Excluding file from sync: exceeds max_file_size");
                None
            }
            Ok(_) => {
                let transfer_id = uuid::Uuid::new_v4().to_string();
                let filename = path.file_name()...;
                Some((path.clone(), transfer_id, filename))
            }
            Err(e) => {
                tracing::warn!(..., "Excluding file from sync: failed to read metadata");
                None
            }
        }
    })
    .collect();
```

## State of the Art

| Old Approach                             | Current Approach                                     | When Changed                  | Impact                                             |
| ---------------------------------------- | ---------------------------------------------------- | ----------------------------- | -------------------------------------------------- |
| Settings loaded once per clipboard event | Settings loaded multiple times (runtime + use cases) | Issue #279 motivated Phase 35 | Double load per event; inconsistent policy surface |
| Policy checks scattered across 3 stages  | Single `OutboundSyncPlan` produced atomically        | Phase 35                      | Eliminates thumbnail-sync bug class                |

**Redundant checks to remove in this phase:**

- `file_sync_enabled` guard in `SyncOutboundFileUseCase::execute()` (lines 54-67): The plan guarantees this is only called when file sync is enabled.
- `max_file_size` guard in `SyncOutboundFileUseCase::execute()` (lines 103-122): The plan guarantees files have already been filtered.

## Open Questions

1. **APFS resolution placement**
   - What we know: `resolve_apfs_file_reference` uses `core_foundation` (macOS-only); uc-app must be platform-independent.
   - What's unclear: Whether to keep extraction entirely in uc-tauri (passing `Vec<PathBuf>` to planner) or split extraction (pure URI parsing in uc-app, APFS resolution wrapper in uc-tauri).
   - Recommendation: Keep `extract_file_paths_from_snapshot` in uc-tauri (it already works), move only the non-platform logic (size filtering, transfer_id generation, all_files_excluded guard) into the planner. The planner's `plan()` signature could accept `pre_extracted_paths: Vec<PathBuf>` as a parameter alongside `snapshot` and `origin`. This is clean and testable.

2. **Whether to export planner through uc-app public API**
   - What we know: `uc-app/src/usecases/mod.rs` has pub use statements for each use case.
   - What's unclear: Whether `OutboundSyncPlanner` needs to be pub-exported or stays internal to uc-tauri bootstrap wiring.
   - Recommendation: Export through `pub use` for testability from uc-tauri integration tests; keep types pub.

## Validation Architecture

### Test Framework

| Property           | Value                                                       |
| ------------------ | ----------------------------------------------------------- |
| Framework          | Rust `#[tokio::test]` (tokio 1, already in uc-app dev-deps) |
| Config file        | none — inline `#[cfg(test)]` modules per-file               |
| Quick run command  | `cd src-tauri && cargo test -p uc-app sync_planner 2>&1`    |
| Full suite command | `cd src-tauri && cargo test -p uc-app 2>&1`                 |

### Phase Requirements → Test Map

This phase has no formal requirement IDs. The behavioral properties to test are:

| Behavior                                                          | Test Type   | Automated Command                                      |
| ----------------------------------------------------------------- | ----------- | ------------------------------------------------------ |
| `plan()` returns `clipboard: None` when `origin == RemotePush`    | unit        | `cargo test -p uc-app sync_planner -- --test-thread=1` |
| `plan()` returns `files: []` when `file_sync_enabled == false`    | unit        | same                                                   |
| `plan()` returns `files: []` when `origin != LocalCapture`        | unit        | same                                                   |
| `plan()` excludes files exceeding `max_file_size`                 | unit        | same                                                   |
| `plan()` returns `clipboard: None` when all files excluded        | unit        | same                                                   |
| `plan()` returns both clipboard and files when mixed sizes        | unit        | same                                                   |
| `plan()` proceeds safely when settings load fails                 | unit        | same                                                   |
| Runtime dispatches clipboard sync iff `plan.clipboard.is_some()`  | integration | `cargo test -p uc-tauri`                               |
| `SyncOutboundFileUseCase` no longer re-checks `file_sync_enabled` | review      | manual code review                                     |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-app sync_planner`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-app && cargo test -p uc-tauri`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-app/src/usecases/sync_planner/` — module does not exist yet, created in Wave 0
- [ ] Test file at `src-tauri/crates/uc-app/src/usecases/sync_planner/planner.rs` inline `#[cfg(test)]` — created with implementation

_(Framework already installed; no new dev-dependencies required)_

## Sources

### Primary (HIGH confidence)

- Direct code inspection: `uc-tauri/src/bootstrap/runtime.rs` lines 1140-1457 — full `on_clipboard_changed()` impl and `extract_file_paths_from_snapshot()`
- Direct code inspection: `uc-app/src/usecases/clipboard/sync_outbound.rs` — `SyncOutboundClipboardUseCase::apply_sync_policy()` and `execute()`
- Direct code inspection: `uc-app/src/usecases/file_sync/sync_outbound.rs` — `SyncOutboundFileUseCase::execute()` with redundant guards
- Direct code inspection: `uc-app/src/usecases/file_sync/sync_policy.rs` — `apply_file_sync_policy()`
- Direct code inspection: `uc-core/src/settings/content_type_filter.rs` — `classify_snapshot()`, `is_content_type_allowed()`

### Secondary (MEDIUM confidence)

- CONTEXT.md Phase 35 — locked design decisions from prior discussion session
- STATE.md accumulated decisions — established patterns (e.g., Phase 25 classify-once-before-loop, Phase 24 settings-not-cached)

### Tertiary (LOW confidence)

- None

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — no new dependencies; all existing crates identified
- Architecture: HIGH — code directly inspected; patterns match existing use cases
- Pitfalls: HIGH — APFS issue is verified (CoreFoundation dep confirmed in runtime.rs); all other pitfalls observed in actual code

**Research date:** 2026-03-16
**Valid until:** 2026-04-16 (stable internal codebase; low churn risk)
