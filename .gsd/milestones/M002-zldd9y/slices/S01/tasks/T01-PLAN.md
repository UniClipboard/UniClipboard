---
estimated_steps: 7
estimated_files: 3
skills_used: []
---

# T01: Extend PermissionLevel with L3/L4 and add daemon_api_strings constants

1. In permission.rs, add L3Sensitive=3 and L4Dangerous=4 variants to PermissionLevel enum
2. Update from_u8() for values 3 and 4
3. Update existing tests (from_u8_l3_returns_none → from_u8_l3, etc.)
4. In daemon_api_strings.rs: add ws_topic::ENCRYPTION, ws_event::ENCRYPTION_SESSION_READY
5. Add http_route constants: SETTINGS, ENCRYPTION_STATE, ENCRYPTION_UNLOCK, ENCRYPTION_LOCK, STORAGE_STATS, STORAGE_CLEAR_CACHE
6. Add value assertion tests for all new constants
7. In ws.rs, add ws_topic::ENCRYPTION to is_supported_topic() and its test

## Inputs

- `src-tauri/crates/uc-daemon/src/security/permission.rs`
- `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs`
- `src-tauri/crates/uc-daemon/src/api/ws.rs`

## Expected Output

- `PermissionLevel with L1-L4 variants`
- `daemon_api_strings with Phase 76 constants`
- `All tests pass`

## Verification

cd src-tauri && cargo test -p uc-daemon permission -- --nocapture && cargo test -p uc-core daemon_api_strings -- --nocapture && cargo test -p uc-daemon is_supported_topic -- --nocapture
