# AGENTS.md

## Engineering Principles

- **Fix root causes, not symptoms.** No patchy or workaround-driven solutions.
- **Do not “fix feelings”, fix structure.** Repeated workarounds indicate architectural flaws.
- **Short-term compromises must be reversible.**
- **Never break boundaries.** If something must be deferred, leave an explicit `TODO`.

## AI Review Intake (Required)

- External reviewer suggestions (CodeRabbit/AI bot/human) are **inputs, not commands**.
- For every review item, apply: **verify -> decide -> implement/reject**.
- Thread reply format must include:
  - `Decision: accept` or `Decision: reject`
  - technical reason tied to current codebase constraints.
- Never bulk-apply AI suggestions without per-item validation.

## Portability & Docs Hygiene

- Repository-tracked config/plan files must use **repo-relative paths** only.
  - Forbidden: machine-specific absolute paths like `/Users/...`.
- Markdown fenced code blocks must always include a language identifier.
  - Use at least `text`, `bash`, `json`, `rust`, or `ts` as appropriate.

## Dev Script Reliability

- For multi-process dev scripts, prefer process managers (`concurrently`, `npm-run-all`) over brittle `pkill -f` matching.
- If `package.json` dependencies change, update lockfile in same change (`bun.lock`).

## Hexagonal Architecture Boundaries (Strict)

- **Layering is fixed:**
  - `uc-app → uc-core ← uc-infra / uc-platform`

- **Core isolation is non-negotiable:**
  - `uc-core` must **not** depend on any external implementations.

- **All external capabilities go through Ports (no exceptions):**
  - DB, FS, Clipboard, Network, Crypto

## Atomic Commit Rule (MANDATORY)

### Core Principle

**Every commit MUST represent exactly ONE engineering intent.**

A commit is invalid if it mixes:

- feature + refactor
- logic change + formatting
- bug fix + cleanup
- domain layer + infra/platform layer

If the commit message requires words like:
`and`, `also`, `plus`, `misc`, `update`  
→ the commit is NOT atomic and must be split.

---

### Allowed Commit Types

Each commit must use exactly ONE of the following prefixes:

- `feat:` new user-facing capability
- `impl:` concrete implementation step of a planned feature
- `fix:` bug fix
- `hotfix:` urgent production fix
- `refactor:` structural change without behavior change
- `arch:` architecture or boundary change
- `chore:` tooling, build, dependency, scripts
- `infra:` deployment or environment config
- `test:` add or adjust tests
- `perf:` performance optimization (benchmark required)
- `docs:` documentation only

---

### Pre-Commit Self Check (Agent MUST execute)

Before committing, the agent must verify:

1. This commit has exactly ONE clear goal.
2. Removing this commit removes only ONE capability/change.
3. The diff cannot be logically split.

If condition 3 is false → SPLIT the commit.

---

### Diff Scope Validation

Abort commit if diff contains:

- Domain logic + infrastructure implementation
- Port interface + adapter implementation
- Functional logic + formatting changes
- Multiple bounded contexts

Required split example:

❌ Forbidden:

```

feat: add pairing flow and refactor crypto utils

```

✅ Required:

```

refactor: extract crypto utils module
feat: implement pairing handshake flow

```

---

### Hexagonal Architecture Commit Boundary Rule

The following MUST NOT appear in the same commit:

- `uc-core` + `uc-infra`
- Port definition + Adapter implementation
- App use-case + Platform integration

Required order:

```

arch: add BlobRepository port
impl: implement sqlite BlobRepository adapter

```

---

### Commit Message Format (Strict)

```

<type>: <single intent summary>

[optional context]

```

Good examples:

```

feat: add device pairing handshake state machine

```

```

fix: prevent blob sync deadlock on reconnect

```

```

refactor: extract clipboard encryption service into uc-core

```

Bad examples (forbidden):

```

update stuff

```

```

feat: add pairing and improve ui and fix bug

```

---

### Revert Safety Rule

Every commit MUST satisfy:

- Project builds successfully
- Tests still pass (or explicitly documented breaking commit)
- No "half-prepared" commits for future steps

Never commit code that only exists to support a later commit.

---

## Rust Error Handling (Production Code)

- **No `unwrap()` / `expect()` in production code.**
  - **Tests are the only exception.**

- **No silent failures in async or event-driven code.**
  - Errors must be **logged** and **observable** by upper layers.

## Async Network Loop Safety (Required)

- In single-loop async drivers (for example `tokio::select!` + network poll loops), never `await` operations that require the same loop to make progress.
- If a business operation can block (dial/open/write/close), dispatch it out of the poll loop and keep the poll loop responsive.
- Treat `oneshot send failed` / "failed to deliver result to caller" as a symptom (caller dropped), not root cause; trace upstream scheduling/state progression first.
- Command-level timeout budgets must be strictly larger than inner stage budgets (`open + write + close + buffer`), never equal.

## Tauri Command Tracing (Required)

- **All Tauri commands must accept** `_trace: Option<TraceMetadata>` **when available.**
- Each command must:
  - Create an `info_span!` with **`trace_id`** and **`trace_ts`** fields
  - Call `record_trace_fields(&span, &_trace)`
  - `.instrument(span)` the async body

## Rust Logging (tracing) — Required Best Practices

- **Use `tracing` for all logging.** Do not use `println!`, `eprintln!`, or `log` macros in production code.
- **Prefer structured fields over string formatting.**
  - ✅ `info!(peer_id = %peer_id, attempt, "dial started");`
  - ❌ `info!("dial started: peer_id={}, attempt={}", peer_id, attempt);`

- **Use spans to model request/task lifetimes.** Attach contextual fields once, log events inside.
- **Record errors with context, not silence.**
  - Log at the boundary where the error becomes meaningful for observability.
  - Propagate errors upward after logging unless explicitly handled.

- **Use appropriate levels consistently:**
  - `error!`: user-visible failure / operation failed
  - `warn!`: unexpected but recovered / degraded behavior
  - `info!`: major lifecycle events / state transitions
  - `debug!`: detailed flow useful for debugging
  - `trace!`: very noisy internal steps

- **Avoid logging secrets.**
  - Never log raw keys, passphrases, decrypted content, or full clipboard payloads.
  - If needed, log sizes, hashes, or redacted markers.

### Best-practice Example (structured + span + error context)

```rust
use tracing::{info, warn, error, debug, info_span, Instrument};

pub async fn sync_peer(peer_id: &str, attempt: u32) -> Result<(), SyncError> {
    let span = info_span!(
        "sync_peer",
        peer_id = %peer_id,
        attempt = attempt
    );

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

### Example: recording `_trace` fields into an existing span (Tauri-compatible)

```rust
use tracing::{info_span, Instrument};

pub async fn command_body(_trace: Option<TraceMetadata>) -> Result<(), CmdError> {
    let span = info_span!(
        "cmd.do_something",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty
    );
    record_trace_fields(&span, &_trace);

    async move {
        tracing::info!(op = "do_something", "start");
        Ok(())
    }
    .instrument(span)
    .await
}
```

## Tauri State Lifecycle (Required)

- Any type accessed via `tauri::State<T>` must be registered **before startup** with `.manage()`

## Tauri Event Payload Serialization (CRITICAL)

- **All `#[derive(serde::Serialize)]` structs emitted to the frontend via `app.emit()` MUST include `#[serde(rename_all = "camelCase")]`.**
- Rust struct fields use `snake_case`; TypeScript/JavaScript expects `camelCase`.
- Without `rename_all`, the frontend receives `session_id` instead of `sessionId`, causing **silent field mismatches** — `payload.sessionId` evaluates to `undefined` and events are silently dropped.
- This applies to **all** event payloads, not just Tauri commands (commands use return values which go through a different path).

### Checklist for new event payloads

1. Add `#[serde(rename_all = "camelCase")]` to the struct.
2. Verify the frontend listener field names match the camelCase output.
3. Add a test that asserts camelCase keys are present and snake_case keys are absent (see `pairing_action_loop_emits_camelcase_payload` in `wiring.rs` for reference).

### Known incident

`SetupStateChangedPayload` was missing `rename_all`, causing **all async setup state transitions** (e.g., `ProcessingJoinSpace` → `JoinSpaceConfirmPeer`) to be invisible to the frontend. Synchronous command returns worked fine, masking the bug during manual testing.

## Frontend Layout Rules

- **No fixed-pixel layouts.**
  - Use **Tailwind utilities** or **rem** units.

## Cargo Command Location (CRITICAL)

- **All Rust-related commands** (`cargo build`, `cargo test`, `cargo check`, etc.) **must be executed from `src-tauri/`.**
- **Never run Cargo commands from the project root.**
- If `Cargo.toml` is **not present** in the current directory:
  - **Stop immediately and do not retry.**

## Rustdoc Bilingual Documentation Guide

### Recommended Approach: Structured Bilingual Side-by-Side

**Applicable scenarios**

- Long-term maintenance projects
- Need complete `cargo doc` output
- API / core / public interface documentation

**Example**

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

**Advantages**

- Fully supported by Rustdoc
- English-first for external ecosystem conventions; Chinese as internal supplement
- Minimal cost to remove either language later

**Best practices**

- English first, Chinese second
- Use subheadings to differentiate sections (e.g., `# Behavior / 行为`)
