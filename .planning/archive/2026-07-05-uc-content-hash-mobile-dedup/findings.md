# Findings & Decisions

## Requirements
- Mobile team reported: 2nd photo in a back-to-back capture sequence never uploads its bytes;
  local record is wrongly marked `Synced`; an unrelated image sometimes "reappears" (separate,
  lower-priority root cause — LWW active-clipboard register behaving as designed).
- User wants the "complete fix" (not the minimal always-upload fallback), with all ends
  (desktop daemon + uc-mobile FFI + eventually RN) sharing ONE hash-computation rule set,
  extracted into an independent crate.
- Scope for THIS repo (uniclipboard desktop/daemon): crate + uc-core refactor + server endpoint
  + uc-mobile FFI methods. RN-side (`uniclipboard-android`) integration is a separate repo/PR,
  out of scope here beyond a handoff doc.

## Research Findings

### Root cause chain (verified against actual code, not just the RN team's report)
- `crates/uc-webserver/src/mobile_lan/routes/history.rs:184-232`: `current_profile_type_allows_hash_drift`
  returns true for `Image | File` only. `current_profile_hash_is_compatible` treats ANY hash as
  compatible for those types once `item_type` matches the currently-active clipboard slot.
  `current_profile_record_for_request` even overwrites the response's `hash` field to echo back
  whatever the client asked for (history.rs:229, with a debug log admitting it).
- This means `GET /api/history/{Type}-{anyhash}` returns 200 for Image/File as long as the
  CURRENT active-clipboard slot's type matches — completely independent of whether that exact
  hash was ever uploaded before.
- Confirmed via curl reproduction in the mobile team's report: `Image-DEADBEEF` (made-up hash) →
  200 with current clipboard content; `Text-DEADBEEF` / `File-DEADBEEF` → 404 (type mismatch at
  that moment, current active was Image).
- History channel is NOT a real paginated history store — `query_history_records` (history.rs:337)
  and `get_history_record` (history.rs:355) both bottom out in `latest_history_record` (history.rs:324)
  which wraps `facade.get_latest_sync_doc()` — i.e. "the single current active-clipboard slot",
  never more than 0-1 records. Confirmed by the module's own doc comment (history.rs:1-12): this is
  an explicitly-scoped SyncClipboard v3 compat shim, not a real history DB.

### Why the hash-drift allowance exists (it's intentional, not a random bug)
- Introduced in PR #678 (2026-05-13, `526aa4595`, original SyncClipboard mobile compat layer),
  with dedicated unit tests (history.rs:275 `current_profile_record_accepts_mobile_upload_hash_drift`).
- Rationale confirmed via commit `6749653e5` (#1159, "dedup re-encoded content by stable
  contentId", 2026-06-30): the daemon may re-encode images server-side (e.g. HEIC→JPEG/PNG)
  after ingest, which changes the SHA-256 of stored bytes vs. what the client originally
  uploaded — the drift-allowance exists to not 404 a client re-confirming its own just-uploaded
  content post-re-encode.
- #1159 fixed this properly for the NATIVE `/SyncClipboard.json` channel by adding a stable
  `content_id` (`blake3v1:<hex>`, frozen at ingest) alongside `hash` (still SHA-256, recomputed
  fresh each read). The sync reducer (`uc-mobile-proto/src/sync_engine.rs`) now dedups on
  `content_id` when both sides have it, falling back to `hash` otherwise.
- `.planning/2026-06-23-contentid-mobile-dedup-design.md:38-57`: explicitly states "History 通道
  保持 hash-only" — the compat History channel's wire shape (`HistoryRecordDoc`) was deliberately
  NOT given a `content_id` field, and this is registered as a KNOWN, ACCEPTED tech-debt item
  ("重编码图片经 history 浮现仍可能重复" — but that's about a COSMETIC duplicate-card-in-list
  symptom, not the silent-data-loss variant we're fixing — the two are related but distinct).

### Why "just add content_id to the History wire" does NOT work (my initial mistake, caught before implementing)
- `content_id`/`snapshot_hash` is a property of the ACTIVE-CLIPBOARD REGISTER's current slot
  (`crates/uc-core/src/clipboard/active_state.rs`, `latest_snapshot_adapter.rs:26,147` — "read
  from the active register's current value"), NOT an independent hash→identity index.
- Echoing `content_id` on a GET response for an arbitrary requested hash would just echo "whatever
  the current slot's content_id happens to be" — same fundamental defect as the current `hash`
  drift-allowance, just renamed. Querying a brand-new hash (e.g. a just-taken 2nd photo) would
  still get a false-positive match against the current slot.
- SHA-256 (the wire `hash` field) is NEVER persisted/indexed anywhere — confirmed via
  `crates/uc-application/src/usecases/mobile_sync/sync_clipboard_mapping.rs:82-101`
  (`sha256_hex_upper`): computed fresh from CURRENT stored bytes on every read ("daemon 在 PUT
  后回填"). There is no "original upload hash → entry" table at all.

### The actual working solution: `find_entry_id_by_snapshot_hash` already exists as a real index
- `crates/uc-core/src/ports/clipboard/clipboard_entry_repository.rs:60`:
  `ClipboardEntryStore::find_entry_id_by_snapshot_hash(&self, snapshot_hash: &str) -> Result<Option<EntryId>>`
  — a REAL, already-wired, persisted hash→entry lookup (not tied to "current active slot").
- Already actively used (not dead code) at: `clipboard_capture/usecase.rs:671` (local capture
  resurface/dedup), `apply_inbound/usecase.rs:321` (inbound download-before-write dedup, shared
  by P2P + mobile), `active_state/apply_inbound.rs:347` (active-register convergence),
  `active_state/serve_pull.rs:101` (serving a peer's pull-by-hash request).
- Persisted impl: `crates/uc-infra/src/db/repositories/clipboard_entry_repo.rs:257`.
- KEY BLOCKER RESOLVED: this is keyed by `snapshot_hash` (blake3), NOT the client's SHA-256 —
  different hash spaces. Solved by making the CLIENT compute the same blake3 formula (see below)
  instead of trying to bridge SHA-256 → blake3 server-side.

### snapshot_hash algorithm (exact, verified by direct file read — this is the crux of the whole plan)
`crates/uc-core/src/clipboard/system.rs`:
- Inner (per-representation) hash — `ObservedClipboardRepresentation::content_hash()` (:239-255):
  `Inline(bytes)` → `blake3::hash(bytes)`; `LocalFile{path}` → streamed blake3 via `stream_blake3`
  (:259-271, 64KiB chunks). Same algorithm either way, just streaming vs in-memory.
- Outer (snapshot) hash — `SystemClipboardSnapshot::snapshot_hash()` (:541-582):
  1. Collect each representation's inner hash bytes (`[u8;32]`), EXCEPT file-list reps when
     `file_content_digests` is non-empty (those get replaced — see next point).
  2. If `file_content_digests` non-empty: `blake3::Hasher` seeded with prefix `b"file-content|"`,
     fed the SORTED digests, finalized → this replaces the excluded file-list rep's hash in the
     collection.
  3. Sort ALL collected rep hashes (`Vec<[u8;32]>::sort_unstable()`).
  4. Final: `blake3::Hasher` seeded with prefix `b"snapshot-hash-v1|"`, fed the sorted hashes in
     order, finalized.
  5. Formatted as `blake3v1:<hex>` via `ContentHash` Display (`crates/uc-core/src/clipboard/hash.rs:17-25`).
- **No server secrets/salts/device-id/mime/filename anywhere in this formula** — pure function of
  content bytes (+ the domain-separation string literals, which are public/fixed).
- **Mobile-sync payloads never trigger the `file_content_digests` branch**: confirmed via grep —
  `crates/uc-application/src/usecases/mobile_sync/apply_incoming.rs:762,809,853` and
  `crates/uc-application/src/usecases/clipboard_sync/payload_codec.rs:168,409,455` ALL construct
  snapshots with `file_content_digests: Vec::new()`. So for Image/File mobile uploads (always a
  single representation), the formula collapses to:
  ```
  snapshot_hash = "blake3v1:" + hex(blake3(b"snapshot-hash-v1|" ++ blake3(payload_bytes)))
  ```
  Two blake3 calls, fully reproducible by any independent client implementation.
- Why this correctly survives server-side re-encoding: the identity is computed ONCE at ingest
  time from the ORIGINAL bytes the client uploaded (frozen, per #1159's design intent), and never
  recomputed after a later re-encode. The client, holding the same original bytes, always
  reproduces the SAME snapshot_hash regardless of what the server later does to its stored copy.

### Data availability — why "entry exists" isn't enough, and how to check the right thing
- `CheckEntryAvailabilityPort::is_entry_available(&self, entry_id: &EntryId) -> Result<bool, ClipboardRepositoryError>`
  — `crates/uc-core/src/ports/clipboard/entry_intents.rs:97` (method at :114).
- Impl `DieselEntryAvailabilityRepository` (`crates/uc-infra/src/db/repositories/entry_availability_repo.rs:42`):
  computed LIVE every call (no cached column) — walks
  `clipboard_entry → event_id → clipboard_snapshot_representation`, requires ≥1 representation
  with no `Failed`/`Lost` payload_state (`reps_indicate_available`, :86); for file-list (uri-list)
  reps ALSO requires the referenced `file://` path to actually exist/be readable/be a regular file
  on disk (:114). This is a real-time DB-state + filesystem double-check, NOT a query against
  blob_store existence directly.
- The EXISTING outbound serve path (`LatestClipboardSnapshotAdapter`,
  `usecases/mobile_sync/latest_snapshot_adapter.rs`) does NOT pre-check availability — it only
  discovers a missing blob when `BlobReaderPort::get()` actually fails mid-read (surfaces as a
  500 `LatestClipboardSnapshotError::Resolution`, not a clean bool). This is why the new
  availability check must go through `CheckEntryAvailabilityPort` directly, not reuse the serve path.

### MobileSyncFacade wiring (what needs to change to expose the above)
- Facade struct: `crates/uc-application/src/facade/mobile_sync/facade.rs:255-286` (11 use cases +
  a few extra fields). Deps struct `MobileSyncFacadeDeps` at :175, constructed via `new` at :291.
- CONFIRMED GAP: facade currently holds neither `FindEntryIdBySnapshotHashPort` nor
  `CheckEntryAvailabilityPort` directly — `find_by_snapshot_hash` today is only reachable inside
  the `apply_inbound` use case's internals; `snapshot_ports.entry_repo` is `GetClipboardEntryPort`
  (plain get_entry), not the hash-lookup port.
  it. Bootstrap wiring point: `crates/uc-bootstrap/src/entrypoint/non_gui.rs:246` (where
  `MobileSyncFacade` gets constructed) — needs two more lines threading these through.

### webserver route registration pattern
- `crates/uc-webserver/src/mobile_lan/routes.rs:105` `build_router`. State type `MobileLanState`
  (:53) exposes `Arc<MobileSyncFacade>` via `FromRef` (:70).
- Minimal handler example: `get_sync_clipboard_json` (`routes/sync_doc.rs:129`),
  `State(facade): State<Arc<MobileSyncFacade>>`. Registered at routes.rs:131, whole group under
  `basic_auth` middleware (:140). New route follows the identical pattern + an
  `axum::extract::Query<T>` extractor for `?snapshotHash=`.

### uc-mobile client architecture (existing pattern the new methods must follow)
- `crates/uc-mobile/src/client.rs` — `MobileSyncClient`, explicitly documented as a byte-for-byte
  Rust port of `uc-ios Shared/Network/SyncClipboardClient.swift` (module doc :1-8), which is
  itself the NORMATIVE reference. Long-lived, `current_thread` tokio runtime on one dedicated
  thread (iOS extension jetsam constraints), reqwest-based, uniffi-exported.
- Existing methods already ported: `get_latest` (GET /SyncClipboard.json, native channel, carries
  content_id), `put_clipboard` (PUT /SyncClipboard.json + optional file PUT — ALWAYS uploads,
  no pre-check, this is the existing safe fallback pattern to preserve), `query_history` (POST
  /api/history/query — paginated-looking but really 0/1 records per the root-cause finding above),
  `get_history_payload` (GET /api/history/{profileId}/data).
- NOT yet ported: `getRecord` (single-profile GET, the buggy existence check) / `putContent`'s
  upload leg. Confirmed by user directly: RN's `SyncClipboardClient.putContent`/`getRecord` are a
  PURE TS invention with ZERO uc-core involvement — uc-core only ever exported
  getLatest/putClipboard/queryHistory/getFile/putFile. This means we're not "fixing a broken
  port", we're adding NEW capability that never existed in Rust.
- `uc-mobile-proto`'s `sync_engine.rs` module docs explicitly treat `content_id` as OPAQUE —
  "never computed client-side, only learned from / echoed back from the server, server is sole
  authority". Decision: keep this invariant intact in uc-mobile-proto; put the new
  snapshot-hash-computation call in `uc-mobile`'s client.rs instead (which already does its own
  hashing-adjacent logic like multipart boundaries) so proto's philosophy isn't violated. The
  client only ever uses its self-computed hash as a QUERY KEY for an existence probe — it never
  asserts this value AS an authoritative identity or tries to get the server to accept a
  client-supplied content_id, so "server is sole authority for actual assignment" still holds;
  worst-case failure mode if the client's formula is ever subtly wrong is a false negative
  (unnecessary re-upload), never a false positive (never reintroduces the data-loss bug).

## Technical Decisions
| Decision | Rationale |
|----------|-----------|
| New crate `uc-content-hash`, not duplicate-with-parity-tests | Eliminates drift risk structurally instead of just detecting it; matches existing small-leaf-crate pattern (uc-mobile-proto, uc-app-paths) |
| Crate has zero deps beyond `blake3`(+`hex`) | Must cross-compile cleanly to iOS/Android via uc-mobile without pulling in uc-core's full domain model graph |
| `uc-core`'s `SnapshotHash`/`RepresentationHash`/`ContentHash` newtypes stay in uc-core | They're legitimate domain concepts per uc-core's own modeling rules; only the raw computation delegates out |
| New dedicated mobile-only route, not a `HistoryRecordDoc.content_id` field | `/api/history/*`'s `hash` is the official SyncClipboard protocol's SHA-256 identity consumed by real third-party clients — must not repurpose; keep the compat shim's documented boundary intact (history.rs:1-12) |
| Availability check = `find_entry_id_by_snapshot_hash` + `is_entry_available`, not the outbound serve path | Serve path only fails on actual read (500, not a clean signal); the availability port is a real side-effect-free double check |
| Hash computed client-side in `uc-mobile`, not `uc-mobile-proto` | Preserves proto's "content_id opaque, server-authoritative" documented philosophy; failure mode of a wrong client computation is safe (false negative → re-upload, never false positive → data loss) |
| Keep `put_clipboard` (client.rs:610) unchanged as the always-safe fallback | New dedup path is purely additive; removing the existing always-upload behavior would be a regression risk for no benefit |

## Issues Encountered
| Issue | Resolution |
|-------|------------|
| Initially proposed "just add content_id to History wire + compare" as the "complete fix" | Caught before implementing: content_id reflects only the CURRENT active slot, not an independent hash index — querying a brand-new hash would still false-positive-match. Pivoted to reusing the already-existing, already-correct `find_entry_id_by_snapshot_hash` index instead, keyed by a client-reproducible blake3 formula. |
| Needed to confirm client can reproduce blake3 formula exactly (hash-space bridge SHA-256 vs blake3) | Verified formula has no server secrets and mobile payloads never hit the `file_content_digests` branch — confirmed by grep across apply_incoming.rs/payload_codec.rs, all three call sites set `Vec::new()` |

## Resources
- `crates/uc-webserver/src/mobile_lan/routes/history.rs` — compat History channel, root cause location
- `crates/uc-core/src/clipboard/system.rs` — snapshot_hash algorithm (:239-271 inner, :541-582 outer)
- `crates/uc-core/src/clipboard/hash.rs:17-25` — `ContentHash` Display (`blake3v1:` formatting)
- `crates/uc-core/src/ports/clipboard/clipboard_entry_repository.rs:60` — `find_entry_id_by_snapshot_hash`
- `crates/uc-core/src/ports/clipboard/entry_intents.rs:97,114` — `CheckEntryAvailabilityPort`
- `crates/uc-infra/src/db/repositories/entry_availability_repo.rs:42,86,114` — availability impl
- `crates/uc-infra/src/db/repositories/clipboard_entry_repo.rs:257` — snapshot_hash lookup impl
- `crates/uc-application/src/facade/mobile_sync/facade.rs:175,255-286,291` — facade struct/deps/ctor
- `crates/uc-bootstrap/src/entrypoint/non_gui.rs:246` — MobileSyncFacade construction/wiring site
- `crates/uc-bootstrap/src/wiring/wire.rs:537-548` — `ClipboardEntryPorts` (find_by_snapshot_hash :544, availability :546 already built here)
- `crates/uc-webserver/src/mobile_lan/routes.rs:105,131,140` — router registration + basic_auth middleware
- `crates/uc-webserver/src/mobile_lan/routes/sync_doc.rs:129` — minimal handler pattern to copy
- `crates/uc-mobile/src/client.rs` — `MobileSyncClient`, port of `SyncClipboardClient.swift`; `put_clipboard` :610, `get_latest` :598
- `crates/uc-application/src/usecases/mobile_sync/sync_clipboard_mapping.rs:82-101` — proof SHA-256 is never persisted/indexed
- `.planning/2026-06-23-contentid-mobile-dedup-design.md` — prior related design (History channel scoped OUT of contentId)
- `.planning/2026-06-23-contentid-rn-integration-guide.md` — style template for the Phase 5 RN handoff doc
- git commits: `526aa4595` (#678, original hash-drift allowance), `6749653e5` (#1159, contentId dedup on native channel)

## Visual/Browser Findings
<!-- none — this task is pure code research/implementation, no browser/image work -->
-

---
*Update this file after every 2 view/browser/search operations*
*This prevents visual information from being lost*
