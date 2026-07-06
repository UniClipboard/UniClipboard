# Findings: MobileSyncEngine (push/pull SDK) — PR-A

## Source of truth
Design doc: `.planning/2026-07-05-mobile-push-pull-sdk-design.md` (v0.2, all grilling Q1-Q10
decided). User explicitly approved skipping the codex adversarial review gate and proceeding
straight to PR-A implementation (2026-07-05).

Scope: `crates/uc-mobile` only. Desktop (`uc-application`) untouched. RN-side wiring is a
separate repo (`uniclipboard-android`) — out of scope here, PR-C produces only the handoff doc.

## Recon: current code state (verified 2026-07-05, before writing any engine code)

### 1. `crates/uc-mobile-proto/src/sync_engine.rs` — pure reducer
No single `tick` fn exists — the design's "internal sync-tick" is a NEW composition the engine
must build by calling the existing decomposed steps in sequence:
- `plan_preamble(&mut SyncRuntimeState, &PreambleSnapshot) -> Preamble` (:218)
- `plan_after_server_get(&SyncRuntimeState, &ServerGetSnapshot) -> ServerRoute` (:335) →
  `Converged{server_hash}` / `ServerNew(ServerNewPlan)` / `Push(PushDecision)`
- Commit family (all `&mut SyncRuntimeState`): `commit_converged` (:481), `commit_apply` (:497,
  returns `CommitOutcome`), `commit_apply_failed` (:516), `commit_stage` (:524), `commit_push`
  (:542, returns `CommitOutcome` — does NOT set `last_applied_hash`, matches Q2), `commit_push_skipped`
  (:568), `commit_consent_push` (:577 — DOES set `last_applied_hash`, unlike commit_push),
  `commit_tick_success`/`commit_tick_failure`/`commit_history_sync_done`.
- `mark_staged_applied`, `acknowledge_loop_detection`, `reset_runtime_state`,
  `handle_active_server_changed` (:743, full reset + clears both watermark fields),
  `handle_network_route_changed` (:754, clears only backoff fields).

`SyncRuntimeState` (:92): `state, last_synced_hash, last_synced_content_id, last_applied_hash,
loop_events, staged_server_hash, staged_content_id, staged_entry, consecutive_failures,
next_attempt_ms, last_history_sync_ms`.

**`SyncSettings` does NOT exist today.** `auto_push`/`auto_apply` are today raw bool fields on
`PreambleSnapshot`/`ServerGetSnapshot` (the input snapshots), not a named settings struct. The
engine introduces `SyncSettings{auto_apply}` as a genuinely new type and must synthesize these
snapshot bools internally (push always constructs snapshots the way `commit_push`'s semantics
require; `auto_push` as a snapshot field becomes an internal engine-only detail, never client-facing).

**No `PullTrigger`-like enum exists anywhere** — wholly new in the engine.

`loop_guard.rs`: `LoopDirection{Pulled,Pushed}`, `record(...)`, `tripped(...)`,
`DEFAULT_WINDOW_SECS=30.0`, `DEFAULT_FLIP_THRESHOLD=3`. Reusable as-is.

`commit_push` vs `commit_apply` asymmetry (already implements Q2/Q5, nothing new needed here):
`commit_push` → `advance_synced(st, Some(h), None)` (content_id cleared, server doesn't know
identity yet), does not touch `last_applied_hash`, doesn't clear staged. `commit_apply` →
`advance_synced(st, hash, content_id)` + sets `last_applied_hash` + clears all staged fields +
records `Pulled` loop-guard event.

### 2. `crates/uc-mobile-proto/src/persist_keys.rs` — ⚠️ discrepancy found
Design §7 claims the engine persists "`LAST_SYNCED_HASH` / `LAST_SYNCED_CONTENT_HASH`(contentId)"
via the new `KeyValueStore` port. **This is wrong.** Current constants:
```rust
pub mod keys {  // UserDefaults-backed
    LAST_SYNCED_CONTENT_HASH = "last_synced_content_hash"  // legacy PRE-FILE-MIGRATION home
                                                             // of the plain HASH watermark,
                                                             // per this file's own doc-comment
    HISTORY_MODIFIED_AFTER, LAST_HISTORY_SYNC_AT, ...
}
pub mod files {  // App-Group file-backed, cross-process-fresh (share extension reads/writes these)
    LAST_SYNCED_HASH = "last_synced_hash"
    LAST_KNOWN_SSID, LIVE_URLS
}
```
There is **no key constant for the content_id watermark at all**. Cross-checked against
`.planning/2026-06-23-contentid-mobile-dedup-design.md:108-109`, which specified the App Group
key as **`lastSyncedContentId`** (camelCase — native/TS-side naming) when `last_synced_content_id`
was added to `SyncRuntimeState` — but this key was apparently never mirrored into
`persist_keys.rs`'s pinned-constants registry (that design predates this file's `files::` module
existing as the canonical shared registry, or the RN side manages it independently).

**Resolution (my call, documented here since I can't see the RN repo to verify)**: add a NEW
constant to `persist_keys.rs`'s `files::` module (same cross-process-fresh tier as
`LAST_SYNCED_HASH`, since they're always advanced together via `advance_synced`):
```rust
pub const LAST_SYNCED_CONTENT_ID: &str = "last_synced_content_id";
```
Do NOT reuse `LAST_SYNCED_CONTENT_HASH` (keys:: module) — it is a different, legacy field.
This is called out explicitly in the PR-C handoff doc as a **new key the native `KeyValueStore`
implementation must wire up** — it has no pre-existing native writer to be compatible with,
unlike `LAST_SYNCED_HASH`.

### 3. `crates/uc-mobile/src/reducer.rs` — FFI mirror of the reducer (existing pattern, not touched)
Per-function FFI mirror, state crosses FFI by value (`SyncRuntimeState` FFI record in/out of every
call, bundled in `*Step` records). This is exactly the pattern the new `MobileSyncEngine`
replaces with a long-lived stateful `Object`. The engine's internals call
`uc_mobile_proto::sync_engine::*` directly (same as `reducer.rs` does) — not through this FFI layer.

### 4. `crates/uc-mobile/src/client.rs` — `MobileSyncClient` (reused as-is)
`#[derive(uniffi::Object)]`, constructed via `new(bridge: Arc<dyn PlatformBridge>, trust_insecure_cert: bool)`.
Async methods (all reused by the engine): `get_latest(server) -> Result<ClipboardMeta, SyncError>` (:620),
`put_clipboard(server, meta, payload: Option<Vec<u8>>)` (:632, always real upload), `put_file` (:667),
`get_file(server, name) -> Vec<u8>` (:681), `query_history`, `get_history_payload`,
`start_sse_subscription(server, listener) -> Arc<SseHandle>` (:954, sync, spawns detached task).
`SyncError` (:328): `NotInitialized, InvalidInput, Network, Unauthorized, NotFound, ServerError,
ProtocolError, DecodingFailed, Cancelled, Internal` — reused as `SyncOutcome::Failed{error}`'s type,
no new error enum needed.
`ServerConfig` (:127): `{base_url, username, password}`.
`ClipboardKind` exists in TWO places by design: `uc_mobile_proto::ClipboardKind` (wire codec) and
`uc_mobile::client::ClipboardKind` (FFI mirror, re-exported at crate root) — `LocalContent` reuses
the FFI one, not a new type.
`ClipboardMeta` (:174) is the closest existing analog to `SyncedMeta` but server-wire-shaped
(`kind, text, data_name, has_data, size, hash, content_id`) — `SyncedMeta`/`LocalContent` are
narrower, purpose-built new types, not renames.

**Test harness `spawn_mock`/`MockConfig`** (`client.rs` `#[cfg(test)] mod tests`, currently private
to that module) supports: forced HTTP status, per-route delays, canned clip/history/file bytes,
`available_snapshot_hashes` (content-availability route — dies with Phase 6), ordered `events` log
for asserting request sequencing. **Needs to become `pub(crate)`-reachable** for the new engine's
test module to reuse rather than duplicating a second mock server.

### 5. uniffi scaffolding (`crates/uc-mobile/src/lib.rs`)
Proc-macro mode, no `.udl`. `uniffi::setup_scaffolding!()` at :30. Existing `Object`s: only
`MobileSyncClient` and `SseHandle` — `MobileSyncEngine` will be the third, in a new module
(`crates/uc-mobile/src/engine.rs`).

### 6. `with_foreign` pattern — already proven twice in this crate, not a new risk
`PlatformBridge` (`client.rs:425`, `#[uniffi::export(with_foreign)] trait PlatformBridge { fn app_group_dir(&self) -> String; }`)
and `SseListener` (`client.rs:1070`) both already use `#[uniffi::export(with_foreign)]` as
constructor/method `Arc<dyn Trait>` arguments (required per uniffi-rs#2797 — plain
`callback_interface` traits can't be constructor args). The new `KeyValueStore` trait follows an
already-working mechanical pattern in this exact crate:
```rust
#[uniffi::export(with_foreign)]
pub trait KeyValueStore: Send + Sync {
    fn get(&self, key: String) -> Option<Vec<u8>>;
    fn set(&self, key: String, value: Vec<u8>);
    fn remove(&self, key: String);
}
```
`PlatformBridge::app_group_dir()` today returns ONLY a directory path — no actual file I/O port
exists in Rust yet (confirmed; matches design §2.2). `KeyValueStore` is the first real
persistence port.

### 7. Test conventions
`#[tokio::test]` throughout for async (`MobileSyncClient` methods); plain `#[test]` for the
synchronous reducer/reducer.rs mirror tests. `tests/init_gate.rs` is a separate integration test
binary that must stay alone (process-wide `uc_mobile_init()` `OnceLock` isolation) — not relevant
to engine tests, which can live in `client.rs`-adjacent unit test modules.
Canonical per-test shape: `spawn_mock(MockConfig{..})` → construct client → call → assert on
`Result` + `state.events()` ordered log.

### 8. Cargo dependencies — no Cargo.toml changes needed
`uc-mobile` already depends on `uc-content-hash` (path dep, kept per design §12 for the engine's
internal snapshot-hash use) and `tokio` with the `sync` feature already enabled — `tokio::sync::Mutex`
for the §6.5 "one async lock serializes push/pull/apply_staged" requirement is available with zero
manifest changes. `uc-mobile-proto` stays uniffi-free/sync (no tokio dep) — correct, not touched.

### 9. No pre-existing overlapping types
Grepped both crates for `SyncOutcome|UpToDateReason|ContentAvailable|SyncedMeta|SyncSettings|
PullTrigger|LocalContent|apply_staged|KeyValueStore` — zero hits. Every type in design §4 is
genuinely new; nothing to rename/reuse beyond what's noted above.

### 10. Exact deletion targets (design §6.3/§12 — "PR-A 删除")
- `compute_snapshot_hash(bytes: Vec<u8>) -> String` — free fn, `client.rs:110-113`.
- `MobileSyncClient::is_content_available(&self, server, snapshot_hash) -> Result<bool, SyncError>`
  — `client.rs:766-789`.
- Associated dead code once both are gone: `mock_content_availability` route (`client.rs:~1701`),
  `available_snapshot_hashes: HashSet<String>` field on `MockConfig` (`:1553`),
  `ContentAvailabilityDoc` struct (`:317`), tests at `:2263,2272,2280,2295,2311`.
- The server endpoint (`GET /api/mobile-sync/content-availability`, uc-webserver) stays **dormant**
  — not deleted, per design §12 (its removal is an independent follow-up).

### 11. ⚠️ Second discrepancy: device-side hash is SHA-256, not blake3 — `uc_content_hash` has no push/pull call site
Design §12 says `uc-content-hash` crate stays "引擎内部算 snapshot_hash 用" (engine uses it internally to
compute snapshot_hash). This does not actually hold for PR-A's core logic. The reducer's entire
watermark/dedup system (`hashes_equal`, `last_synced_hash`, `last_applied_hash`, `PushDecision`
routing in `plan_push`) operates on **uppercase-hex SHA-256**, via
`uc_mobile_proto::hash::sha256_hex_upper` (`hash.rs:34`) — wired through
`Clipboard::publish_text/publish_image/publish_file` (`clipboard_doc.rs:155,183,227,248`), which
already compute the hash, handle the long-text-overflow naming (`text_{HASH}.txt`), and set
`has_data`/`size` correctly. `content_id` (blake3v1, via `uc_content_hash`) is a SEPARATE identity
axis that is **always server-assigned** — `Clipboard.content_id`'s own doc-comment states "the
device never computes it" — and `commit_push`/`commit_consent_push` always pass `content_id: None`
on push (Q2, confirmed in recon #1). Mixing blake3 into the SHA-256-keyed watermark comparison
would silently break `hashes_equal` (different string shapes: 64-hex-char SHA-256 vs
`"blake3v1:<hex>"`), a real correctness bug, not a style nit.

**Resolution**: `push()` builds its wire `Clipboard`/device-hash via `Clipboard::publish_text` /
`publish_image` / `publish_file` (SHA-256), not via `uc_content_hash`. The `uc-content-hash` path
dependency in `crates/uc-mobile/Cargo.toml` is left in place per design §12's explicit instruction,
but **no engine code in PR-A calls into it** — its only two call sites (`compute_snapshot_hash`,
`is_content_available`) are exactly what Phase 6 deletes. Not adding a synthetic call site to
"use" the dependency (would be dead-code-shaped busywork); flagged here in case a future PR-B adds
a real integrity-check use for it.

### 12. Bug caught while designing Phase 7 tests: pull() leaked `ConsentMode` instead of `AlreadySynced`
`pull()` always sets `auto_push: false` on the shared `ServerGetSnapshot` so a `ServerRoute::Push`
route (reached whenever the server isn't `Converged`/`ServerNew`) is harmless — but the original
`run_route` unconditionally ran EVERY `Push(decision)` through `push_skip_reason`, and
`plan_push` checks `!snap.auto_push` FIRST, so it always produced `PushDecision::SkipConsentMode`
→ `UpToDateReason::ConsentMode` for pull's "nothing new" case. `ConsentMode` is a push-only
concept (mirrors `PushDecision::SkipConsentMode`'s doc: "consent-push mode is off") — nonsensical
leaking out of a `pull()` call, and it contradicted the design's own §9 test list, which explicitly
expects `UpToDate{AlreadySynced}` from the cross-process-pickup pull scenario. Fixed: `run_route`'s
`Push` arm now branches on `req.push_job` — `Some` (a real `push()` call) still uses the granular
`push_skip_reason`; `None` (a `pull()` call) always reports `AlreadySynced` directly, since reaching
this branch at all means `get_latest` confirmed the server already matches what's synced.

### 13. Bug caught by tests: `blocking_lock()` panics — the 4 lifecycle methods became `async`
Design's §4 interface sketch shows `set_server`/`handle_network_route_changed`/`set_settings`/
`acknowledge_loop_detected` as plain sync `fn`s (no `async`). Implemented that way using
`tokio::sync::Mutex::blocking_lock()` to access the shared `EngineState` — compiled fine, but the
`set_server_to_different_target_clears_watermark` test panicked: `"Cannot block the current thread
from within a runtime"`. `blocking_lock()` refuses to run inside a tokio task (correctly — it would
risk deadlocking a single-threaded executor), and `#[tokio::test]` wraps the whole test body in one.
There is no portable "block only if not already in a runtime" primitive. Since a real, demonstrated
panic path is worse than a minor interface deviation, all 4 methods became `pub async fn` using
`.lock().await` like `push`/`pull`/`apply_staged` — uniffi bridges an async Rust fn to a Swift
`async` / Kotlin `suspend` function automatically, so this is a mechanical adjustment for the RN
side, not a structural one. Flagged prominently in the PR-C handoff doc (Phase 8).

### 14. Test-writing lesson: a fresh engine always sees a non-empty mock clip as "server-new"
Every engine test needed to seed the `FakeStore`'s `LAST_SYNCED_HASH` to match the mock server's
configured initial `clip` — a fresh `MobileSyncEngine` starts with `last_synced_hash: None`, and
`is_already_synced` treats `None` vs any real hash as "different" unconditionally, so the very
first `push`/`pull` against an unseeded engine always routes to `ServerNew` (downloads/applies the
mock's initial content) rather than reaching the branch under test. Cost two failed test iterations
to discover (`ping_pong_same_content_trips_loop_guard`'s first push, `push_second_different_image_
truly_uploads`'s design already accounted for it) — worth calling out for anyone extending this
test module later.

## Decisions made during implementation (not in the design doc verbatim)
| Decision | Rationale |
|---|---|
| New `persist_keys::files::LAST_SYNCED_CONTENT_ID = "last_synced_content_id"` constant | Design §7's claimed key mapping doesn't match actual code (see #2 above); reusing the wrong existing constant would silently corrupt the hash watermark or fail to persist content_id at all |
| Engine's internal module lives at `crates/uc-mobile/src/engine.rs` | Matches existing one-object-per-file convention (`client.rs` for `MobileSyncClient`, presumably) |
| `spawn_mock`/`MockConfig` visibility widened to `pub(crate)` | Needed so `engine.rs`'s test module can reuse the existing mock server instead of duplicating one |
| Engine calls `uc_mobile_proto::sync_engine::*` directly, not through `reducer.rs` | `reducer.rs` is itself an FFI boundary (per-function value-passing mirror) that the engine's whole point is to replace — going through it would be a pointless extra translation layer |
| Load-before-op fold reimplemented directly against `KeyValueStore`, not via a constructed dummy `PreambleSnapshot` + `plan_preamble` call | `plan_preamble` conflates the resync-fold with `has_active_server`/paused/backoff-gate decisions and a `record_local`/`auto_push` computation the engine has no use for (history stays 100% native, Q4); reusing it would mean threading meaningless dummy fields through just to borrow two lines of fold logic — reimplementing the ~6-line fold directly is more honest about what's actually happening. The engine still separately replicates the paused-state check and (for pull) the backoff-gate check, since those ARE genuinely reused decisions (Q3) |
| `push`/`pull`'s shared route inputs bundled into a private `RouteRequest` struct | `run_route` originally took 8 positional args, tripping clippy's `too_many_arguments`; bundling also makes the push-vs-pull diff (`auto_push`/`push_job`) explicit at each call site |
| `apply_staged`'s `commit_tick_failure` call copies `guard.cfg` into a local before use | `MutexGuard`'s `DerefMut` means `&mut guard.runtime` borrows the WHOLE guard (a real method call, not plain field projection), so it doesn't disjoint-split from `&guard.cfg` in the same call the way it does for a plain `&mut EngineState` reference (which is why the same pattern compiles fine inside `run_route`/`handle_server_new`/`do_push`, which only ever receive `guard: &mut EngineState`). `se::SyncConfig` is `Copy`, so copying it out first is free |
