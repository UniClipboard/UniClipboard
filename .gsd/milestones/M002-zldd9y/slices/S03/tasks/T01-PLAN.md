---
estimated_steps: 5
estimated_files: 1
skills_used: []
---

# T01: Create storage.rs HTTP handler module with stats and clear-cache endpoints

1. Create storage.rs: pub fn router() with GET /storage/stats and POST /storage/clear-cache
2. get_storage_stats_handler: call get_storage_stats().execute(), compute spool_size_bytes from spool_dir, get blob_count from clipboard stats total_count
3. Build StorageStatsResponse DTO with 5 fields: total_size_bytes, blob_count, database_size_bytes, cache_size_bytes, spool_size_bytes
4. clear_cache_handler: parse ClearCacheRequest with confirmed:bool, return 400 confirmation_required if missing/false
5. On confirmed:true, call clear_cache().execute(), return freed_bytes

## Inputs

- `src-tauri/crates/uc-daemon/src/api/clipboard.rs`
- `src-tauri/crates/uc-app/src/usecases/storage/get_storage_stats.rs`
- `src-tauri/crates/uc-app/src/usecases/storage/clear_cache.rs`
- `src-tauri/crates/uc-app/src/app_paths.rs`

## Expected Output

- `src-tauri/crates/uc-daemon/src/api/storage.rs`

## Verification

cd src-tauri && cargo check -p uc-daemon
