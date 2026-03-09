# Phase 11: Command Contract Hardening - Research

**Researched:** 2026-03-06
**Domain:** Tauri command layer contracts — DTO mapping, typed error taxonomy, serialization compatibility
**Confidence:** HIGH

---

<phase_requirements>

## Phase Requirements

| ID          | Description                                                                                                                          | Research Support                                                                                                                                                                      |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CONTRACT-01 | User-visible command responses use explicit DTOs instead of returning domain models directly                                         | Audit reveals `list_paired_devices` returns `PairedDevice` (domain model); `get_settings` returns `Value`; `setup.rs` returns `String`-encoded JSON — these are the migration targets |
| CONTRACT-02 | Command failures are returned with structured, typed error categories rather than raw `String`-only errors                           | All commands currently `map_err(\|e\| e.to_string())` — need `CommandError` enum with `#[derive(Serialize)]` replacing raw strings                                                    |
| CONTRACT-03 | Command/event payload serialization remains frontend-compatible (camelCase where required) with tests covering key payload contracts | Mixed casing today: models use `snake_case`, pairing uses `camelCase`; tests exist for events but not command payload shapes                                                          |
| CONTRACT-04 | Command timeout/error contracts distinguish cancellation, timeout, and internal failures for reliable UI handling                    | `spawn_blocking` join errors collapse to a single string; no distinction between timeout, cancellation, and internal failure at command boundary                                      |

</phase_requirements>

---

## Summary

Phase 11 adds stable DTO and typed error contracts to the command surface. Phase 10 closed the dependency-direction violations; this phase closes the API-shape violations. The command layer (`src-tauri/crates/uc-tauri/src/commands/`) is the driving adapter in hexagonal architecture — it must translate between the domain world and the frontend world without leaking domain internals.

The two primary problems are distinct and non-overlapping:

1. **Plan 11-01 (DTO mapping)**: Some commands return domain models directly (e.g., `list_paired_devices` returns `Vec<PairedDevice>` from `uc-core`), some return serialized strings instead of typed structs (all `setup.rs` commands return `Result<String, String>`), and payload serialization casing is inconsistent across modules. The fix is to add DTO structs to `models/` and map domain models at the command boundary, then write payload shape tests.

2. **Plan 11-02 (typed errors)**: Every command currently converts errors with `.map_err(|e| e.to_string())`. The `error.rs` module has a stub `map_err()` function explicitly noted as "future upgrade path." Tauri supports returning any `Serialize`-able type as the error value. The fix is to introduce a `CommandError` enum with categories (`NotFound`, `InternalError`, `Timeout`, `Cancelled`, `ValidationError`, `Conflict`) and migrate all command handlers to use it.

**Primary recommendation:** Introduce `CommandError` enum in `commands/error.rs` with `#[derive(Serialize, thiserror::Error)]` carrying a `code` field; implement `From<anyhow::Error>` for fallback; add DTO structs under `models/` for the three leaking domain model return sites; write serialization snapshot tests in `#[cfg(test)]` modules.

---

## Standard Stack

### Core

| Library      | Version           | Purpose                                                     | Why Standard                     |
| ------------ | ----------------- | ----------------------------------------------------------- | -------------------------------- |
| `serde`      | 1.x (workspace)   | Derive `Serialize`/`Deserialize` on DTOs and `CommandError` | Already in all crates            |
| `serde_json` | 1.x (workspace)   | JSON snapshot tests for payload shapes                      | Already in `uc-tauri`            |
| `thiserror`  | 2.0.x (workspace) | `#[derive(thiserror::Error)]` on `CommandError`             | Already in `uc-tauri/Cargo.toml` |

### Supporting

| Library      | Version   | Purpose                                                                     | When to Use                                                  |
| ------------ | --------- | --------------------------------------------------------------------------- | ------------------------------------------------------------ |
| `tauri::ipc` | Tauri 2.x | Tauri accepts any `Serialize` as command error — no additional crate needed | Always: Tauri 2 desugars `Result<T, E>` where `E: Serialize` |

### Alternatives Considered

| Instead of                                  | Could Use                       | Tradeoff                                                                                                 |
| ------------------------------------------- | ------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `thiserror` for `CommandError`              | Manual `impl std::error::Error` | `thiserror` is already a dependency; manual impl adds boilerplate with no gain                           |
| Separate error code field on `CommandError` | HTTP-style integer codes        | String codes are self-documenting and directly usable as TypeScript discriminants without a codegen step |

**No new dependencies required.** All needed crates are already in workspace.

---

## Architecture Patterns

### Recommended Project Structure

The existing layout is correct. Additions fit within it:

```
src-tauri/crates/uc-tauri/src/
├── commands/
│   ├── error.rs          # Expand: CommandError enum replaces String
│   ├── clipboard.rs      # No DTO change needed (models/ already used)
│   ├── pairing.rs        # list_paired_devices: add PairedDeviceDto to models/
│   ├── setup.rs          # Return typed SetupStateResponse DTO, not String
│   └── settings.rs       # get_settings: return SettingsDto (wrapper), not serde_json::Value
└── models/
    └── mod.rs            # Expand: add PairedDeviceDto, SetupStateResponse, SettingsDto
```

### Pattern 1: Typed CommandError Enum

**What:** A single `CommandError` enum covering all categories of command failure, serialized to the frontend with a machine-readable `code` field.

**When to use:** All command return types — replace every `Result<T, String>` with `Result<T, CommandError>`.

**Example:**

```rust
// src-tauri/crates/uc-tauri/src/commands/error.rs
use serde::Serialize;

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(tag = "code", content = "message")]
pub enum CommandError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("internal error: {0}")]
    InternalError(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("cancelled: {0}")]
    Cancelled(String),

    #[error("validation error: {0}")]
    ValidationError(String),

    #[error("conflict: {0}")]
    Conflict(String),
}

impl CommandError {
    /// Upgrade path from legacy String errors.
    /// Maps anyhow::Error to InternalError fallback.
    pub fn from_anyhow(err: anyhow::Error) -> Self {
        CommandError::InternalError(err.to_string())
    }
}
```

**Frontend TypeScript discriminated union (for reference — planners verify with TS team):**

```typescript
type CommandError =
  | { code: 'NotFound'; message: string }
  | { code: 'InternalError'; message: string }
  | { code: 'Timeout'; message: string }
  | { code: 'Cancelled'; message: string }
  | { code: 'ValidationError'; message: string }
  | { code: 'Conflict'; message: string }
```

### Pattern 2: DTO Structs in models/

**What:** Explicit frontend-facing structs in `uc-tauri/src/models/mod.rs` for every domain model currently returned directly by commands.

**When to use:** Any time a command currently imports from `uc-core` or `uc-app` in its return type.

**Identified leaking domain models (from code audit):**

1. `list_paired_devices` → returns `Vec<PairedDevice>` from `uc-core::network`. `PairedDevice` has `PeerId` (newtype) and `PairingState` (enum), serialized with whatever `uc-core` decides. Needs `PairedDeviceDto`.

2. `get_settings` → returns `serde_json::Value` (bypasses type safety entirely). Should return a `SettingsDto` wrapper that owns a stable JSON representation, or directly `serde_json::Value` is acceptable if CONTRACT-01 is interpreted as "no domain model struct leakage" and `Settings` is already a DTO-like domain model. **Decision for planner:** the requirement says "explicit DTOs instead of domain models" — `serde_json::Value` is worse than a domain model because it loses type safety entirely. Use `Settings` wrapped in a command-layer re-export or rename.

3. `get_setup_state`, `start_new_space`, etc. → return `Result<String, String>` where the string is a JSON-serialized `SetupState`. This is a layering violation: the command is doing its own serialization instead of returning a typed value. Return `Result<SetupState, CommandError>` directly.

4. `get_local_device_info` → returns `LocalDeviceInfo` from `uc-app::usecases`. This is a use-case output DTO, not a domain model — arguably acceptable, but Phase 11's plan scope calls for having all command response types live in the `uc-tauri` layer. Planner discretion: either re-export or create a thin `LocalDeviceInfoDto`.

**Example DTO for PairedDevice:**

```rust
// models/mod.rs addition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDeviceDto {
    pub peer_id: String,
    pub device_name: String,
    pub pairing_state: String,  // "Pending" | "Trusted" | "Revoked"
    pub paired_at: String,      // RFC3339
    pub last_seen_at: Option<String>,
}
```

### Pattern 3: Payload Contract Tests

**What:** `#[test]` functions that serialize a known DTO or event value to JSON and assert the exact shape with `serde_json::json!`.

**When to use:** For every DTO or event struct that the frontend depends on.

**Example (from existing encryption event test — this is the pattern to follow):**

```rust
// From: src-tauri/crates/uc-tauri/src/events/mod.rs (existing)
#[test]
fn encryption_event_serializes_with_type_tag() {
    let ready = serde_json::to_value(EncryptionEvent::SessionReady).unwrap();
    assert_eq!(ready, serde_json::json!({ "type": "SessionReady" }));
}
```

**New tests to add for Phase 11:**

```rust
// commands/error.rs tests
#[test]
fn command_error_not_found_serializes_with_code() {
    let err = CommandError::NotFound("entry-1".to_string());
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["code"], "NotFound");
    assert!(json["message"].is_string());
}

// models/mod.rs tests — for each new DTO
#[test]
fn clipboard_entries_response_ready_has_entries_field() {
    let resp = ClipboardEntriesResponse::Ready { entries: vec![] };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["status"], "ready");
    assert!(json["entries"].is_array());
}
```

### Pattern 4: spawn_blocking Error Category Mapping

**What:** When `tokio::task::spawn_blocking` is used (clipboard.rs lines 279 and 352), the outer `Err` arm is a `JoinError` representing task cancellation or panic — not a business logic error. This must map to `CommandError::Cancelled` or `CommandError::InternalError` distinctly from use case errors.

**Current code (lines 279-296 in clipboard.rs):**

```rust
match tokio::task::spawn_blocking(move || { ... }).await {
    Ok(Ok(())) => Ok(true),
    Ok(Err(err)) => Err(format!("Outbound clipboard sync command failed: {err}")),
    Err(err) => Err(format!("Outbound clipboard sync command task join failed: {err}")),
}
```

**After migration:**

```rust
match tokio::task::spawn_blocking(move || { ... }).await {
    Ok(Ok(())) => Ok(true),
    Ok(Err(err)) => Err(CommandError::InternalError(format!("sync failed: {err}"))),
    Err(join_err) if join_err.is_cancelled() => Err(CommandError::Cancelled("sync task cancelled".to_string())),
    Err(join_err) => Err(CommandError::InternalError(format!("sync task panic: {join_err}"))),
}
```

### Anti-Patterns to Avoid

- **Returning `serde_json::Value` as a command response:** Loses compile-time safety and forces frontend to parse blindly. Use typed structs.
- **Double-encoding JSON:** `setup.rs` currently serializes `SetupState` to a JSON string and then Tauri serializes the string again. The frontend gets `"\"Welcome\""` (double-encoded). Return `SetupState` directly and let Tauri handle the serialization.
- **Using `String` as both the error and domain data carrier:** When a command returns `Result<String, String>`, the frontend cannot distinguish "the value is the string" from "an error occurred and it's a string." Use `Result<T, CommandError>`.
- **Per-command error types:** Don't create `ClipboardError`, `PairingError`, etc. for the command boundary. That belongs at the use-case layer (which already does it with `thiserror`). The command layer has a single `CommandError` taxonomy.

---

## Don't Hand-Roll

| Problem                             | Don't Build                                 | Use Instead                                                            | Why                                                                                                           |
| ----------------------------------- | ------------------------------------------- | ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Typed error serialization           | Custom `impl Serialize for MyError` by hand | `thiserror` + `#[derive(Serialize)]` with `#[serde(tag = "code")]`     | Already in all crates; handles `Display` and structured serialization together                                |
| Mapping `anyhow::Error` chains      | Custom error chain walker                   | `err.to_string()` wrapped in `CommandError::InternalError`             | Anyhow's `Display` already traverses the chain; adding a chain walker adds complexity for no frontend benefit |
| Frontend TypeScript type generation | Manual TypeScript interfaces                | Write Rust structs with `serde`, verify shape with JSON snapshot tests | The tests are the contract; TypeScript types are a downstream concern                                         |

**Key insight:** The command layer error taxonomy does NOT need to be exhaustive or match use-case error types 1:1. Its job is to give the frontend enough signal to make a UI decision (show "not found" vs "retry" vs "unexpected error"). The use-case typed errors are for correctness; the command errors are for UI handling.

---

## Common Pitfalls

### Pitfall 1: Double-Encoded JSON from setup.rs

**What goes wrong:** `setup.rs` commands return `Result<String, String>` where the `Ok` value is `serde_json::to_string(&state)`. Tauri then wraps this in JSON again, so the frontend receives a JSON string that contains escaped JSON. The frontend `setup.ts` works around this with a `decodeSetupState` function that calls `JSON.parse` on the result. If the DTO is changed to return `SetupState` directly, the frontend must also remove the double-decode.

**Why it happens:** The original implementation did not return typed structs — it used raw string encoding as a workaround.

**How to avoid:** Return `SetupState` directly from setup commands and update the frontend `decodeSetupState` shim to remove the `JSON.parse` layer. This is a breaking change coordinated between backend and frontend.

**Warning signs:** `JSON.parse` calls in frontend command consumers; `serde_json::to_string` in command handlers.

### Pitfall 2: Inconsistent Casing Between Modules

**What goes wrong:** `models/mod.rs` uses `snake_case` field names (no `rename_all`) while `commands/pairing.rs` DTOs use `#[serde(rename_all = "camelCase")]`. If new DTOs are added without explicit casing attributes, field names will be inconsistent.

**Why it happens:** No casing convention was enforced at module creation time.

**How to avoid:** Establish a convention for this phase: all new command-layer DTOs in `models/mod.rs` use `#[serde(rename_all = "camelCase")]` to match the frontend JavaScript convention. Update existing models if they don't comply (coordinate with frontend). The existing `ClipboardEntryProjection` uses `snake_case` and the frontend TypeScript already uses `snake_case` field names — so for that DTO, don't change (would be a breaking change).

**Warning signs:** Frontend TypeScript interfaces that use `has_detail`, `size_bytes` (snake_case) while Rust uses `camelCase` attributes.

### Pitfall 3: PairedDevice Domain Model Has `PeerId` Newtype

**What goes wrong:** `PairedDevice.peer_id` is typed as `uc_core::PeerId` (a newtype wrapping `String`). If returned directly, `PeerId` serializes as `{"inner": "..."}` unless it has a custom `Serialize` implementation. Adding a `PairedDeviceDto` with `peer_id: String` avoids this dependency.

**How to avoid:** Always map to plain Rust primitives (`String`, `i64`, `bool`) in DTO structs. Never include newtypes from `uc-core` in `models/mod.rs`.

### Pitfall 4: `get_lifecycle_status` Returns JSON String

**What goes wrong:** `get_lifecycle_status` (lifecycle.rs line 41) returns `Result<String, String>` where the string is JSON-encoded lifecycle status, similar to setup.rs. The pattern is the same anti-pattern.

**How to avoid:** Return a typed `LifecycleStatusDto` struct. If the planner keeps this in scope, add it to models. If out of scope, flag as follow-up.

### Pitfall 5: `get_settings` Returns `serde_json::Value`

**What goes wrong:** `get_settings` returns `Result<serde_json::Value, String>`. The `Settings` domain model from `uc-core` is serialized to `Value` in the command handler. This means the frontend's TypeScript type for settings is not verified against the Rust struct shape.

**How to avoid:** Return the `Settings` struct directly (it already derives `Serialize`). Add a payload test that asserts the JSON shape of `Settings`. Alternatively, create a `SettingsDto` if the domain model has fields that should not be exposed.

---

## Code Examples

Verified patterns from existing sources:

### Existing: Encryption Event with Type Tag (HIGH confidence)

```rust
// Source: src-tauri/crates/uc-tauri/src/events/mod.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EncryptionEvent {
    Initialized,
    SessionReady,
    Failed { reason: String },
}
```

This is the established pattern for discriminated unions with a `type` tag. For `CommandError`, use `#[serde(tag = "code")]` for a `code` discriminant instead.

### Existing: ClipboardEntriesResponse with Status Tag (HIGH confidence)

```rust
// Source: src-tauri/crates/uc-tauri/src/models/mod.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ClipboardEntriesResponse {
    Ready { entries: Vec<ClipboardEntryProjection> },
    NotReady,
}
```

Use the same `#[serde(tag = "...")]` approach for `CommandError`. Note this enum uses `snake_case` for status values (`ready`, `not_ready`).

### Existing: PairedDevice in pairing.rs Already Has a DTO (HIGH confidence)

```rust
// Source: src-tauri/crates/uc-tauri/src/commands/pairing.rs
// PairedPeer is a command-layer DTO for paired device display
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedPeer {
    pub peer_id: String,
    pub device_name: String,
    // ...
}
```

The `get_paired_peers` command already returns `Vec<PairedPeer>` (a DTO) — this is the correct pattern. The problem is `list_paired_devices` bypasses this and returns `Vec<PairedDevice>` directly. The fix is to make `list_paired_devices` also map to `Vec<PairedPeer>` or a similar DTO.

### Existing: spawn_blocking Task Error Pattern (HIGH confidence)

```rust
// Source: src-tauri/crates/uc-tauri/src/commands/clipboard.rs (lines 279-296)
match tokio::task::spawn_blocking(move || { ... }).await {
    Ok(Ok(())) => Ok(true),
    Ok(Err(err)) => Err(format!("...")),
    Err(err) => Err(format!("... task join failed: {err}")),
}
```

The outer `Err(join_err)` represents either task cancellation (`join_err.is_cancelled()`) or panic (`join_err.is_panic()`). Map these to distinct `CommandError` categories.

---

## State of the Art

| Old Approach                                        | Current Approach                                                                  | Impact                                                                   |
| --------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `Result<String, String>` everywhere                 | Already partially migrated — clipboard/pairing use typed DTOs and `String` errors | Phase 11 completes the migration of error type; DTO migration is partial |
| `pub fn map_err(err: anyhow::Error) -> String` stub | Stub exists with comment "future enhancement"                                     | The stub is the designed upgrade point — Phase 11 fulfills it            |
| Domain models returned directly                     | `models/mod.rs` DTOs exist for clipboard; pairing has inline DTOs                 | Gap: `list_paired_devices`, setup commands, lifecycle status             |

**Deprecated/outdated:**

- `commands/error.rs` returning `String`: noted in code as temporary; Phase 11 replaces this
- `setup.rs` double-encoding: the `decodeSetupState` shim in `setup.ts` must be updated when backend changes to typed returns

---

## Open Questions

1. **Should `SettingsDto` wrap `Settings` or re-expose it?**
   - What we know: `Settings` from `uc-core` already derives `Serialize` and is the domain model
   - What's unclear: whether `Settings` has any fields that should be hidden from frontend or transformed
   - Recommendation: Return `Settings` directly for now (it is already a clean model), but rename in the command signature to make the DTO intent explicit. Add a payload shape test. If `Settings` ever gains internal-only fields, a proper DTO becomes necessary.

2. **Frontend breaking change from setup.rs DTO migration**
   - What we know: `setup.ts` uses `decodeSetupState` with `JSON.parse` to handle the double-encoding
   - What's unclear: Whether the planner should update the frontend `setup.ts` in the same plan as the backend change, or leave it to a follow-up
   - Recommendation: Update frontend and backend together in the same plan task to avoid a window where both old and new behavior coexist. The change is small (remove `decodeSetupState`'s `JSON.parse` branch).

3. **`get_lifecycle_status` scope**
   - What we know: Returns `Result<String, String>` with JSON-encoded status (same anti-pattern as setup)
   - What's unclear: Whether lifecycle status DTO is in Phase 11 scope or deferred to Phase 12 (lifecycle governance)
   - Recommendation: Include it in Plan 11-02's migration sweep since it's the same error-pattern fix, but keep the DTO minimal.

---

## Validation Architecture

> `workflow.nyquist_validation` is absent from `.planning/config.json` — treated as enabled.

### Test Framework

| Property           | Value                                                                      |
| ------------------ | -------------------------------------------------------------------------- |
| Framework          | Rust built-in `#[test]` + `#[tokio::test]` (no separate test crate needed) |
| Config file        | `src-tauri/crates/uc-tauri/Cargo.toml` (standard `[dev-dependencies]`)     |
| Quick run command  | `cd src-tauri && cargo test -p uc-tauri 2>&1 \| tail -20`                  |
| Full suite command | `cd src-tauri && cargo test -p uc-tauri`                                   |

### Phase Requirements → Test Map

| Req ID      | Behavior                                                                            | Test Type | Automated Command                                                   | File Exists?                                              |
| ----------- | ----------------------------------------------------------------------------------- | --------- | ------------------------------------------------------------------- | --------------------------------------------------------- |
| CONTRACT-01 | `PairedDeviceDto` serializes without `uc-core` types                                | unit      | `cd src-tauri && cargo test -p uc-tauri paired_device_dto`          | Wave 0 (new test in models/mod.rs)                        |
| CONTRACT-01 | Setup commands return typed struct, not double-encoded string                       | unit      | `cd src-tauri && cargo test -p uc-tauri encode_setup_state`         | Wave 0 (update existing `commands/setup.rs` test)         |
| CONTRACT-02 | `CommandError::NotFound` serializes with `code` field                               | unit      | `cd src-tauri && cargo test -p uc-tauri command_error`              | Wave 0 (new test in commands/error.rs)                    |
| CONTRACT-02 | `CommandError::Timeout` and `Cancelled` are distinct                                | unit      | `cd src-tauri && cargo test -p uc-tauri command_error_timeout`      | Wave 0 (new test in commands/error.rs)                    |
| CONTRACT-03 | `ClipboardEntriesResponse::Ready` serializes as `{status: "ready", entries: [...]}` | unit      | `cd src-tauri && cargo test -p uc-tauri clipboard_entries_response` | ✅ Existing (events/mod.rs pattern; check if test exists) |
| CONTRACT-03 | Event payloads: `EncryptionEvent`, `ClipboardEvent` shape assertions                | unit      | `cd src-tauri && cargo test -p uc-tauri encryption_event`           | ✅ Existing (events/mod.rs has these tests)               |
| CONTRACT-03 | Frontend: `getClipboardItems` maps snake_case response fields                       | unit      | `bun run test src/api/__tests__/clipboardItems.test.ts`             | ✅ Existing                                               |
| CONTRACT-04 | `spawn_blocking` cancelled join maps to `CommandError::Cancelled`                   | unit      | `cd src-tauri && cargo test -p uc-tauri sync_clipboard_task_error`  | Wave 0 (new test in clipboard.rs)                         |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-tauri`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-tauri && bun run test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-tauri/src/models/mod.rs` — add `PairedDeviceDto` with serialization test
- [ ] `src-tauri/crates/uc-tauri/src/commands/error.rs` — expand `CommandError` enum with tests for each variant's JSON shape
- [ ] `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — add test verifying `spawn_blocking` join error maps to distinct `CommandError` category (unit test for the match arm)
- [ ] Update `commands/setup.rs` existing test (`encode_setup_state_welcome`) — test must verify that `SetupState` value serializes correctly without the double-encoding wrapper

---

## Sources

### Primary (HIGH confidence)

- Direct code audit: `src-tauri/crates/uc-tauri/src/commands/*.rs` — all command return types catalogued
- Direct code audit: `src-tauri/crates/uc-tauri/src/models/mod.rs` — existing DTO patterns
- Direct code audit: `src-tauri/crates/uc-tauri/src/events/mod.rs` — serialization test patterns
- Direct code audit: `src-tauri/crates/uc-tauri/src/commands/error.rs` — stub `map_err` function with explicit "future enhancement" comment
- Direct code audit: `src/api/clipboardItems.ts`, `src/api/setup.ts` — frontend casing expectations and double-decode pattern

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-tauri/Cargo.toml` — confirmed `thiserror = "2.0"` already present; no new dependencies needed
- `.planning/phases/10-boundary-repair-baseline/10-CONTEXT.md` — confirmed Phase 11 scope starts where Phase 10 left off: "Typed error migration into port surfaces — Phase 11"

### Tertiary (LOW confidence)

- None — all findings are directly from code inspection.

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — no new dependencies; all crates already in workspace
- Architecture: HIGH — patterns directly observed in existing code (encryption event tests, models/mod.rs, map_err stub)
- Pitfalls: HIGH — double-encoding and casing inconsistencies confirmed by direct code reading
- Serialization shape: HIGH — JSON snapshot test pattern exists and is functional in events/mod.rs

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (Tauri 2 and serde are stable; 30-day window appropriate)
