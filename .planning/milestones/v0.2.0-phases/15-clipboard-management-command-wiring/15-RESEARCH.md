# Phase 15: Clipboard Management Command Wiring - Research

**Researched:** 2026-03-07
**Domain:** Tauri clipboard command wiring (Rust) + React frontend API contracts
**Confidence:** HIGH

## User Constraints (from CONTEXT.md)

### Locked Decisions

- No 15-CONTEXT.md found; default to global project decisions in `.planning/STATE.md` and `.planning/REQUIREMENTS.md`.
- v0.2.0 scope is architecture hardening and gap closure, not new UX flows.
- Clipboard boundary rules from Phases 10–13 remain in force (use cases only, no direct runtime deps in commands).
- CONTRACT-03 is explicitly mapped to Phase 15 and must be satisfied here.

### Claude's Discretion

- Fill in the missing backend clipboard management commands (stats/items/favorites) required by the existing frontend API surface.
- Propose DTO shapes and serialization that keep the current frontend TypeScript types working with minimal or no changes.
- Recommend how to organize tests to cover command payload contracts.

### Deferred Ideas (OUT OF SCOPE)

- Any new clipboard UX (search, advanced favorites UI, quick-paste overlays) beyond wiring what already exists in the frontend API.
- Cross‑internet sync / file sync expansions (kept out of v0.2.0 per REQUIREMENTS.md).

<phase_requirements>

## Phase Requirements

| ID          | Description                                                                                                                           | Research Support                                                                                                                                                                                                           |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CONTRACT-03 | Command/event payload serialization remains frontend-compatible (camelCase where required) with tests covering key payload contracts. | This research identifies existing clipboard DTOs, frontend API expectations, and recommends concrete command + DTO patterns and tests to guarantee the clipboard management flow (stats/items/favorites) stays compatible. |

</phase_requirements>

## Summary

Phase 15 focuses on closing the remaining clipboard management wiring gaps between the React frontend (`src/api/clipboardItems.ts`) and the new `uc-tauri` command layer, in order to satisfy CONTRACT-03 for clipboard flows. The codebase already has a rich set of clipboard use cases in `uc-app` (capture, list, restore, touch, projections, payload resolution) and several wired Tauri commands in `uc-tauri` (`get_clipboard_entries`, `delete_clipboard_entry`, `get_clipboard_entry_detail`, `get_clipboard_entry_resource`, `sync_clipboard_items`, `restore_clipboard_entry`). The frontend, however, exposes additional higher-level APIs for clipboard stats and favoriting that currently have no backend implementation.

The standard stack for this phase is the existing Tauri 2 + uc-tauri command pattern: commands accept simple, JSON‑serializable inputs, delegate to `runtime.usecases()` accessors, and return DTOs defined in `uc-tauri::models` with explicit serde configuration. On the frontend, all calls go through `invokeWithTrace`, and `clipboardItems.ts` defines the canonical TypeScript representations for clipboard entries, stats, and favorite toggling. Planning this phase well means: (1) enumerating exactly which APIs are already wired vs missing, (2) understanding how clipboard projections/entries are modeled in uc-core/uc-app, and (3) defining precise DTO + serialization rules and tests so that the React code does not have to change.

**Primary recommendation:** Implement missing clipboard stats and favorite commands in `uc-tauri` using existing uc-app ports/use cases, design DTOs to match `ClipboardStats` and favorite toggling signatures in `src/api/clipboardItems.ts`, and add focused Rust + Vitest tests that lock down the JSON contract for these clipboard management endpoints.

## Standard Stack

### Core

| Library / Module                       | Version / Origin        | Purpose                                                | Why Standard                                                                                           |
| -------------------------------------- | ----------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| Tauri 2 (`tauri`, `#[tauri::command]`) | `src-tauri/Cargo.toml`  | Desktop IPC and command definition layer               | Already used for all backend commands; required by project architecture.                               |
| `uc-tauri` crate                       | workspace member        | Command wiring + DTOs + error handling                 | New command layer for v0.2.0; clipboard commands already live here.                                    |
| `uc-app` clipboard use cases           | workspace member        | Business logic for capture, list, delete, restore, etc | Commands must delegate via `runtime.usecases()` per Phase 10 decisions.                                |
| `uc-core` clipboard domain + ports     | workspace member        | Domain models (entries, representations, selections)   | All clipboard operations ultimately operate on these types.                                            |
| `serde` / `serde_json`                 | Rust deps               | Serialization of DTOs for Tauri responses              | Existing DTOs (ClipboardEntryProjection, ClipboardEntriesResponse, LifecycleStatusDto) already use it. |
| React 18 + TypeScript                  | `package.json`          | Frontend UI and type system                            | Existing API file `src/api/clipboardItems.ts` is the contract source of truth.                         |
| `invokeWithTrace` wrapper              | `src/lib/tauri-command` | Frontend Tauri call wrapper with observability         | All clipboard API functions already invoke commands via this wrapper.                                  |

### Supporting

| Library / Module | Version / Origin | Purpose | When to Use |
| `uc-tauri::models::ClipboardEntryProjection` & `ClipboardEntriesResponse` | `src-tauri/crates/uc-tauri/src/models/mod.rs` | Projected list view of clipboard entries | For `get_clipboard_entries` responses; must remain snake_case. |
| `uc-tauri::models::ClipboardEntryDetail` & `ClipboardEntryResource` | same as above | Detailed entry content & resource metadata | For `get_clipboard_entry_detail` and `get_clipboard_entry_resource` commands. |
| `uc-app::usecases::clipboard::CaptureClipboardUseCase` | `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs` | Capture clipboard snapshots into events/entries | Already integrated; relevant for understanding stats and favorites fields. |
| `uc-app::usecases::clipboard::RestoreClipboardSelectionUseCase` | `src-tauri/crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs` | Build snapshots and write them back to system clipboard | Used by `restore_clipboard_entry` Tauri command. |
| `uc-app::usecases::TouchClipboardEntryUseCase` | `src-tauri/crates/uc-app/src/usecases/clipboard/touch_clipboard_entry.rs` | Update entry `active_time` based on a clock port | Called in `restore_clipboard_entry_impl` to mark last use time. |
| `uc-app::usecases::clipboard::list_entry_projections::ListClipboardEntryProjections` | `src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs` | Generate entry projections for list responses | Backing UC for `get_clipboard_entries` with thumbnail + preview + content_type. |
| Vitest | `package.json` | Frontend unit tests for API mapping | Already used in `src/api/__tests__/clipboardItems.test.ts`. |
| Rust tests in uc-tauri | `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` | Verify command helpers and behaviors (e.g. not_ready logic, restore flows) | Extend to cover new commands and DTO serialization. |

### Alternatives Considered

Because Phase 15 is a gap-closure phase, not a redesign, alternatives should not be adopted:

| Instead of                     | Could Use                           | Tradeoff                                                                                               |
| ------------------------------ | ----------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `uc-tauri` command wiring      | Legacy `src-tauri/src/api` commands | Legacy layer is being retired; re‑adding clipboard wiring there would violate boundary refactor goals. |
| `ClipboardEntriesResponse` DTO | Directly serializing domain models  | Breaks CONTRACT-01/02/03 separation and risks leaking internal fields/shape changes to the UI.         |
| Custom HTTP/REST endpoints     | Tauri IPC + commands                | Adds extra server surface and complexity with no user benefit for v0.2.0.                              |

**Installation:**（仅供参考，当前项目已配置完成）

```bash
# 前端单测
bun test

# 后端覆盖率（含命令层）
bun run test:coverage   # cd src-tauri && cargo llvm-cov --html --workspace
```

## Architecture Patterns

### Recommended Project Structure（本阶段相关子集）

```
src-tauri/
├── crates/
│   ├── uc-core/           # Clipboard domain models & ports
│   ├── uc-app/            # Clipboard use cases (capture, list, restore, touch)
│   ├── uc-platform/       # Platform adapters, clipboard watcher
│   └── uc-tauri/
│       ├── src/commands/  # Tauri command functions (clipboard.rs, ...)
│       └── src/models/    # API DTOs (ClipboardEntryProjection, ...)
└── src/main.rs            # Registers commands via invoke_handler!

src/
├── api/clipboardItems.ts  # Frontend clipboard API surface
└── api/__tests__/...      # Vitest tests for API mapping
```

### Pattern 1: Command → UseCase → DTO

**What:** Each clipboard management operation is implemented as a Tauri command function that:

- Accepts only primitives / simple structs from the frontend.
- Uses `runtime.usecases().xxx()` to get the relevant uc-app use case.
- Converts domain/use case DTOs into uc-tauri `models` structs.
- Returns `Result<Dto, CommandError>` (or `Result<(), CommandError>` for void) with JSON-serializable DTOs.

**When to use:** For new clipboard stats and favorites commands, and for any missing item-level command where the frontend already has a function in `clipboardItems.ts`.

**Example:** `get_clipboard_entries` (existing, canonical pattern)

```rust
// Source: src-tauri/crates/uc-tauri/src/commands/clipboard.rs:21
#[tauri::command]
pub async fn get_clipboard_entries(
    runtime: State<'_, Arc<AppRuntime>>,
    limit: Option<usize>,
    offset: Option<usize>,
    _trace: Option<TraceMetadata>,
) -> Result<ClipboardEntriesResponse, CommandError> {
    let resolved_limit = limit.unwrap_or(50);
    let resolved_offset = offset.unwrap_or(0);
    let device_id = runtime.device_id();

    let span = info_span!(
        "command.clipboard.get_entries",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        device_id = %device_id,
        limit = resolved_limit,
        offset = resolved_offset,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let encryption_state = runtime.encryption_state().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to check encryption state");
            CommandError::InternalError(format!("Failed to check encryption state: {}", e))
        })?;

        let session_ready = runtime.is_encryption_ready().await;
        if should_return_not_ready(encryption_state, session_ready) {
            tracing::warn!("Encryption initialized but session not ready yet, returning not-ready response.");
            return Ok(ClipboardEntriesResponse::NotReady);
        }

        let uc = runtime.usecases().list_entry_projections();
        let dtos = uc.execute(resolved_limit, resolved_offset).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to get clipboard entry projections");
            CommandError::InternalError(e.to_string())
        })?;

        let projections: Vec<ClipboardEntryProjection> = dtos
            .into_iter()
            .map(|dto| ClipboardEntryProjection {
                id: dto.id,
                preview: dto.preview,
                has_detail: dto.has_detail,
                size_bytes: dto.size_bytes,
                captured_at: dto.captured_at,
                content_type: dto.content_type,
                thumbnail_url: dto.thumbnail_url,
                is_encrypted: dto.is_encrypted,
                is_favorited: dto.is_favorited,
                updated_at: dto.updated_at,
                active_time: dto.active_time,
            })
            .collect();

        Ok(ClipboardEntriesResponse::Ready { entries: projections })
    }
    .instrument(span)
    .await
}
```

### Pattern 2: UseCase Composition for Restore

**What:** Higher-level commands may need to orchestrate multiple use cases. `restore_clipboard_entry` is the model: it builds a clipboard snapshot from history, updates the entry’s `active_time`, writes the snapshot to the system clipboard, and then triggers outbound sync and a frontend event.

**When to use:** For any management command that affects multiple domain concerns (e.g., restore + stats updates). Stats itself should remain a pure query; favorites will typically be a single use case call but may later connect with sync or telemetry.

**Example:** `restore_clipboard_entry_impl` orchestration

```rust
// Source: src-tauri/crates/uc-tauri/src/commands/clipboard.rs:315
async fn restore_clipboard_entry_impl(
    runtime: &AppRuntime,
    entry_id: String,
    trace: Option<TraceMetadata>,
) -> Result<bool, CommandError> {
    let span = info_span!(
        "command.clipboard.restore_entry",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        entry_id = %entry_id,
    );
    record_trace_fields(&span, &trace);

    async move {
        let parsed_id = EntryId::from(entry_id.clone());

        let restore_uc = runtime.usecases().restore_clipboard_selection();
        let snapshot = restore_uc.build_snapshot(&parsed_id).await.map_err(|e| {
            tracing::error!(error = %e, entry_id = %entry_id, "Failed to build restore snapshot");
            CommandError::InternalError(e.to_string())
        })?;

        let touch_uc = runtime.usecases().touch_clipboard_entry();
        let touched = touch_uc.execute(&parsed_id).await.map_err(|e| {
            tracing::error!(error = %e, entry_id = %entry_id, "Failed to update entry active time");
            CommandError::InternalError(e.to_string())
        })?;

        if !touched {
            tracing::warn!(entry_id = %entry_id, "Entry not found when touching active time");
            return Err(CommandError::NotFound("Entry not found".to_string()));
        }

        let outbound_snapshot = snapshot.clone();
        restore_uc.restore_snapshot(snapshot).await.map_err(|err| {
            tracing::error!(error = %err, entry_id = %entry_id, "Failed to write restore snapshot");
            CommandError::InternalError(err.to_string())
        })?;

        let outbound_sync_uc = runtime.usecases().sync_outbound_clipboard();
        match tokio::task::spawn_blocking(move || {
            outbound_sync_uc.execute(outbound_snapshot, uc_core::ClipboardChangeOrigin::LocalRestore)
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                tracing::warn!(error = %err, entry_id = %entry_id, "Restore outbound sync failed");
            }
            Err(err) => {
                tracing::warn!(error = %err, entry_id = %entry_id, "Restore outbound sync task join failed");
            }
        }

        Ok(true)
    }
    .instrument(span)
    .await
}
```

### Anti-Patterns to Avoid

- **Bypassing use cases from commands:** Directly accessing repositories or ports from `uc-tauri` violates the Phase 10 boundary decisions. All clipboard management must be done via `runtime.usecases()` accessors.
- **Returning domain models directly to the frontend:** This breaks the DTO layering (`uc-core` → `uc-app` DTOs → `uc-tauri` DTOs → TypeScript interfaces) and jeopardizes CONTRACT-01/03.
- **Ad-hoc camelCase conversion in Rust:** DTOs for clipboard management already use field naming conventions (`snake_case` vs `camelCase`) driven by existing TypeScript expectations. Do not introduce manual renaming; use serde attributes on DTOs instead.
- **Duplicating business logic in commands:** Logic such as `has_detail` or image detection must stay in uc-app use cases (`ListClipboardEntryProjections`) or in the domain, not reimplemented in multiple layers.

## Don't Hand-Roll

| Problem                                          | Don't Build                                     | Use Instead                                                                      | Why                                                                                                               |
| ------------------------------------------------ | ----------------------------------------------- | -------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Clipboard entry listing with previews/thumbnails | Custom SQL / ad-hoc projection code in commands | `ListClipboardEntryProjections` use case + `ClipboardEntryProjection` DTO        | Existing use case already handles selection, MIME, thumbnails, and edge cases (missing selection/rep) with tests. |
| System clipboard restore                         | Direct platform writes from commands            | `RestoreClipboardSelectionUseCase` + `ClipboardIntegrationMode`                  | Use case handles origin tracking, passive mode, snapshot building, and error behavior.                            |
| Entry `active_time` update                       | Inline DB updates in commands                   | `TouchClipboardEntryUseCase` via `runtime.usecases().touch_clipboard_entry()`    | Centralizes clock usage and repository semantics; already tested.                                                 |
| Clipboard stats aggregation                      | Raw SQL in uc-tauri or frontend aggregation     | A dedicated `uc-app` use case over `ClipboardEntryRepositoryPort`                | Domain is responsible for counts/sizes; wiring phase should only call/use DTOs.                                   |
| Favorite status toggling                         | Manual flag mutations from frontend             | A `ToggleFavoriteClipboardEntry` use case (or equivalent) using repository ports | Keeps business rules (e.g., audit, sync triggers) in the app layer.                                               |

**Key insight:** Clipboard management involves multiple cross-cutting concerns (encryption readiness, selection policies, thumbnails, passive mode, sync). Implementing them ad hoc in commands or frontend logic risks violating boundaries and duplicating complex rules. Whenever something feels like “just a flag” (favorites, stats), verify if a port/use case already exists or should exist in uc-app, and then wire only that in Phase 15.

## Common Pitfalls

### Pitfall 1: Breaking TypeScript Contracts via Serde Defaults

**What goes wrong:** Adjusting Rust structs or enums in `uc-tauri::models` without aligning serialization with existing `clipboardItems.ts` types, leading to runtime errors or subtle UI bugs (e.g., fields renamed to camelCase unexpectedly).

**Why it happens:** Rust developers rely on default serde behavior, but the frontend types already assume:

- `ClipboardEntriesResponse` is tagged enum with `status: 'ready' | 'not_ready'`.
- Entry projections use `snake_case` (e.g., `has_detail`, `size_bytes`, `content_type`, `thumbnail_url`).
- `ClipboardItemsResult` rewraps `entries` as `items` with transformed structure.

**How to avoid:**

- Keep DTO field names stable in `src-tauri/crates/uc-tauri/src/models/mod.rs`.
- For new DTOs (e.g., `ClipboardStats`), pick snake_case names and mirror them exactly in TypeScript.
- Add Rust tests that serialize DTOs and assert on JSON (as already done for `ClipboardEntriesResponse` and `LifecycleStatusDto`).

**Warning signs:**

- Frontend tests in `src/api/__tests__/clipboardItems.test.ts` start failing due to shape mismatch.
- Console logs showing `undefined` for fields like `thumbnail_url` or `has_detail` that should exist.

### Pitfall 2: Ignoring Encryption Session Readiness

**What goes wrong:** Commands try to query or decrypt clipboard data before encryption state/session is ready (e.g., immediately on startup), returning confusing errors or partial results.

**Why it happens:** The command layer must consider `EncryptionState` and `is_encryption_ready()`—this is already implemented for `get_clipboard_entries` via `should_return_not_ready`, but new commands might forget this behavior.

**How to avoid:**

- For commands that rely on decrypted clipboard content (list, detail, stats), reuse the `should_return_not_ready` logic and pattern from `get_clipboard_entries`.
- For operations that only touch metadata not requiring decryption, be explicit in docs/comments that they do not depend on encryption readiness.

**Warning signs:**

- Frequent `Failed to check encryption state` logs or unexpected `InternalError` responses during startup.
- Frontend sees `status: 'ready'` but underlying data still cannot be decrypted.

### Pitfall 3: Passive Clipboard Mode Violations

**What goes wrong:** Commands attempt to sync or write to the system clipboard when `UC_CLIPBOARD_MODE=passive`, violating user expectations and tests around passive mode.

**Why it happens:** The clipboard integration mode is surfaced via `ClipboardIntegrationMode` in uc-app and checked in some commands (`sync_clipboard_items_impl`), but not all potential write/sync paths.

**How to avoid:**

- For any new command that initiates clipboard sync or writes, gate behavior based on `runtime.clipboard_integration_mode()` exactly as `sync_clipboard_items_impl` does.
- For read-only management commands (stats, list, favorites), ensure they do not write to the system clipboard or start watchers, so passive mode is naturally respected.

**Warning signs:**

- Tests like `sync_clipboard_items_returns_error_in_passive_mode` fail.
- Users report clipboard changes despite passive mode.

### Pitfall 4: Skipping `touch` Updates on Restore Paths

**What goes wrong:** Restoring an entry to the system clipboard does not update its `active_time`, causing sorting by recency or "Recently used" semantics in the UI to drift.

**Why it happens:** Commands forget to call the `TouchClipboardEntryUseCase` or ignore its return value, despite it being the authoritative way to update usage timestamps.

**How to avoid:**

- Follow `restore_clipboard_entry_impl`’s order: `build_snapshot` → `touch` → `restore` → sync → emit event.
- For any additional restore-like operations, reuse `restore_clipboard_entry_impl` or create a shared helper that always includes touching.

**Warning signs:**

- Tests similar to `restore_entry_returns_error_before_clipboard_write_when_touch_fails` start failing or are bypassed.
- UI sorting appears inconsistent with actual usage.

## Code Examples

### Listing Clipboard Entry Projections (Backend → DTO)

```rust
// Source: src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs
pub async fn execute(
    &self,
    limit: usize,
    offset: usize,
) -> Result<Vec<EntryProjectionDto>, ListProjectionsError> {
    if limit == 0 { /* ... InvalidLimit ... */ }
    if limit > self.max_limit { /* ... InvalidLimit ... */ }

    let entries = self
        .entry_repo
        .list_entries(limit, offset)
        .await
        .map_err(|e| ListProjectionsError::RepositoryError(e.to_string()))?;

    let mut projections = Vec::with_capacity(entries.len());

    for entry in entries {
        // look up selection
        let selection = match self.selection_repo.get_selection(&entry.entry_id).await {
            Ok(Some(selection)) => selection,
            Ok(None) => { warn!("Skipping entry without selection"); continue; }
            Err(e) => { warn!(error = %e, "Skipping entry due to selection lookup failure"); continue; }
        };

        // preview representation
        let representation = match self
            .representation_repo
            .get_representation(&entry.event_id, &selection.selection.preview_rep_id)
            .await
        { /* ... skip on missing/failed ... */ };

        let is_image = representation
            .mime_type
            .as_ref()
            .map(|mt| mt.as_str().to_ascii_lowercase().starts_with(MIME_IMAGE_PREFIX))
            .unwrap_or(false);

        let preview = if let Some(data) = representation.inline_data.as_ref() {
            String::from_utf8_lossy(data).trim().to_string()
        } else if is_image {
            format!("Image ({} bytes)", representation.size_bytes)
        } else {
            entry
                .title
                .as_ref()
                .map(|title| title.trim().to_string())
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| "Text content (full payload in background processing)".to_string())
        };

        let content_type = representation
            .mime_type
            .as_ref()
            .map(|mt| mt.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let thumbnail_url = if is_image {
            match self.thumbnail_repo.get_by_representation_id(&selection.selection.preview_rep_id).await {
                Ok(Some(_metadata)) => Some(format!("uc://thumbnail/{}", preview_rep_id)),
                Ok(None) => None,
                Err(err) => { tracing::error!(error = %err, "Failed to fetch thumbnail metadata"); None }
            }
        } else { None };

        let has_detail = representation.blob_id.is_some()
            || matches!(representation.payload_state(), PayloadAvailability::Staged | PayloadAvailability::Processing);

        projections.push(EntryProjectionDto { /* ... fields ... */ });
    }

    Ok(projections)
}
```

### Frontend Mapping of Clipboard Entries

```typescript
// Source: src/api/clipboardItems.ts:145
export async function getClipboardItems(
  _orderBy?: OrderBy,
  limit?: number,
  offset?: number,
  _filter?: Filter
): Promise<ClipboardItemsResult> {
  const response = await invokeWithTrace<ClipboardEntriesResponse>('get_clipboard_entries', {
    limit: limit ?? 50,
    offset: offset ?? 0,
  })

  if (response.status === 'not_ready') {
    return { status: 'not_ready' }
  }

  const items = response.entries.map(entry => {
    const isImage = isImageType(entry.content_type)

    const item: ClipboardItem = {
      image: isImage
        ? {
            thumbnail: entry.thumbnail_url ?? null,
            size: entry.size_bytes,
            width: 0,
            height: 0,
          }
        : null,
      text: !isImage
        ? {
            display_text: entry.preview,
            has_detail: entry.has_detail,
            size: entry.size_bytes,
          }
        : null,
      file: null as unknown as ClipboardFileItem,
      link: null as unknown as ClipboardLinkItem,
      code: null as unknown as ClipboardCodeItem,
      unknown: null,
    }

    return {
      id: entry.id,
      is_downloaded: true,
      is_favorited: entry.is_favorited,
      created_at: entry.captured_at,
      updated_at: entry.updated_at,
      active_time: entry.active_time,
      item,
    }
  })

  return { status: 'ready', items }
}
```

## State of the Art

| Old Approach                                                            | Current Approach                                                                              | When Changed                                                                          | Impact                                                              |
| ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| Legacy `src-tauri/src/api` commands directly touching runtime and infra | `uc-tauri` crate with commands delegating only to `runtime.usecases()` and returning DTOs     | Phases 10–13 (boundary and decomposition)                                             | Stronger boundaries, better testability, easier contract hardening. |
| Frontend-driven clipboard projections (ad-hoc mapping from raw rows)    | Backend `ListClipboardEntryProjections` use case + `ClipboardEntryProjection` DTO             | Introduced by clipboard projections refactor (pre‑Phase 15 but completed by Phase 13) | Centralizes preview/thumbnail/has_detail logic in backend.          |
| Mixed camelCase/snake_case across responses                             | Explicit serde config on DTOs (snake_case for clipboard entries, camelCase for lifecycle DTO) | Phase 11 / models.rs tests                                                            | Enables CONTRACT-03 compliance with deterministic serialization.    |

**Deprecated/outdated:**

- Direct Tauri command access to runtime dependencies (`runtime.deps.xxx`) is deprecated; commands must go through `runtime.usecases()` and `uc-tauri::models`.
- Any clipboard commands still wired in the legacy `src-tauri/src/api` module should be considered obsolete for planning purposes; Phase 15 should not extend that layer.

## Open Questions

1. **Where is favorite state stored and how should toggling interact with sync?**
   - What we know: `EntryProjectionDto` includes `is_favorited` (currently hard-coded `false` in `ListClipboardEntryProjections`), and the frontend calls `toggle_favorite_clipboard_item` with `{ id, isFavorited }` but no backend implementation exists.
   - What's unclear: Whether favorites are meant to stay local or be synced, and whether a dedicated port/use case exists (or needs to be added) for updating this flag.
   - Recommendation: For Phase 15 planning, assume favorites are stored on the entry record via `ClipboardEntryRepositoryPort` and implement a `ToggleFavoriteClipboardEntry` use case plus a `toggle_favorite_clipboard_item` command that only flips the flag locally; defer any sync semantics to a later architecture phase if not already defined.

2. **How exactly should stats be computed (scope, filters, encryption readiness)?**
   - What we know: Frontend defines `ClipboardStats` with `total_items` and `total_size`, and has `getClipboardStats()` that calls `get_clipboard_stats` (missing on the backend). `ListClipboardEntryProjections` already has access to `total_size` and entries.
   - What's unclear: Whether stats should include encrypted entries only when decryption is ready, and whether filters (e.g., by content type or favorites) are needed now or later.
   - Recommendation: For Phase 15, define a stats use case that counts all entries visible to the current user, uses `total_size` as sum, and follows the same encryption readiness gating as `get_clipboard_entries`; keep filters out of scope unless required later.

3. **Should `get_clipboard_item` remain as a thin alias or be replaced by more granular detail/resource calls?**
   - What we know: Frontend implements `getClipboardItem(id, fullContent)` and calls `invokeWithTrace('get_clipboard_item', { id, fullContent })`, but backend only has `get_clipboard_entry_detail` and `get_clipboard_entry_resource` commands wired.
   - What's unclear: Whether `get_clipboard_item` should be fully wired on the backend, or the frontend should migrate to using `getClipboardEntryDetail` / `getClipboardEntryResource` directly.
   - Recommendation: For Phase 15, plan to implement a `get_clipboard_item` command that composes detail/resource as needed to satisfy existing UI callers, then later simplify the frontend once Phase 14/15 integration is stable.

## Validation Architecture

（`.planning/config.json` 未显式将 `workflow.nyquist_validation` 设为 false，因此视为启用。）

### Test Framework

| Property             | Value                                                                                                            |
| -------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Framework            | Frontend: Vitest; Backend: Rust `cargo test` (workspace)                                                         |
| Frontend config file | Implicit (Vitest via Vite + `package.json` `test` script)                                                        |
| Backend config file  | None (standard Rust test discovery under `src-tauri/`)                                                           |
| Quick run command    | Frontend: `bun test src/api/__tests__/clipboardItems.test.ts`; Backend: `cd src-tauri && cargo test -p uc-tauri` |
| Full suite command   | Frontend: `bun test`; Backend: `cd src-tauri && cargo test` or `bun run test:coverage`                           |

### Phase Requirements → Test Map

| Req ID      | Behavior                                                                                                                            | Test Type          | Automated Command                                                                                                | File Exists?                                                                                                                                        |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------- | ------------------ | ---------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| CONTRACT-03 | Clipboard commands serialize payloads in a frontend-compatible shape (snake_case / camelCase as expected) and are covered by tests. | unit + integration | Backend: `cd src-tauri && cargo test -p uc-tauri`; Frontend: `bun test src/api/__tests__/clipboardItems.test.ts` | ✅ `src-tauri/crates/uc-tauri/src/models/mod.rs`, `src-tauri/crates/uc-tauri/src/commands/clipboard.rs`, `src/api/__tests__/clipboardItems.test.ts` |

For Phase 15 specifically, new tests should include:

- Rust tests around new clipboard stats and favorite commands, asserting DTO serialization (`serde_json::to_value`) and command behavior (including error cases).
- Vitest tests for `getClipboardStats`, `favoriteClipboardItem`, and `unfavoriteClipboardItem`, using the same mocking pattern as `clipboardItems.test.ts` to check command names and payload shapes.

### Sampling Rate

- **Per task commit:**
  - Run `cd src-tauri && cargo test -p uc-tauri` for quick backend verification.
  - Run `bun test src/api/__tests__/clipboardItems.test.ts` for frontend API mapping.
- **Per wave merge:**
  - Run full frontend suite: `bun test`.
  - Run full backend suite: `cd src-tauri && cargo test` (or `bun run test:coverage` when checking coverage).
- **Phase gate:**
  - Require all clipboard-related backend tests (`uc-tauri` commands and related use cases) and `clipboardItems` Vitest tests to be green before `/gsd:verify-work` for Phase 15.

### Wave 0 Gaps

- [ ] No Rust tests yet for a hypothetical `get_clipboard_stats` command and its DTO; these must be added in `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` (or a dedicated tests module) once the command exists.
- [ ] No Rust tests yet for a `toggle_favorite_clipboard_item` command; tests should cover both favoriting and unfavoriting, including not-found behavior and serialization of responses.
- [ ] No tests currently validate `get_clipboard_item` command behavior; once wired, add tests to mirror the orchestration pattern used in `restore_clipboard_entry_impl`.

If these are addressed during Phase 15 implementation, the validation architecture will fully cover CONTRACT-03 for clipboard management.

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — Existing clipboard commands (`get_clipboard_entries`, `delete_clipboard_entry`, `get_clipboard_entry_detail`, `get_clipboard_entry_resource`, `sync_clipboard_items`, `restore_clipboard_entry`) and associated tests.
- `src-tauri/crates/uc-tauri/src/models/mod.rs` — Clipboard DTO definitions and serialization tests for `ClipboardEntriesResponse` and `ClipboardEntryProjection`.
- `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs` — Capture use case and title generation/representation handling.
- `src-tauri/crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs` — Restore use case and passive mode behavior.
- `src-tauri/crates/uc-app/src/usecases/clipboard/touch_clipboard_entry.rs` — Touch use case for updating `active_time`.
- `src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs` — Entry projection use case powering `get_clipboard_entries`.
- `src/api/clipboardItems.ts` — Frontend clipboard API contract (stats, items, detail, favorites, sync, copy).
- `src/api/__tests__/clipboardItems.test.ts` — Existing Vitest coverage for `getClipboardItems` mapping.
- `.planning/STATE.md` — Phase history and key decisions (Phases 10–13) affecting boundaries.
- `.planning/REQUIREMENTS.md` — CONTRACT-03 definition and mapping to Phase 15.
- `.planning/codebase/CONVENTIONS.md` — Naming, error handling, and DTO/command conventions.

### Secondary (MEDIUM confidence)

- `src-tauri/Cargo.toml` and `package.json` — Confirmed Rust workspace structure, test commands, and frontend tooling (Tauri 2, Vitest, Bun).

### Tertiary (LOW confidence)

- None used for this phase; all findings are based on the local codebase and planning metadata.

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — Directly derived from Cargo/Node configs and existing clipboard code paths.
- Architecture: HIGH — Clipboard command patterns and use cases are fully implemented with tests; Phase 10–13 decisions are documented in `.planning/STATE.md`.
- Pitfalls: MEDIUM — Based on observed patterns (encryption readiness, passive mode, DTO tests) and standard Tauri/serde behavior; should be refined if additional undocumented constraints exist in DeepWiki.

**Research date:** 2026-03-07
**Valid until:** 2026-04-06 (clipboard architecture is stable but Phase 15/14 integration work may introduce new contracts or tests that slightly adjust implementation details).
