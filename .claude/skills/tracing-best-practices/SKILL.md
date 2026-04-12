---
name: tracing-best-practices
description: TRIGGER when writing, modifying, or reviewing any Rust code that involves tracing, logging, spans, events, #[instrument], tracing::info/debug/warn/error, subscriber initialization, or tokio::spawn with context propagation. Enforces project tracing standards.
---

# Tracing Best Practices

## When to Apply

This skill MUST be followed whenever you:

- Add or modify `#[instrument]` attributes
- Write `tracing::info!`, `tracing::debug!`, `tracing::warn!`, `tracing::error!`, `tracing::trace!` events
- Create or modify spans (`info_span!`, `debug_span!`, etc.)
- Write `tokio::spawn` or any async task spawning
- Modify subscriber/tracing initialization code
- Handle errors at boundary layers
- Write state machine event handlers
- Add IPC/daemon request handlers

---

## 1. Core Concepts

tracing records three types of structured data — NOT print text:

- **Span**: A time range (request, usecase execution, state machine event handler)
- **Event**: A discrete fact at a single moment ("received pairing proof", "db write failed")
- **Field**: Structured key-value data (`trace_id`, `device_id`, `session_id`, `state`, `elapsed_ms`)

### Design Principles

- Spans = flow boundaries; Events = facts within flows
- **Every span MUST contain at least one event** — `#[instrument]` alone only creates a span; `tracing_subscriber::fmt` only outputs **events**. A span with no events inside it produces zero log output, making the function invisible in logs.
- Fields first, messages second
- Stable field names over ad-hoc description strings
- Record business context, never sensitive plaintext
- Call chains connect via span hierarchy, not string search
- Default to info/debug-safe for production

---

## 2. Where Spans Are REQUIRED

### 2.1 Entry Layer (MUST have top-level span)

Applies to: Tauri commands, CLI handlers, HTTP/IPC handlers, background task entries, scheduled task entries.

```rust
#[instrument(
    name = "cmd.get_clipboard_items",
    level = "info",
    skip(runtime),
    fields(trace_id = %uuid::Uuid::new_v4())
)]
pub async fn get_clipboard_items(runtime: State<'_, AppRuntime>) -> Result<Vec<Item>, String> {
    // ...
}
```

Requirements:

- Every entry function MUST have a top-level span
- MUST include `trace_id` or equivalent request identifier
- MUST include business object IDs (`session_id`, `device_id`, `space_id`)

### 2.2 UseCase / Orchestrator Layer (MUST have span)

Applies to: `start_join_space`, `submit_passphrase`, `persist_entry`, `handle_event`, `sync_once`, etc.

```rust
#[instrument(
    name = "space_access.submit_passphrase",
    level = "info",
    skip(self, passphrase),
    fields(session_id = %self.session_id, device_id = %self.device_id, state = ?self.state)
)]
async fn submit_passphrase(&mut self, passphrase: String) -> Result<()> {
    // Span name reflects business action, not technical detail
}
```

### 2.3 External Boundaries (at least event, preferably span)

Applies to: DB read/write, file I/O, network send/receive, subprocess calls, encryption boundaries, WebSocket/libp2p/relay/RPC.

- MUST record: target object, result, elapsed time, error code/kind
- Large payloads MUST NEVER be logged directly

### 2.4 Async Spawn (MUST propagate span)

```rust
// CORRECT - Propagate current span
tokio::spawn(task().in_current_span());

// CORRECT - Create specific span for spawned work
let span = tracing::info_span!("pairing_session", session_id = %session_id);
tokio::spawn(task.instrument(span));
```

Long-lived background loops MUST create child spans or events per iteration.
Channel/callback boundaries MUST re-attach message IDs into new span context.

### 2.5 Async Span Lifecycle — Three Correct Patterns

#### Core Rule: NEVER use `.entered()` in async functions

`EnteredSpan` contains `*mut ()` → not `Send` → holding it across `.await` makes the future non-`Send` → `tokio::spawn` / `JoinSet::spawn` will fail to compile.

#### Pattern 1: Function-level — `#[instrument]` (preferred)

Best for: standalone async functions where parameters can be skipped.

```rust
#[instrument(skip_all, fields(session_id = %session_id, peer_id = %peer_id))]
async fn handle_message(session_id: &str, peer_id: &str, msg: Message) -> Result<()> {
    info!("received message");  // automatically under the span
    do_something().await;       // await-safe
}
```

#### Pattern 2: Spawn-level — `.instrument(span)`

Best for: futures passed to `tokio::spawn` / `JoinSet::spawn`.

```rust
let span = tracing::info_span!("pairing.action_loop");
tasks.spawn(run_action_loop(rx, cancel).instrument(span));
```

#### Pattern 3: Inline async block — `async { }.instrument(span).await`

Best for: match arms, if-branches, or other blocks that need a local span with `.await` inside.

```rust
match event {
    Event::Succeeded { session_id } => {
        let span = info_span!("pairing.session", session_id = %session_id);
        async {
            info!(event = "succeeded");    // under the span
            notify_peer().await;           // await-safe
        }.instrument(span).await;
    }
}
```

#### FORBIDDEN Patterns

```rust
// ❌ Compile error — EnteredSpan is not Send
let _guard = info_span!("my_span").entered();
something.await;  // _guard held across await

// ❌ Same problem, different syntax
let span = info_span!("my_span");
let _guard = span.enter();
something.await;  // _guard still held across await
```

---

## 3. Where NOT to Use `#[instrument]`

### Pure utility functions — NO span

`hash_bytes`, `normalize_path`, `parse_header`, `to_png` — no business semantics, no span. Adds noise.

### High-frequency hot paths — NO span

Tight loops, per-poll/tick functions, per-item iteration helpers. Use sampled events or aggregate stats instead.

### Functions with sensitive/large parameters — MUST skip

`#[instrument]` records params via Debug by default. Passwords, tokens, ciphertext, large blobs WILL leak unless explicitly `skip()`-ed.

---

## 4. `#[instrument]` Standard Template

```rust
#[instrument(
    name = "space_access.submit_passphrase",  // Stable name — survives refactors
    level = "info",                            // Explicit default level
    skip(self, passphrase),                    // Skip sensitive/large params
    fields(
        session_id = %self.session_id,         // Key business context
        device_id = %self.device_id,
        state = ?self.state
    )
)]
async fn submit_passphrase(&mut self, passphrase: String) -> Result<()> { ... }
```

### `skip` Rules — ALWAYS skip:

- `self` (unless Debug is very light and valuable)
- All secrets: `password`, `passphrase`, `token`, `secret`
- Large collections, binary data, raw request/response bodies
- Sensitive DTOs, large `Arc<AppState>` / runtime / container

### `%` vs `?` Selection:

- `%field` (Display): stable, short, search-friendly fields
- `?field` (Debug): enums, struct summaries, diagnostic detail

### Return Values:

Do NOT automatically record full return values. Log key results as separate events:

```rust
tracing::info!(session_id = %session_id, result = "accepted", "space access completed");
```

### CRITICAL: `#[instrument]` Requires Events Inside the Function Body

`#[instrument]` generates a **span**, not an event. `tracing_subscriber::fmt` only writes **events** to log output (console/JSON file). A function with only `#[instrument]` and no event macros (`info!`, `debug!`, etc.) produces **zero log output** — the function is completely invisible in logs.

**Every `#[instrument]`-annotated function MUST emit at least one tracing event.** Minimum pattern:

```rust
#[instrument(name = "api.search_query", level = "info", skip(state, params), fields(query = %params.query))]
async fn search_query_handler(state: State, params: Query) -> Result<Json<Response>, Error> {
    // ... business logic ...
    let result = do_search().await?;
    // At minimum, log the outcome — this makes the function visible in logs
    info!(total = result.total, "search completed");
    Ok(Json(result))
}
```

For thin delegation functions (usecases that just call a port), a single `debug!` after the call is sufficient:

```rust
#[tracing::instrument(name = "usecase.index_entry.execute", skip(self, doc, postings), fields(entry_id = %doc.entry_id))]
pub async fn execute(&self, doc: SearchDocument, postings: Vec<SearchPosting>) -> Result<(), SearchError> {
    self.search_index.index_entry(doc, postings).await?;
    tracing::debug!("entry indexed successfully");
    Ok(())
}
```

---

## 5. Event Standards

Events record facts within a span. Message MUST be short; fields carry the data.

### Success Event

```rust
tracing::info!(
    session_id = %session_id,
    peer_id = %peer_id,
    attempt = retry_count,
    "relay connection established"
);
```

### Failure Event

MUST include: `error_kind` (stable category), `error` or `source = ?err`, retryability flag, key context IDs.

```rust
tracing::error!(
    session_id = %session_id,
    error_kind = "proof_verification_failed",
    retryable = false,
    error = %err,
    "space access failed"
);
```

### State Machine Transition Event

```rust
tracing::debug!(
    session_id = %session_id,
    event = "ReceivedProof",
    from_state = ?old_state,
    to_state = ?new_state,
    "state transition"
);
```

---

## 6. Field Naming Convention

Field names MUST be stable (once shipped, avoid renaming). Use `snake_case`.

### Common Fields (use consistently):

| Field                               | Purpose                            |
| ----------------------------------- | ---------------------------------- |
| `trace_id`                          | Cross-boundary request correlation |
| `request_id`                        | Per-request identifier             |
| `session_id`                        | Pairing/space session              |
| `task_id`                           | Background task identifier         |
| `device_id`                         | Device identifier                  |
| `space_id`                          | Space identifier                   |
| `peer_id`                           | Network peer                       |
| `user_action`                       | What the user triggered            |
| `state` / `from_state` / `to_state` | State machine context              |
| `elapsed_ms`                        | Duration                           |
| `retry_count`                       | Retry attempts                     |
| `error_code`                        | Business error code                |
| `error_kind`                        | Stable error classification        |

### Error Field Breakdown — never log as single string blob:

- `error`: human-readable summary
- `error_kind`: stable classification
- `error_code`: business error code
- `source`: originating module
- `retryable`: whether retry makes sense

### FORBIDDEN as field values:

- Plaintext passwords/tokens/secrets
- Complete request/response bodies
- Large binary content
- User clipboard plaintext
- Full configuration objects

When debugging, log: length, hash, summary, or object ID instead.

---

## 7. Level Usage

| Level     | Meaning                                              | Examples                                                         |
| --------- | ---------------------------------------------------- | ---------------------------------------------------------------- |
| **ERROR** | Unrecoverable, or main flow result affected          | Decryption failed, DB corruption, illegal state transition       |
| **WARN**  | Abnormal but system continues, or fallback triggered | Relay failed -> switched backup, invalid config -> using default |
| **INFO**  | Important business milestones (safe for production)  | Space join success, pairing established, sync start/complete     |
| **DEBUG** | Development/troubleshooting context                  | State machine event received, branch selection, retry parameters |
| **TRACE** | Ultra-fine internal behavior (local debugging only)  | Per protocol frame, per loop iteration, per poll                 |

---

## 8. Error Handling + Tracing Rules

### Rule 1: Never "just return err" on critical failure paths

At least one error event MUST exist at the boundary where the error is discovered.

### Rule 2: Don't repeat-bomb the same error up the stack

```rust
// WRONG - Same error logged at 5 stack levels
error!("failed: {}", err);        // layer 1
error!("op failed: {}", err);     // layer 2
error!("handler failed: {}", err); // layer 3

// CORRECT - Full detail at boundary, summary at top
// At discovery boundary:
tracing::error!(error_kind = "db_write_failed", error = %err, entry_id = %id, "persist failed");
// Upper layer: just propagate via ? or log only business outcome
```

### Rule 3: Timeout, cancel, and retry are distinct event types

Don't lump them with generic failures. They need separate classification for operational statistics.

---

## 9. State Machine Tracing (Project Priority)

State machines are the MOST important tracing target in this project.

### Every event handling gets a span

```rust
#[instrument(
    name = "space_access.handle_event",
    level = "debug",
    skip(self, event),
    fields(session_id = %self.session_id, state = ?self.state)
)]
async fn handle_event(&mut self, event: Event) -> Result<()> { ... }
```

### Every state transition gets a structured event

Fields: `event`, `from_state`, `to_state`, `reason`, `session_id`.

### Timeout / cancel / retry — separate event types

Never lump with generic failures.

---

## 10. IPC / Daemon Tracing

### Request-level span REQUIRED

Fields: `trace_id`, `request_id`, `route`/`command`, `client`/`source`, `session_id`.

### Cross-process trace_id

GUI sends `trace_id` with each daemon request; daemon creates span with same ID. Connects "frontend click -> IPC -> usecase -> infra" into one trace.

### Never log sensitive body content

Record length, type, or object ID only.

---

## 11. Subscriber Initialization

### Single initialization entry point ONLY

`observability::init_tracing()` / `bootstrap::init_observability()`. No module may independently initialize a subscriber.

### Default filter

`info` baseline, own crates at `debug`, noisy third-party crates at `warn`. Uses `EnvFilter` with directives.

### File output

- Production MUST use non-blocking writer (`tracing_appender::non_blocking`)
- WorkerGuard MUST be held for process lifetime
- Rolling file strategy: daily (dev), daily/hourly with size limits (prod)

---

## 12. Code Review Checklist

When writing or reviewing tracing code, verify:

| Check                                                    | Rule          |
| -------------------------------------------------------- | ------------- |
| Entry function has span?                                 | **MUST**      |
| Key usecase/orchestrator has span?                       | **MUST**      |
| `#[instrument]` function has at least one event inside?  | **MUST**      |
| `tokio::spawn` propagates span?                          | **MUST**      |
| Sensitive params use `skip()`?                           | **MUST**      |
| Errors have structured fields (not just string)?         | **MUST**      |
| Field names follow project convention?                   | **MUST**      |
| Subscriber init centralized?                             | **MUST**      |
| File output holds WorkerGuard?                           | **MUST**      |
| High-frequency function avoids needless `#[instrument]`? | **SHOULD**    |
| Span names stable and business-oriented?                 | **SHOULD**    |
| `info` level safe for long-term production?              | **SHOULD**    |
| State machine transitions use structured events?         | **SHOULD**    |
| Same error repeated across stack layers?                 | **FORBIDDEN** |
| Secret/large payload in tracing output?                  | **FORBIDDEN** |
| `.entered()` / `.enter()` held across `.await` in async? | **FORBIDDEN** |
| `#[instrument]` with no events inside (silent span)?     | **FORBIDDEN** |
