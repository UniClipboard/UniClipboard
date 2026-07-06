# Progress Log

## Session: 2026-07-05

### Phase 0: Investigation (prior to planning-with-files setup)
- **Status:** complete
- Actions taken:
  - Explored current mobile-sync history query architecture (Explore agent)
  - Verified mobile team's bug report against actual server code (read history.rs in full)
  - Traced git history of the hash-drift-allowance logic (#678, #1159) to confirm it's
    intentional, not a random bug
  - Investigated whether "add content_id to History wire" would work — found it would NOT
    (content_id reflects only current active slot, not an independent index) — course-corrected
    before proposing an incomplete fix
  - Verified `find_entry_id_by_snapshot_hash` exists as a real, already-wired hash index (Explore agent)
  - Verified the exact snapshot_hash algorithm via direct file reads (system.rs) — confirmed pure
    function, no server secrets, mobile payloads always hit the simple 2-level formula
  - Verified data-availability semantics (CheckEntryAvailabilityPort vs. serve-path-implicit-fail)
    and MobileSyncFacade's current dependency gaps (Explore agent)
  - User confirmed: RN's putContent/getRecord is pure TS, zero uc-core involvement; uc-core only
    exports getLatest/putClipboard/queryHistory/getFile/putFile
  - User decided: pursue the "complete fix" (real snapshot-hash-based dedup), and requested all
    ends share one hash-computation crate
- Files created/modified:
  - task_plan.md (created)
  - findings.md (created)
  - progress.md (this file, created)

### Phase 1: uc-content-hash crate
- **Status:** complete
- **Started:** 2026-07-05
- Actions taken:
  - Checked Cargo.toml conventions against `uc-app-paths` (small leaf crate style) and
    confirmed `blake3 = "1.8.2"` / `hex = "0.4.3"` versions already pinned elsewhere in
    the workspace (uc-core), matched them here for resolver consistency
  - Wrote `crates/uc-content-hash/src/lib.rs`: `content_hash`, `content_hash_reader`,
    `file_content_wrapper`, `snapshot_hash`, `format_blake3v1`, `snapshot_hash_single_payload`
    — each with a doc comment, plus 9 unit tests (manual-formula parity, order-independence,
    streaming-vs-in-memory equivalence, format contract)
  - Registered as workspace member in root `Cargo.toml`
  - `cargo test -p uc-content-hash`: 9/9 pass
  - `cargo clippy -p uc-content-hash --all-targets`: clean, no warnings
- Files created/modified:
  - crates/uc-content-hash/Cargo.toml (created)
  - crates/uc-content-hash/src/lib.rs (created)
  - Cargo.toml (added workspace member)

### Phase 2: uc-core refactor to use uc-content-hash
- **Status:** complete
- **Started:** 2026-07-05
- Actions taken:
  - Read system.rs imports + confirmed `std::io::Read` becomes unused after refactor (was only
    used by the old `stream_blake3`'s `.read()` call) — removed the import
  - Confirmed `RepresentationHash`/`SnapshotHash` both `impl Deref<Target = ContentHash>`
    (system.rs:66,74) — the `.bytes` field access on `r.content_hash().bytes` auto-derefs, so
    that call site needed zero changes
  - Replaced `content_hash`/`stream_blake3` to delegate to `uc_content_hash::content_hash` /
    `content_hash_reader`; replaced `snapshot_hash`'s manual blake3 aggregation (file-content
    wrapper + sort + snapshot-hash-v1 prefix) with `uc_content_hash::file_content_wrapper` /
    `snapshot_hash` calls (sorting now lives inside the shared crate, so the old explicit
    `rep_hashes.sort_unstable()` line was removed as redundant)
  - Updated `hash.rs`'s `ContentHash::fmt` to call `uc_content_hash::format_blake3v1`
  - Confirmed via grep: `blake3::` no longer appears anywhere in uc-core/src after the edit;
    `hex::` still used (hash.rs's `From<String>` parser + one unrelated site at system.rs:95),
    so `hex` dependency was kept
  - Updated `crates/uc-core/Cargo.toml`: removed `blake3`, added `uc-content-hash` path dep
  - `cargo test -p uc-core`: 152 + 18 doctests, all green, including the pre-existing
    `snapshot_hash_tests::parse_round_trips_display_form` (proves `blake3v1:<hex>` format
    unchanged)
  - `cargo clippy -p uc-core --all-targets`: cross-checked every warning's line number against
    the edited ranges — all warnings are pre-existing baseline debt in untouched code
    (hash.rs:34/64 `expect_used` in the `From` impls I didn't touch; system.rs:118/153/192
    `map_or`/`expect_used` outside the content_hash/snapshot_hash functions)
- Files created/modified:
  - crates/uc-core/src/clipboard/system.rs (modified)
  - crates/uc-core/src/clipboard/hash.rs (modified)
  - crates/uc-core/Cargo.toml (modified)

### Phase 3: Server-side mobile-only content-availability endpoint
- **Status:** complete
- **Started:** 2026-07-05
- Actions taken:
  - Read `MobileSyncFacadeDeps`/`MobileSyncFacade` struct + `entry_intents.rs` port defs +
    `clipboard_entry_repository.rs` to confirm exact trait names/paths before editing
  - Added `find_entry_by_snapshot_hash`/`check_entry_availability` fields to Deps + facade struct;
    wired through `new()`; added `CheckContentAvailableError` + `check_content_available` method
    (pattern-matched against the existing `IsDeviceCredentialCurrentError`/`is_device_credential_current`
    thin-facade-method precedent already in the same file)
  - Re-exported `CheckContentAvailableError` through `facade/mobile_sync/mod.rs` and `facade/mod.rs`
    (uc-application's single external-facing surface per AGENTS.md §11.4)
  - Fixed 4 pre-existing test call sites in facade.rs that construct `MobileSyncFacadeDeps` inline
    (missed on the first edit pass — caught immediately by `cargo test`'s E0063 compiler error,
    not a runtime surprise). Refactored the main `build_facade()` into a `build_facade_deps()` +
    thin wrapper so the new dedicated tests could override just 2 fields via struct-update syntax
    instead of duplicating the ~30-line fixture
  - Added 3 facade-level tests: unknown hash → false, known+available → true,
    known-but-unavailable → false (the exact scenario the port's own doc-comment warns about)
  - Wired `crates/uc-bootstrap/src/entrypoint/non_gui.rs`'s `build_mobile_sync_facade`: both new
    ports were already available at `deps.clipboard.entry_ports.{find_by_snapshot_hash,availability}`
    (built in `wire.rs:544/546` for other consumers already) — pure threading, no new adapter
  - Wrote new route module `content_availability.rs`: `GET ?snapshotHash=` → `Query` extractor →
    facade call → `{"available": bool}` JSON, 400 on empty/missing hash, 500 on repository error.
    Module doc explicitly states this is NOT part of the SyncClipboard-compat protocol surface
  - Registered the route + module in `routes.rs`, updated its module-level doc table (existing
    convention in this file — a doc table enumerating every route's compat-boundary status)
  - Extended `uc-webserver/mobile_lan/test_support.rs`: added `CheckEntryAvailabilityPort` for
    `NoopEntryRepo`; refactored `build_facade_with_seeded_device` the same way as facade.rs
    (deps-builder + thin wrapper) and added `build_facade_with_seeded_device_and_content_index`
    for tests needing a controllable hash→entry/availability fake pair
  - Added 5 axum-integration tests to `routes/tests.rs` (this file's established home for
    router-level tests, as opposed to inline `#[cfg(test)]` blocks for pure-function unit tests
    like `history.rs` has) — checked this convention by reading `history.rs`'s existing inline
    tests (pure helpers only) vs. `routes/tests.rs` (axum `oneshot` integration tests) before
    deciding where to put mine
  - `cargo test -p uc-application --lib`: 729/729 green; `cargo test -p uc-webserver --lib`:
    130/130 green; `cargo check --workspace`: clean; `cargo clippy -p uc-webserver --all-targets`:
    no new warnings
- Files created/modified:
  - crates/uc-application/src/facade/mobile_sync/facade.rs (modified — deps/struct/method/tests)
  - crates/uc-application/src/facade/mobile_sync/mod.rs (modified — re-export)
  - crates/uc-application/src/facade/mod.rs (modified — re-export)
  - crates/uc-bootstrap/src/entrypoint/non_gui.rs (modified — wiring)
  - crates/uc-webserver/src/mobile_lan/routes/content_availability.rs (created)
  - crates/uc-webserver/src/mobile_lan/routes.rs (modified — module registration + route + doc table)
  - crates/uc-webserver/src/mobile_lan/routes/tests.rs (modified — 5 new tests)
  - crates/uc-webserver/src/mobile_lan/test_support.rs (modified — new fixture builder)

### Phase 4: uc-mobile FFI surface
- **Status:** complete
- **Started:** 2026-07-05
- Actions taken:
  - Added `uc-content-hash` dep to Cargo.toml; verified `cargo tree -i aws-lc-rs` stays empty
    (the mobile build script's hard invariant, per Cargo.toml's own comment)
  - Read `uc_mobile_init`'s section to add `compute_snapshot_hash` free fn there (same
    "process-wide utility, no client instance" category)
  - Read `get_history_payload` (closest existing analog: simple GET, profile_id → bytes) to
    model `is_content_available`'s style; added `ContentAvailabilityDoc` decode-only struct
    near `HistoryRecord`'s `From` impl
  - **Mid-implementation course correction**: while designing the originally-planned
    `put_clipboard_deduped` convenience wrapper, realized it cannot be safely implemented —
    `find_entry_id_by_snapshot_hash` matches ANY entry with the hash, not necessarily the
    ACTIVE one, so skipping the full upload would leave the active-clipboard register stale
    for peer devices. This requires a server-side "register by reference" capability that
    doesn't exist (`IncomingMobileBuffer`'s pairing protocol requires an actual staged file).
    Dropped this method from scope entirely rather than ship something that looks like a fix
    but reintroduces a different correctness gap. Documented the reasoning in task_plan.md's
    "Decisions Made" section for future reference (e.g. if RN team asks for the bandwidth
    optimization later, this is the prerequisite work)
  - Added mock server support: `MockConfig.available_snapshot_hashes: HashSet<String>` +
    `mock_content_availability` handler + route registration in `spawn_mock`
  - Added 5 tests in the existing `mod tests` (not a separate module — this crate keeps tests
    inline in client.rs): 2 pure (`compute_snapshot_hash` parity vs. calling `uc_content_hash`
    directly, and differentness), 3 mock-server (unknown hash → false, known hash → true,
    wrong password → 401 mapped correctly)
  - `cargo test -p uc-mobile --lib`: 89/89 green; `cargo clippy -p uc-mobile --all-targets`: clean
  - `cargo check --workspace`: clean; `cargo tree -p uc-mobile -i aws-lc-rs`: still empty (confirmed
    invariant holds — command "errors" with "no match", which IS the pass condition)
- Files created/modified:
  - crates/uc-mobile/Cargo.toml (modified — new dependency)
  - crates/uc-mobile/src/client.rs (modified — free fn, method, decode struct, mock server, tests)

### Phase 5: RN handoff doc + final checks
- **Status:** complete
- **Started:** 2026-07-05
- Actions taken:
  - Wrote the RN integration guide, explicitly documenting the §5 scope-limitation warning
    (do not use `isContentAvailable` as a blanket skip-the-whole-upload switch) so this doesn't
    get lost/reintroduced by whoever implements the RN side without this repo's context
  - `cargo check --workspace`: clean
  - `cargo test --workspace` finished: 1 failure (`uc-platform::...::effective_mime::...`),
    confirmed via `git status --short` to be in a crate this plan never touched, and matching a
    documented pre-existing baseline-debt memory ("uc-platform effective_mime... 均 HEAD 即坏") —
    not a regression from this work
  - Ran `cargo test -p uc-bootstrap --lib` explicitly (the one touched crate without its own
    dedicated test run earlier in this session, since its change was a 2-line wiring addition):
    46/46 pass
- Files created/modified:
  - .planning/2026-07-05-content-availability-rn-integration-guide.md (created)

## Final Summary
All 5 phases complete. Every crate touched by this work is fully green:
uc-content-hash 9/9, uc-core 152+18doctests, uc-application 729/729, uc-webserver 130/130,
uc-bootstrap 46/46, uc-mobile 89/89. `cargo check --workspace` clean. The one workspace-wide
test failure (`uc-platform::effective_mime`) is pre-existing baseline debt unrelated to any file
this plan touched.

Files changed (13 modified + 3 created, all uncommitted — user has not asked to commit):
- crates/uc-content-hash/ (new crate: Cargo.toml, src/lib.rs)
- crates/uc-core/{Cargo.toml, src/clipboard/system.rs, src/clipboard/hash.rs}
- crates/uc-application/src/facade/mobile_sync/{facade.rs, mod.rs}, src/facade/mod.rs
- crates/uc-bootstrap/src/entrypoint/non_gui.rs
- crates/uc-webserver/src/mobile_lan/{routes.rs, routes/tests.rs, test_support.rs,
  routes/content_availability.rs (new)}
- crates/uc-mobile/{Cargo.toml, src/client.rs}
- Cargo.toml (workspace member registration), Cargo.lock
- .planning/2026-07-05-content-availability-rn-integration-guide.md (new)

Scope change from the original plan: dropped `put_clipboard_deduped` convenience method
(see task_plan.md's "Course correction" note) — cannot be safely implemented without a new
server "register by reference" capability that doesn't exist. `is_content_available` ships
alone, with the RN handoff doc explicitly warning against using it as a blanket upload-skip.

Not done (explicitly out of scope, confirmed with user earlier in conversation): the actual
RN/TS-side code change in the separate `uniclipboard-android` repo. Only the handoff doc is
in scope here.

## Test Results
| Test | Input | Expected | Actual | Status |
|------|-------|----------|--------|--------|
| `cargo test -p uc-content-hash` | 9 unit tests | all pass | all pass | ✓ |
| `cargo test -p uc-core` | 152 lib + 18 doctests | all pass, format unchanged | all pass | ✓ |
| `cargo test -p uc-application --lib` | 729 tests | all pass | all pass | ✓ |
| `cargo test -p uc-webserver --lib` | 130 tests | all pass | all pass | ✓ |
| `cargo check --workspace` | full workspace | clean | clean | ✓ |

## Error Log
| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
|           |       | 1       |            |

## 5-Question Reboot Check
| Question | Answer |
|----------|--------|
| Where am I? | Phase 1 — scaffolding `uc-content-hash` crate |
| Where am I going? | Phase 2 (uc-core refactor) → Phase 3 (server endpoint) → Phase 4 (uc-mobile FFI) → Phase 5 (RN handoff doc) |
| What's the goal? | Fix mobile-sync silent-data-loss bug (2nd photo never uploads) via a shared, reproducible content-hash crate + real availability-backed existence check, replacing the RN client's unreliable getRecord/putContent heuristic |
| What have I learned? | See findings.md — full root-cause chain, snapshot_hash algorithm, availability semantics, facade wiring gaps |
| What have I done? | See Phase 0/1 log above |

---
*Update after completing each phase or encountering errors*
