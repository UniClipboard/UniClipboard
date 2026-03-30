# S03: Storage Stats & Clear Cache HTTP Handlers

**Goal:** Create storage.rs handler module with stats (including spool_size_bytes and blob_count) and clear-cache with L4 confirmation pattern
**Demo:** After this: GET /storage/stats returns all 5 fields; POST /storage/clear-cache with confirmed:true clears cache; without confirmed returns 400

## Tasks
- [x] **T01: Created storage.rs with GET /storage/stats (5 fields) and POST /storage/clear-cache (confirmation-required pattern)** — 1. Create storage.rs: pub fn router() with GET /storage/stats and POST /storage/clear-cache
2. get_storage_stats_handler: call get_storage_stats().execute(), compute spool_size_bytes from spool_dir, get blob_count from clipboard stats total_count
3. Build StorageStatsResponse DTO with 5 fields: total_size_bytes, blob_count, database_size_bytes, cache_size_bytes, spool_size_bytes
4. clear_cache_handler: parse ClearCacheRequest with confirmed:bool, return 400 confirmation_required if missing/false
5. On confirmed:true, call clear_cache().execute(), return freed_bytes
  - Estimate: 30min
  - Files: src-tauri/crates/uc-daemon/src/api/storage.rs
  - Verify: cd src-tauri && cargo check -p uc-daemon
- [x] **T02: Storage router already registered in routes.rs and mod.rs by T01 — T02 confirmed no delta needed** — 1. In mod.rs, add pub mod storage; (if not already present)
2. In routes.rs router_l2_plus(), merge storage::router()
3. Run full daemon test suite to verify no regressions
  - Estimate: 10min
  - Files: src-tauri/crates/uc-daemon/src/api/routes.rs, src-tauri/crates/uc-daemon/src/api/mod.rs
  - Verify: cd src-tauri && cargo test -p uc-daemon -- --nocapture 2>&1 | tail -20
