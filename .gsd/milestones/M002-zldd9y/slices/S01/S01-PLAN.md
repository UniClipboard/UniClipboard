# S01: Foundation: Permissions, Constants & Unlock Use Case

**Goal:** Extend PermissionLevel with L3/L4, add WS/HTTP route constants to daemon_api_strings, create UnlockEncryptionWithPassphrase use case — building blocks for S02 and S03
**Demo:** After this: PermissionLevel L3/L4 variants exist, daemon_api_strings has all Phase 76 constants, UnlockEncryptionWithPassphrase use case passes unit tests

## Tasks
- [x] **T01: Extend PermissionLevel with L3/L4 and add Phase 76 daemon_api_strings constants** — 1. In permission.rs, add L3Sensitive=3 and L4Dangerous=4 variants to PermissionLevel enum
2. Update from_u8() for values 3 and 4
3. Update existing tests (from_u8_l3_returns_none → from_u8_l3, etc.)
4. In daemon_api_strings.rs: add ws_topic::ENCRYPTION, ws_event::ENCRYPTION_SESSION_READY
5. Add http_route constants: SETTINGS, ENCRYPTION_STATE, ENCRYPTION_UNLOCK, ENCRYPTION_LOCK, STORAGE_STATS, STORAGE_CLEAR_CACHE
6. Add value assertion tests for all new constants
7. In ws.rs, add ws_topic::ENCRYPTION to is_supported_topic() and its test
  - Estimate: 20min
  - Files: src-tauri/crates/uc-daemon/src/security/permission.rs, src-tauri/crates/uc-core/src/network/daemon_api_strings.rs, src-tauri/crates/uc-daemon/src/api/ws.rs
  - Verify: cd src-tauri && cargo test -p uc-daemon permission -- --nocapture && cargo test -p uc-core daemon_api_strings -- --nocapture && cargo test -p uc-daemon is_supported_topic -- --nocapture
- [x] **T02: Created UnlockEncryptionWithPassphrase use case with 8 unit tests and CoreUseCases accessor** — 1. Create unlock_encryption_with_passphrase.rs in uc-app/src/usecases/
2. Define UnlockWithPassphraseError enum (NotInitialized, ScopeFailed, KeySlotLoadFailed, MissingWrappedMasterKey, KekDeriveFailed, UnwrapFailed, SessionSetFailed)
3. Implement execute(passphrase: Passphrase) flow: check state → get scope → load keyslot → derive KEK → unwrap master key → set session
4. Do NOT derive Debug on anything holding Passphrase
5. Add unit tests: uninitialized error, success path, wrong passphrase error
6. Add pub mod and re-export in mod.rs
7. Add CoreUseCases::unlock_encryption_with_passphrase() accessor
  - Estimate: 30min
  - Files: src-tauri/crates/uc-app/src/usecases/unlock_encryption_with_passphrase.rs, src-tauri/crates/uc-app/src/usecases/mod.rs
  - Verify: cd src-tauri && cargo test -p uc-app unlock_encryption_with_passphrase -- --nocapture
