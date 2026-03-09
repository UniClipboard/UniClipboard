# Deferred Items - Phase 11

## Pre-existing Issues (Out of Scope)

### 1. uc-tauri lib test compilation failure (encryption.rs test module)

**Discovered during:** 11-01 Task 1 execution
**Impact:** Cannot run `cargo test -p uc-tauri --lib` tests. Integration tests (`--test`) still work.
**Root cause:** `src-tauri/crates/uc-tauri/src/commands/encryption.rs` test module has broken imports:

- `uc_core::ports::watcher_control::WatcherControlError` - path no longer exists
- `uc_core::ports::IdentityStoreError` / `IdentityStorePort` - moved to `uc_platform::ports`
- Missing `UiPort` and `AutostartPort` trait imports in test scope

**Workaround:** Tests were placed in `tests/models_serialization_test.rs` (integration test binary) instead of inline `#[cfg(test)]` modules.
**Recommended fix:** Update encryption.rs test imports to use correct paths from uc_platform.
