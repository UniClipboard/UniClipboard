# Task Plan: uc-content-hash crate + reliable mobile-sync upload dedup

## Goal
Fix the mobile-sync bug where the RN client's `putContent`/`getRecord` existence check
silently skips uploading real content (data loss) by: (1) extracting the snapshot-hash
algorithm into a shared zero-dependency crate `uc-content-hash`, (2) making uc-core use it
(pure refactor), (3) adding a mobile-only content-availability endpoint backed by real
DB+FS availability checks, and (4) exposing a reliable `is_content_available` probe (+
optional `put_clipboard_deduped` convenience) from `uc-mobile` for the RN team to integrate.

## Background (why this matters)
- Root cause: `GET /api/history/{profileId}` (`crates/uc-webserver/src/mobile_lan/routes/history.rs`)
  deliberately allows hash drift for Image/File types (server may re-encode images), but the
  RN client's `putContent`/`getRecord` (pure TS, zero uc-core involvement) misuses this as an
  exact-existence check → false "already exists" → real upload skipped → local record wrongly
  marked Synced → data loss on device 2+ of a back-to-back capture sequence.
- This is NOT a random bug — the hash-drift allowance was intentional (PR #678, hardened by
  #1159's contentId dedup on the NATIVE `/SyncClipboard.json` channel). The History-channel
  (compat shim for third-party SyncClipboard clients) was explicitly left hash-only/out of
  scope for contentId (`.planning/2026-06-23-contentid-mobile-dedup-design.md:38,54-57`).
- Full root-cause chain verified against actual server code (not just RN team's report) —
  see findings.md for the verification trail.

## Current Phase
Done (all 5 phases complete)

## Phases

### Phase 1: uc-content-hash crate (new leaf crate)
- [x] Scaffold new workspace member `crates/uc-content-hash`
- [x] Implement `content_hash`, `content_hash_reader`, `file_content_wrapper`,
      `snapshot_hash`, `format_blake3v1`, `snapshot_hash_single_payload`
- [x] Unit tests (9 tests: formula parity via manual re-derivation, order-independence,
      streaming-matches-in-memory, format contract, differentness) — all green,
      `cargo clippy -p uc-content-hash --all-targets` clean
- **Status:** complete

### Phase 2: uc-core refactor to use uc-content-hash (pure extraction, no behavior change)
- [x] `crates/uc-core/src/clipboard/system.rs` `content_hash`/`stream_blake3` → renamed
      `stream_content_hash`, delegates to `uc_content_hash::content_hash`/`content_hash_reader`
- [x] `crates/uc-core/src/clipboard/system.rs` `snapshot_hash` → delegates to
      `uc_content_hash::file_content_wrapper`/`snapshot_hash` (internal sort logic removed,
      now lives in the shared crate)
- [x] `crates/uc-core/src/clipboard/hash.rs:17-23` (`ContentHash` Display) → uses `format_blake3v1`
- [x] `crates/uc-core/Cargo.toml`: removed direct `blake3` dep (no longer used directly anywhere
      in uc-core, confirmed via grep), added `uc-content-hash` path dependency; kept `hex` (still
      used by `hash.rs`'s `From<String>` parser and an unrelated `system.rs:95` site)
- [x] `cargo test -p uc-core`: 152 lib tests + 18 doctests, all pass unchanged — including the
      pre-existing `snapshot_hash_tests::parse_round_trips_display_form` golden-format test
- [x] `cargo clippy -p uc-core --all-targets`: only pre-existing baseline warnings remain
      (hash.rs:34/64, system.rs:118/153/192 — all outside the edited ranges, confirmed by line #)
- **Status:** complete

### Phase 3: Server-side mobile-only content-availability endpoint
- [x] `MobileSyncFacadeDeps` (`crates/uc-application/src/facade/mobile_sync/facade.rs`):
      added `find_entry_by_snapshot_hash: Arc<dyn FindEntryIdBySnapshotHashPort>` +
      `check_entry_availability: Arc<dyn CheckEntryAvailabilityPort>` fields; same 2 fields added
      to the `MobileSyncFacade` struct itself (held directly, like `device_find_by_id`)
- [x] Facade method `check_content_available(&self, snapshot_hash: &str) -> Result<bool, CheckContentAvailableError>`:
      `find_entry_id_by_snapshot_hash` → None ⇒ `Ok(false)`; Some(id) ⇒ `is_entry_available(id)`.
      New `CheckContentAvailableError` enum (1 variant, `Repository`) mirrors the existing
      `IsDeviceCredentialCurrentError` pattern for thin facade-local methods with no backing use case
- [x] Wiring: `crates/uc-bootstrap/src/entrypoint/non_gui.rs` `build_mobile_sync_facade` — added
      `deps.clipboard.entry_ports.find_by_snapshot_hash.clone()` + `.availability.clone()` (both
      already built in `wire.rs:544/546`, just threaded through — no new port/adapter needed)
- [x] New route `GET /api/mobile-sync/content-availability?snapshotHash=blake3v1:...` in new file
      `crates/uc-webserver/src/mobile_lan/routes/content_availability.rs`, registered in
      `routes.rs`'s `build_router` + module-doc table updated. Returns `{"available": bool}`.
      Deliberately separate from `HistoryRecordDoc`/SyncClipboard compat wire shape (own route,
      own DTO, own module) — explicitly documented as NOT part of the SyncClipboard protocol surface
- [x] Re-exports threaded through: `CheckContentAvailableError` added to both
      `facade/mobile_sync/mod.rs` and `facade/mod.rs`'s `pub use mobile_sync::{...}` lists
      (uc-application/AGENTS.md §11.4: facade/ is the only external-facing surface)
- [x] Test infra: `uc-application/facade/mobile_sync/facade.rs` test module — added
      `CheckEntryAvailabilityPort` impl for `UnusedEntryRepo`; refactored `build_facade()` into
      `build_facade_deps()` + thin `build_facade()` wrapper so new tests can override just 2 fields
      via struct-update syntax; fixed 4 other pre-existing inline `MobileSyncFacadeDeps` construction
      sites in the same file that needed the 2 new fields too (missed on first pass, caught by
      compiler E0063, fixed via targeted edits/replace_all)
- [x] `uc-webserver/mobile_lan/test_support.rs`: added `CheckEntryAvailabilityPort` impl for
      `NoopEntryRepo` (returns `Ok(false)`); refactored `build_facade_with_seeded_device` into
      `build_facade_deps_with_seeded_device` + wrapper, added new
      `build_facade_with_seeded_device_and_content_index(...)` for tests needing controllable
      hash→entry + availability fakes
- [x] Unit/integration tests (5 new, in `routes/tests.rs` per this file's established convention —
      NOT inline in the handler file, since the handler file has no pure-function logic worth
      unit-testing on its own): 401 unauthenticated, 400 missing query param, false when hash
      unknown, true when hash known+available, false when hash known but entry unavailable
      (the exact case the availability port's doc-comment calls out)
- [x] `cargo test -p uc-application --lib`: 729/729 pass (incl. 3 new `check_content_available_*`)
- [x] `cargo test -p uc-webserver --lib`: 130/130 pass (incl. 5 new `content_availability_*`)
- [x] `cargo check --workspace`: clean (confirms uc-mobile/uc-cli/uc-daemon/src-tauri/uc-tauri all
      still compile against the changed facade signature)
- [x] `cargo clippy -p uc-webserver --all-targets`: no new warnings in touched files
- **Status:** complete

### Phase 4: uc-mobile FFI surface (breaking change — binding regen needed downstream)
- [x] Added `uc-content-hash` path dependency to `crates/uc-mobile/Cargo.toml`; confirmed
      `cargo tree -p uc-mobile -i aws-lc-rs` still empty (the mobile build script's hard
      invariant — blake3/hex introduce no native crypto lib)
- [x] New free function `compute_snapshot_hash(bytes: Vec<u8>) -> String` (uniffi-exported,
      alongside `uc_mobile_init`) — thin wrapper over `uc_content_hash::snapshot_hash_single_payload`
- [x] `MobileSyncClient::is_content_available(server, snapshot_hash: String) -> Result<bool, SyncError>`
      wrapping `GET /api/mobile-sync/content-availability`, mirrors `get_history_payload`'s style
- [x] **SCOPE CHANGE — dropped `put_clipboard_deduped` convenience method.** See "Course
      correction" note below; it cannot be implemented safely with the current server wire
      contract. `is_content_available` ships as a standalone primitive only.
- [x] 5 new tests: `compute_snapshot_hash` parity + differentness (pure), `is_content_available`
      false/true/401-mapping (mock-server, matching this crate's existing `spawn_mock` pattern —
      added a `available_snapshot_hashes` knob to `MockConfig` + a new mock route/handler)
- [x] `cargo test -p uc-mobile --lib`: 89/89 pass; `cargo clippy -p uc-mobile --all-targets`: clean
- [x] `cargo check --workspace`: clean (confirms src-tauri/uc-tauri/uc-cli/uc-daemon all still build)
- **Status:** complete

**Course correction — `put_clipboard_deduped` dropped from scope:**
Investigated whether a convenience method could skip the byte upload entirely when
`is_content_available` returns true. Found this is NOT safely implementable with the current
server contract: `find_entry_id_by_snapshot_hash` matches ANY entry with this content hash,
not necessarily the CURRENTLY ACTIVE one. If a client skips the full `put_clipboard` call
because matching (but non-active) content exists, the daemon's active-clipboard register never
advances to reflect this capture, so peer devices would not see it as new activity — this
reintroduces a different variant of the original bug (silent non-propagation instead of silent
data loss). A truly safe "skip bytes, still register as active" flow needs a NEW server
capability (e.g. a metadata-only PUT that references existing content by hash) that does not
exist today — the existing `IncomingMobileBuffer` two-step protocol requires a file to have
actually been staged via a preceding `PUT /file/{name}` before `PUT /SyncClipboard.json` will
pair with it. Building that is out of scope for this round; flagged as a candidate follow-up
if the RN team wants the bandwidth optimization back. `is_content_available` alone is still a
strict improvement: it gives the RN team a *reliable* primitive to replace the broken
`/api/history/{profileId}`-based heuristic, usable for narrower safe cases (e.g. "did the
upload I just completed a moment ago actually land") without reintroducing the false-positive
class of bug.

### Phase 5: Documentation + handoff notes for RN team
- [x] Wrote `.planning/2026-07-05-content-availability-rn-integration-guide.md` (mirrors the
      `2026-06-23-contentid-rn-integration-guide.md` style/structure). Covers: problem recap,
      what's new in Rust core, FFI regen requirement, exact code change (`getRecord` →
      `isContentAvailable`), and — critically — an explicit §5 warning on the scope limitation
      discovered during Phase 4 (this is NOT a blanket "skip the whole upload" switch; explains
      why, with a verification checklist)
- [x] `cargo check --workspace`: clean
- [x] `cargo test --workspace`: 1 failure workspace-wide —
      `uc-platform::clipboard::common::tests::effective_mime::parameterized_text_plain_classifies_as_text_plain`.
      Confirmed via `git status --short` this is in a crate untouched by any change in this plan
      (only uc-content-hash/uc-core/uc-application/uc-webserver/uc-bootstrap/uc-mobile were
      touched). Matches a documented pre-existing baseline debt (this session's own memory:
      "uc-platform effective_mime... 均 HEAD 即坏"). Every touched crate individually green:
      uc-content-hash 9/9, uc-core 152+18doctests, uc-application 729/729, uc-webserver 130/130,
      uc-bootstrap 46/46, uc-mobile 89/89
- **Status:** complete

## Key Questions
1. Crate name: `uc-content-hash` — confirmed, no objection raised.
2. Client surface granularity: ship BOTH `is_content_available` (core primitive) AND
   `put_clipboard_deduped` (convenience) — my recommendation, proceeding since user said
   "直接开始" without further pushback on this specific point. Revisit if RN team prefers
   only the primitive.
3. Where does hash computation live for the mobile probe? Decision: **uc-mobile**, not
   uc-mobile-proto — preserves proto's existing "content_id is opaque, never computed
   client-side" philosophy (see uc-mobile-proto module docs).

## Decisions Made
| Decision | Rationale |
|----------|-----------|
| New crate `uc-content-hash` instead of duplicating formula in uc-mobile-proto + parity tests | Real sharing eliminates drift risk entirely rather than just detecting it; matches existing project pattern of small dependency-free leaf crates (uc-mobile-proto, uc-app-paths) |
| Content-availability check = `find_entry_id_by_snapshot_hash` + `CheckEntryAvailabilityPort::is_entry_available`, NOT the existing outbound serve path (`LatestClipboardSnapshotAdapter`) | The serve path only discovers a missing blob when it tries to actually read it (throws 500, not a clean false); availability port does a real-time DB+FS double-check without side effects |
| New dedicated route `GET /api/mobile-sync/content-availability`, NOT adding `content_id` to `HistoryRecordDoc` | `/api/history/*` `hash` field is the official SyncClipboard protocol's SHA-256 identity, consumed by real third-party clients — must not repurpose its semantics; keep compat surface boundary clean per `history.rs:1-12`'s own stated intent |
| Hash formula for mobile always uses the simple 2-level case (no `file_content_wrapper` layer) | Verified `apply_incoming.rs:762,809,853` and `payload_codec.rs:168,409,455` all set `file_content_digests: Vec::new()` for mobile-sync inbound — the uri-list wrapping branch never triggers for mobile payloads |
| Keep existing `put_clipboard` (client.rs:610) as-is, always-upload | It's already the safe fallback; the new dedup path is additive, never removes the safety net |

## Errors Encountered
| Error | Attempt | Resolution |
|-------|---------|------------|
|       | 1       |            |

## Notes
- This plan follows on from extensive research already done in conversation (see findings.md
  for full verification trail — server-side hash-drift code read directly, git blame/commit
  archaeology on #678/#1159, snapshot_hash algorithm confirmed via direct file reads).
- Architecture rule reminder (project AGENTS.md): don't mix boundary layers in one commit.
  Phase 1/2 (uc-core + new crate) is one architectural slice; Phase 3 (application+infra+webserver)
  is another; Phase 4 (uc-mobile FFI) is a third. Commit them separately (see prior plan's
  "提交拆分" section already agreed with user).
- RN/TS-side integration (Phase 5's actual code change) happens in a SEPARATE repo
  (`uniclipboard-android`), out of scope for this repo's implementation — only the handoff doc
  is in scope here.
- Update phase status as you progress: pending → in_progress → complete
- Re-read this plan before major decisions
- Log ALL errors — they help avoid repetition
