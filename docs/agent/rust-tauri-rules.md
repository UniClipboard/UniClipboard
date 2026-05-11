# Rust and Tauri Rules

Use this document when editing Rust, Tauri commands, daemon handlers, async loops, tracing, or frontend event payloads.

## Rust Error Handling

- **No `unwrap()` / `expect()` in production code.**
  - **Tests are the only exception.**
- **No silent failures in async or event-driven code.**
  - Errors must be **logged** and **observable** by upper layers.

## Async Network Loop Safety

- In single-loop async drivers (for example `tokio::select!` + network poll loops), never `await` operations that require the same loop to make progress.
- If a business operation can block (dial/open/write/close), dispatch it out of the poll loop and keep the poll loop responsive.
- Treat `oneshot send failed` / "failed to deliver result to caller" as a symptom (caller dropped), not root cause; trace upstream scheduling/state progression first.
- Command-level timeout budgets must be strictly larger than inner stage budgets (`open + write + close + buffer`), never equal.

## Tauri Command Tracing

- **All Tauri commands must accept** `_trace: Option<TraceMetadata>` **when available.**
- Each command must:
  - Create an `info_span!` with `trace_id` and `trace_ts` fields
  - Call `record_trace_fields(&span, &_trace)`
  - `.instrument(span)` the async body

## Rust Logging (`tracing`)

- **Use `tracing` for all logging.** Do not use `println!`, `eprintln!`, or `log` macros in production code.
- **Prefer structured fields over string formatting.**
- **Use spans to model request/task lifetimes.** Attach contextual fields once, log events inside.
- **Record errors with context, not silence.**
- **Avoid logging secrets.**

Example:

```rust
use tracing::{info, warn, error, debug, info_span, Instrument};

pub async fn sync_peer(peer_id: &str, attempt: u32) -> Result<(), SyncError> {
    let span = info_span!("sync_peer", peer_id = %peer_id, attempt = attempt);

    async move {
        info!("start");

        let session = match open_session(peer_id).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "open_session failed; will retry if possible");
                return Err(SyncError::OpenSession(e));
            }
        };

        debug!(session_id = %session.id(), "session opened");

        if let Err(e) = push_updates(&session).await {
            error!(error = %e, "push_updates failed");
            return Err(SyncError::PushUpdates(e));
        }

        info!("done");
        Ok(())
    }
    .instrument(span)
    .await
}
```

## Tauri State Lifecycle

- Any type accessed via `tauri::State<T>` must be registered **before startup** with `.manage()`.

## Tauri Event Payload Serialization

- **All `#[derive(serde::Serialize)]` structs emitted to the frontend via `app.emit()` MUST include `#[serde(rename_all = "camelCase")]`.**
- Verify frontend listener field names match camelCase output.
- Add a test that proves camelCase keys exist and snake_case keys do not.

## Enum Wire Serialization (`rename_all_fields`)

`#[serde(rename_all = "...")]` on an **enum** only renames the **variant names** — it does **NOT** touch named fields inside struct-style variants. This is a hidden serde footgun that caused issue #606 (PUT `/settings` returning 422 because `RetentionRuleDto::ByAge { max_age }` serialized to `{"byAge": {"max_age": N}}` while the frontend sent `{"byAge": {"maxAge": N}}`).

**Rule:** an enum **MUST** also declare `rename_all_fields = "..."` (matching the variant case style) when **all three** conditions hold:

1. It is an `enum` (not a `struct` — `rename_all` on a struct already covers its fields).
2. At least one variant is **struct-style** (`Foo { name1, name2 }`, with named fields). Tuple variants `Foo(T)` and unit variants `Foo` are exempt.
3. Any field name is **multi-word snake_case** (`max_age` → `maxAge` cares; single-word `current` doesn't).

Even when condition 3 is currently false, prefer to **declare `rename_all_fields` defensively** for any enum with struct variants — the next PR that adds a multi-word field will silently emit the wrong wire shape if the attribute is missing.

**Tests:** every such enum must have wire-shape tests that

- assert the camelCase / snake_case wire literal verbatim, and
- explicitly **reject** the legacy snake_case-in-camelCase-wire (or vice versa) bug shape, so a future revert is caught immediately.

See `uc-daemon-contract::api::dto::settings::retention_rule_dto_tests` for the canonical pattern.

## Cargo Command Location

- **All Rust-related commands** (`cargo build`, `cargo test`, `cargo check`, etc.) **must be executed from `src-tauri/`.**
- **Never run Cargo commands from the project root.**
- If `Cargo.toml` is not present in the current directory, stop immediately and do not retry.

## Rustdoc Bilingual Documentation Guide

Use English-first, Chinese-second side-by-side rustdoc when public APIs need long-term maintenance documentation.

```rust
/// Load or create a local device identity.
///
/// 加载或创建本地设备标识。
///
/// # Behavior / 行为
/// - If an ID exists on disk, it will be loaded.
/// - Otherwise, a new ID will be generated and persisted.
///
/// - 如果磁盘上已有 ID，则直接加载。
/// - 否则生成新的 ID 并持久化保存。
pub fn load_or_create() -> Result<Self> {
    // ...
}
```
