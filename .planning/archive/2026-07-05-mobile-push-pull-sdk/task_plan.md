# Task Plan: MobileSyncEngine push/pull SDK (PR-A)

## Goal
Implement `.planning/2026-07-05-mobile-push-pull-sdk-design.md`'s PR-A in `crates/uc-mobile`:
a new long-lived `MobileSyncEngine` uniffi Object collapsing the client-facing surface to
`push(content)` / `pull(trigger, device_hash)` (+ `apply_staged` + a few lifecycle methods),
with dedup/anti-loop/watermark/backoff/conflict-resolution fully internal. Delete the
now-superseded `is_content_available`/`compute_snapshot_hash` FFI from the same PR. Desktop
side and RN-side (`uniclipboard-android`) wiring are out of scope — only produce the Rust
engine + Rust tests + an updated RN handoff doc (PR-C).

## Background
See findings.md for the full recon trail (current reducer/client/persist_keys code state,
verified before writing engine code) and the one real discrepancy found: the design doc's §7
key-mapping table names the wrong existing constant for the content-id watermark; resolved by
adding a new `LAST_SYNCED_CONTENT_ID` key rather than reusing `LAST_SYNCED_CONTENT_HASH`.

User approved skipping the codex adversarial review gate the design doc calls for
(2026-07-05) — proceeding straight to implementation.

## Current Phase
Done (all 8 phases complete)

## Phases

### Phase 1: persist_keys addition (small, standalone commit)
- [x] Add `pub const LAST_SYNCED_CONTENT_ID: &str = "last_synced_content_id";` to
      `crates/uc-mobile-proto/src/persist_keys.rs`'s `files::` module
- [x] Add to the existing `#[cfg(test)]` literal-pinning test block
- [x] `cargo test -p uc-mobile-proto`: 281/281 pass
- **Status:** complete

### Phase 2: KeyValueStore port + new domain types
- [x] New file `crates/uc-mobile/src/engine.rs`
- [x] `#[uniffi::export(with_foreign)] trait KeyValueStore: Send + Sync { get/set/remove }`
      (mirrors `PlatformBridge`/`SseListener` pattern, `client.rs:425,1070`)
- [x] New uniffi Record/Enum types: `LocalContent`, `SyncSettings{auto_apply}`, `PullTrigger`
      (Routine/Explicit/SseHello/SseResync/SseUpdate{content_id}), `SyncOutcome` (Uploaded/
      Applied/Staged/UpToDate/BackingOff/LoopDetected/Failed), `SyncedMeta`, `UpToDateReason`,
      `StagedPreview`
- [x] Reuse existing `uc_mobile::client::ClipboardKind`, `ServerConfig`, `SyncError`, and
      `crate::reducer::SyncConfig` (constructor's `config` param) — none redefined
- [x] `cargo check -p uc-mobile`: clean
- **Status:** complete

### Phase 3: MobileSyncEngine skeleton + internal sync-tick + push()
- [x] `MobileSyncEngine` as `#[derive(uniffi::Object)]`; constructor takes
      `(server, config: reducer::SyncConfig, settings, store: Arc<dyn KeyValueStore>, client: Arc<MobileSyncClient>)`
- [x] Internal state (private `EngineState`, never crosses FFI): `tokio::sync::Mutex<EngineState>`
      holding `server`/`settings`/`cfg`/`runtime: se::SyncRuntimeState` (proto type directly, NOT
      via `reducer.rs`'s FFI mirror)
- [x] load-before-op: `fold_persisted_watermark` reads `LAST_SYNCED_HASH` / `LAST_SYNCED_CONTENT_ID`
      via `KeyValueStore`, folds into in-memory state — reimplemented directly (not by constructing
      a dummy `PreambleSnapshot` and calling `plan_preamble`, see findings.md decisions)
- [x] save-after-op: `persist_watermark`, called after every `advance_synced`-triggering commit
- [x] Internal `run_route`/`handle_server_new`/`do_push` helpers: `get_latest` (404→`None`) →
      `plan_after_server_get` → route (Converged / ServerNew / Push) — shared by push and pull via
      a `RouteRequest` bundle (Q10: push calls this FIRST, server-new wins, only then falls through
      to `put_clipboard`)
- [x] `push(&self, content: LocalContent) -> SyncOutcome`: build wire `Clipboard` + device_hash via
      `publish_text`/`publish_image`/`publish_file` (SHA-256 — NOT `uc_content_hash`/blake3, see
      findings.md #11) → run_route → server-new ⟹ `Applied`/`Staged`; else watermark/self-write
      gate ⟹ `UpToDate{...}` or full `put_clipboard` + `commit_push` + loop_guard record ⟹ `Uploaded`
- [x] `cargo check -p uc-mobile` + `cargo clippy --all-targets`: clean (fixed a `too_many_arguments`
      clippy warning by bundling push/pull's route inputs into a `RouteRequest` struct; fixed an
      E0502 borrow error in `apply_staged` — `guard.cfg`/`guard.runtime` split through a
      `MutexGuard`'s `DerefMut` doesn't disjoint-split like a plain `&mut EngineState` does, fixed
      by copying `cfg` out first, `se::SyncConfig` being `Copy`)
- **Status:** complete

### Phase 4: pull() + apply_staged()
- [x] `pull(&self, trigger: PullTrigger, current_device_hash: Option<String>) -> SyncOutcome`:
      fold → paused-state check → SSE short-circuit → `Routine` backoff gate
      (`BackingOff{retry_after_ms}`) / other triggers punch through
- [x] Truth-gate / convergence check using `current_device_hash` (Q1) — fed as `device_hash` into
      the shared `run_route`/`plan_after_server_get`, with `auto_push: false` so any Push-branch
      route (never meaningfully reachable for pull) collapses to `UpToDate{NoLocalChange}`
- [x] `auto_apply` gate: true → download + `commit_apply` ⟹ `Applied` (optimistically sets
      `last_applied_hash`, Q5) or `LoopDetected` on trip; false (or `already_staged`) → `commit_stage`
      ⟹ `Staged{preview}`
- [x] `apply_staged(&self) -> SyncOutcome`: `get_file` download (via `fetch_content`) +
      `mark_staged_applied` ⟹ `Applied`
- [x] `cargo check -p uc-mobile`: clean
- **Status:** complete

### Phase 5: Lifecycle methods + concurrency
- [x] `set_server(&self, server: ServerConfig)` — different from current ⟹ `KeyValueStore::remove`
      watermark keys + `handle_active_server_changed` (Q8); same server ⟹ no-op
- [x] `handle_network_route_changed(&self)` — clears sync-op backoff only
- [x] `set_settings(&self, settings: SyncSettings)`
- [x] `acknowledge_loop_detected(&self)` — clears loop_guard event buffer
- [x] One `tokio::sync::Mutex` serializes push/pull/apply_staged (§6.5), held load-before-op
      through save-after-op; the 4 lifecycle methods are plain sync `fn`s (per design) using
      `blocking_lock()` since they never enter the tokio runtime themselves
- [x] `cargo test -p uc-mobile --lib`: 89/89 pass (pre-existing tests, unchanged);
      `cargo check --workspace`: clean
- **Status:** complete

### Phase 6: Delete superseded FFI (is_content_available / compute_snapshot_hash)
- [x] Removed `compute_snapshot_hash` free fn + its doc comment
- [x] Removed `MobileSyncClient::is_content_available` + its doc comment
- [x] Removed now-dead: `mock_content_availability` route + router registration,
      `available_snapshot_hashes` field on `MockConfig`, `ContentAvailabilityDoc` struct + its
      `ContentAvailabilityQuery` mock-side counterpart, all 5 tests; also removed the
      now-unused `serde::Deserialize` import
- [x] Left the server endpoint (`uc-webserver`) untouched — dormant per design §12
- [x] Widened `MockConfig`/`MockState`/`spawn_mock`/`server_cfg`/`new_client` (+ the enclosing
      `mod tests`) to `pub(crate)` so `engine.rs` tests can reuse the same mock server
- [x] `cargo check -p uc-mobile --all-targets` + clippy: clean
- [x] `cargo test -p uc-mobile --lib`: 84/84 pass (89 − 5 deleted)
- **Status:** complete

### Phase 7: Engine unit tests (design §9 scenarios)
- [x] Fake in-memory `KeyValueStore` (`FakeStore`, HashMap-backed)
- [x] Enhanced the shared mock server (`client.rs`'s `MockState`) with a `current_clip` that
      `PUT /SyncClipboard.json` updates and `GET` echoes back (previously fully static — needed
      for meaningful multi-round push/pull tests) + a `set_current_clip` test-only poke hook (for
      simulating "another device changed the server" between calls); verified backward-compatible
      (all 84 pre-existing tests still pass unchanged)
- [x] Two different images pushed back-to-back → second one truly uploads (reproduces the
      original bug scenario) — `push_second_different_image_truly_uploads`
- [x] Push of previously-applied content, after other activity moved the watermark → `UpToDate
      {SelfWritten}` — `push_previously_applied_content_after_other_activity_is_self_written`
      (a direct "pull X then immediately re-push X" hits the truth-gate's `Converged`, not
      `SelfWritten` — that's the existing reducer's own tested precedence, not a bug; genuine
      `SelfWritten` needs `last_synced_hash` to have moved away from X afterward while
      `last_applied_hash` still remembers it, see findings.md)
- [x] Ping-pong push/pull same hash (7-step alternation via the `set_current_clip` hook) →
      `LoopDetected` on the 4th same-hash event (3 flips, default threshold) —
      `ping_pong_same_content_trips_loop_guard`
- [x] `SseUpdate{content_id}` matching last-synced content_id → no network call, `UpToDate
      {SseShortCircuit}` — `pull_sse_update_matching_content_id_short_circuits_without_network`
- [x] Cross-process: fake store pre-seeded with a "share-extension-written" key → load-before-op
      picks it up → `UpToDate{AlreadySynced}` —
      `pull_picks_up_cross_process_watermark_and_reports_already_synced` (surfaced and fixed a
      real bug: pull's "nothing new" case was reporting `ConsentMode` instead, findings.md #12)
- [x] Q10 stale-clobber: local candidate X + server already Y (newer) → `push(X)` returns
      `Applied{Y}`, mock never receives X's upload —
      `push_yields_to_newer_server_content_instead_of_clobbering`
- [x] Backoff: consecutive failures → `pull(Routine)` returns `BackingOff`; `pull(Explicit)`
      punches through regardless — `pull_routine_backs_off_but_explicit_punches_through`
- [x] `set_server` → watermark keys removed → next pull applies the new server's content cleanly
      — `set_server_to_different_target_clears_watermark` (caught a real bug: `blocking_lock()`
      panics inside `#[tokio::test]`'s runtime context, findings.md #13 — the 4 lifecycle methods
      became `async fn`)
- [x] `cargo test -p uc-mobile --lib`: 92/92 pass (84 pre-existing + 8 new);
      `cargo clippy -p uc-mobile --all-targets`: clean
- **Status:** complete

### Phase 8: Workspace verification + RN handoff doc (PR-C)
- [x] `cargo test -p uc-mobile-proto -p uc-mobile --lib`: 281/281 + 92/92 pass
- [x] `cargo clippy -p uc-mobile-proto -p uc-mobile --all-targets`: clean
- [x] `cargo check --workspace`: clean
- [x] `cargo test --workspace --no-fail-fast`: green except 3 pre-existing, unrelated baseline
      failures (`uc-platform --lib` effective_mime, `uc-platform --doc`, `uc-daemon-process --doc`
      — all documented pre-existing debt, crates untouched by this work) + one confirmed-flaky
      test-isolation issue in `uc-app-paths` (passes reliably in isolation/single-threaded;
      crate untouched)
- [x] Wrote RN handoff doc `.planning/2026-07-05-mobile-push-pull-sdk-rn-integration-guide.md`:
      `push`/`pull(trigger)`/`apply_staged` call sequence, `KeyValueStore` implementation contract
      (prominently flags the NEW `last_synced_content_id` App Group key — no pre-existing native
      writer, unlike `last_synced_hash`), the lifecycle-methods-are-async deviation, migration off
      "drive reducer function-by-function"
- [x] Marked `2026-07-05-content-availability-rn-integration-guide.md` as superseded (banner at top)
- [x] Updated the design doc's own status header (v0.2→v0.3) to record PR-A as implemented +
      the two discrepancies found during implementation
- **Status:** complete

## Key Questions
1. Content-id persistence key name/tier — resolved in findings.md (`LAST_SYNCED_CONTENT_ID` in
   `files::`, new constant, flagged to RN team as new).
2. PR-B (uc-mobile-proto tweaks) — design predicts zero changes needed; confirm during Phase 3
   whether the existing `plan_*`/`commit_*` signatures are sufficient as-is for the internal
   sync-tick, or whether a small reducer-side addition becomes necessary.

## Decisions Made
See findings.md "Decisions made during implementation" table.

## Errors Encountered
| Error | Attempt | Resolution |
|-------|---------|------------|
|       | 1       |            |

## Notes
- Commit boundaries roughly follow phases 1 / 2-5 (engine) / 6 (deletion) / 7 (tests) / 8 (docs)
  — confirm final split with user before committing, per this repo's atomic-commit convention.
- Re-read this plan before major decisions; update phase status as you progress.
- Log ALL errors — they help avoid repetition.
